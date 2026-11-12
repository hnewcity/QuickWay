use crate::{auth, models, session::SessionManager};
use anyhow::{Context, Result, bail};
use reqwest::{Client, Response, Url};
use serde_json::{Value, json};
use std::{collections::VecDeque, path::PathBuf, sync::Mutex, time::Duration};

const RECENT_ERRORS: usize = 20;

pub struct Upstream {
    pub cloud: crate::cloud::Manager,
    pub client: Client,
    pub sessions: SessionManager,
    directory: models::Directory,
    recent_errors: Mutex<VecDeque<Value>>,
}
impl Upstream {
    pub fn new(base: PathBuf) -> Result<Self> {
        Ok(Self {
            cloud: crate::cloud::Manager::default(),
            client: Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("QuickWay/", env!("CARGO_PKG_VERSION")))
                .build()?,
            sessions: SessionManager::new(base),
            directory: models::Directory::default(),
            recent_errors: Mutex::default(),
        })
    }
    pub fn with_model_cache(mut self, path: PathBuf) -> Self {
        self.directory = models::Directory::with_cache(path);
        self
    }
    pub fn with_credential_file(mut self, path: PathBuf) -> Self {
        self.sessions = self.sessions.with_credential_file(path);
        self
    }
    pub async fn last_cloud_model(&self) -> Option<crate::cloud::ModelObservation> {
        let identity = self.sessions.catalog_identity().await.ok()?;
        self.cloud.last_model(identity).await
    }

    pub async fn models(&self, force: bool) -> models::Catalog {
        let identity = self.sessions.catalog_identity().await.ok();
        let mut catalog =
            self.directory
                .get(identity, force, || async {
                    tokio::time::timeout(Duration::from_secs(15), async {
                        let mut response = self.post("/integration/quick-work/qbs-event", &json!({
                    "eventType":"invokeAgent", "eventBody":{"invokeAgent":{
                        "path":"config/all", "queryParams":{"_method":"GET"}, "body":{}
                    }}
                })).await.map_err(|error| error.to_string())?;
                        if !response.status().is_success() {
                            return Err(format!(
                                "模型目录刷新失败（HTTP {}）。",
                                response.status().as_u16()
                            ));
                        }
                        let mut bytes = Vec::new();
                        while let Some(chunk) =
                            response.chunk().await.map_err(|_| "读取模型目录失败。")?
                        {
                            if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                                return Err("模型目录过大。".into());
                            }
                            bytes.extend_from_slice(&chunk);
                        }
                        let body: Value =
                            serde_json::from_slice(&bytes).map_err(|_| "模型目录格式无效。")?;
                        // Quick can switch accounts while the network request is in flight.
                        if self.sessions.catalog_identity().await.ok() != identity {
                            return Err("Quick 登录已变更，请刷新模型目录。".into());
                        }
                        models::parse_config(&body)
                    })
                    .await
                    .unwrap_or_else(|_| Err("模型目录刷新超时，稍后会自动重试。".into()))
                })
                .await;
        if self.sessions.catalog_identity().await.ok() != identity {
            return self
                .directory
                .get(None, false, || async { unreachable!() })
                .await;
        }
        if self
            .sessions
            .session(&self.client)
            .await
            .is_ok_and(|session| !session.managed_by_quick)
        {
            catalog.models.extend(crate::cloud::models());
        }
        catalog
    }
    pub async fn post(&self, path: &str, body: &Value) -> Result<Response> {
        let s = match self.sessions.session(&self.client).await {
            Ok(session) => session,
            Err(error) => {
                self.record(path, json!({"error":error.to_string()}));
                return Err(error);
            }
        };
        let url = tenant_url(&s, path)?;
        let response = authorized(&self.client, url, &s.access_token)
            .json(body)
            .send()
            .await
            .map_err(|error| {
                self.record(path, json!({"error":describe_send_error(&error)}));
                anyhow::anyhow!("Quick upstream connection failed or timed out")
            })?;
        if !response.status().is_success() {
            self.record(path, json!({"status":response.status().as_u16()}));
        }
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.sessions.reject(&s).await;
        }
        Ok(response)
    }

    /// Remember failed upstream calls (path, status or our own error text,
    /// never request or response content) for the diagnostics report.
    fn record(&self, path: &str, mut detail: Value) {
        detail["at"] = auth::now().round().into();
        detail["path"] = path.into();
        let mut recent = self.recent_errors.lock().unwrap_or_else(|e| e.into_inner());
        if recent.len() == RECENT_ERRORS {
            recent.pop_front();
        }
        recent.push_back(detail);
    }

    pub fn recent_errors(&self) -> Vec<Value> {
        let recent = self.recent_errors.lock().unwrap_or_else(|e| e.into_inner());
        recent.iter().cloned().collect()
    }

    pub async fn usage(&self) -> Result<Value> {
        let response = self
            .post("/integration/quick-work/user-info", &json!({}))
            .await?;
        let status = response.status();
        if !status.is_success() {
            bail!("Quick session validation returned HTTP {}", status.as_u16());
        }
        let value: Value = response
            .json()
            .await
            .context("Invalid Quick user-info response")?;
        Ok(value.get("usageSummary").cloned().unwrap_or(Value::Null))
    }
}

/// Headers every Quick App Integrations call carries, matching Quick's own client.
pub(crate) fn authorized(client: &Client, url: Url, bearer: &str) -> reqwest::RequestBuilder {
    client
        .post(url)
        .bearer_auth(bearer)
        .header("X-QuickDsk-mdid", "user-conversation")
        .header("x-amzn-quick-atype", "ufg")
}

/// Classify a transport failure without echoing the URL.
pub(crate) fn describe_send_error(error: &reqwest::Error) -> String {
    let kind = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_request() {
        "request"
    } else {
        "transport"
    };
    let mut detail = String::new();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        detail = cause.to_string();
        source = cause.source();
    }
    if detail.is_empty() {
        kind.into()
    } else {
        format!("{kind}: {detail}")
    }
}

pub(crate) fn tenant_url(s: &auth::QuickSession, path: &str) -> Result<Url> {
    let mut url = Url::parse(&s.tenant_url).context("Invalid Quick tenant URL")?;
    let host = url.host_str().context("Quick tenant has no hostname")?;
    if url.scheme() != "https"
        || !host.ends_with(".prod.plato.ai.aws.dev")
        || !host.contains(".dp.appintegrations.")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        bail!("This build supports Quick's production App Integrations tenant only")
    }
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

pub(crate) fn error_kind(status: u16) -> &'static str {
    match status {
        400 | 413 | 422 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        429 => "rate_limit_error",
        503 | 529 => "overloaded_error",
        _ => "api_error",
    }
}

pub async fn error_body(mut response: Response) -> (u16, Value, Option<String>) {
    let status = response.status().as_u16();
    let retry = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let code = response
        .headers()
        .get("x-amzn-errortype")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(error_kind(status))
        .to_owned();
    // Error bodies can contain full request content or be arbitrarily large.
    // Read a bounded JSON object and expose only scrubbed diagnostic fields.
    const LIMIT: usize = 16 * 1024;
    let mut bytes = Vec::new();
    let payload = loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= LIMIT => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(None) => break serde_json::from_slice::<Value>(&bytes).ok(),
            _ => break None,
        }
    };
    let error = if let Some(payload) = payload.filter(Value::is_object) {
        crate::model_error::ModelError::from_upstream(&code, &payload).event()
    } else {
        json!({"type":"error","error":{"type":error_kind(status),"message":format!("Quick upstream returned HTTP {status}; no readable error details were returned.")}})
    };
    (status, error, retry)
}

#[cfg(test)]
mod error_tests {
    use super::*;

    #[tokio::test]
    async fn http_errors_keep_safe_details_and_retry_after() {
        let response = axum::http::Response::builder().status(403)
            .header("retry-after", "30")
            .body(json!({"__type":"com.aws#AccessDeniedException", "message":"This client is not supported for this request.", "request":"private content"}).to_string())
            .unwrap().into();
        let (status, body, retry) = error_body(response).await;
        assert_eq!(status, 403);
        assert_eq!(retry.as_deref(), Some("30"));
        assert_eq!(body["error"]["type"], "permission_error");
        assert_eq!(body["error"]["code"], "AccessDeniedException");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("This client is not supported")
        );
        assert!(!body.to_string().contains("private content"));
    }

    #[tokio::test]
    async fn non_json_and_oversized_http_errors_never_echo_bodies() {
        for body in [
            "<html>secret</html>".to_owned(),
            json!({"message":"secret ".repeat(3000)}).to_string(),
        ] {
            let response = axum::http::Response::builder()
                .status(503)
                .body(body)
                .unwrap()
                .into();
            let (status, body, _) = error_body(response).await;
            assert_eq!(status, 503);
            assert_eq!(body["error"]["type"], "overloaded_error");
            assert!(!body.to_string().contains("secret"));
        }
    }
}
