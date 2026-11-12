//! Support report for sign-in and upstream failures. It may be pasted into a
//! chat or issue, so it must never contain token values, client secrets,
//! email addresses, user names or request/response content beyond short,
//! scrubbed upstream error bodies.
use crate::{
    auth,
    upstream::{self, Upstream},
};
use base64::Engine;
use futures_util::StreamExt;
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

const USER_INFO: &str = "/integration/quick-work/user-info";
const MODEL_CONFIG: &str = "/integration/quick-work/qbs-event";
/// JWT claims that describe the token rather than the person.
const SAFE_CLAIMS: &[&str] = &[
    "iss",
    "aud",
    "azp",
    "client_id",
    "token_use",
    "typ",
    "scope",
    "scp",
    "ver",
    "amr",
];
const TIME_CLAIMS: &[&str] = &["exp", "iat", "nbf", "auth_time"];
const RESPONSE_HEADERS: &[&str] = &[
    "content-type",
    "date",
    "www-authenticate",
    "x-amzn-requestid",
    "x-amz-request-id",
    "x-amzn-errortype",
    "x-amzn-error-message",
    "x-amz-apigw-id",
    "x-amzn-trace-id",
    "x-cache",
];
const BODY_LIMIT: usize = 800;

/// Build the report. `online` sends read-only requests (user info, model
/// catalog) to Quick; nothing is written to Quick's credential store, and the
/// probes do not mark the current credential as rejected.
pub async fn report(upstream: &Upstream, app_version: &str, online: bool) -> Value {
    let now = auth::now();
    let base = upstream.sessions.quick_base();
    let source = upstream.sessions.credential_source().await;
    let mut report = json!({
        "report": "quickway.diagnostics",
        "version": 1,
        "generated_at": now.round(),
        "app": {"version": app_version, "core": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "arch": std::env::consts::ARCH},
        "credential_source": source,
        "quick_profile": auth::profile_summary(base),
        "inference": {"status":"not_checked"},
    });

    // The local credential is inspected even when an import is active, so a
    // mismatch between the two is visible.
    let local = auth::read_local_credential(base);
    let mut alternates: Vec<(&'static str, String, f64)> = Vec::new();
    report["local_credential"] = match &local {
        Ok(credential) => {
            let (summary, tokens) = inspect_credential(&credential.raw, credential.enterprise, now);
            let mut summary = summary;
            summary["records"] = credential.records.into();
            alternates = tokens;
            summary
        }
        Err(error) => json!({"error": format!("{error:#}")}),
    };

    let session = upstream.sessions.session(&upstream.client).await;
    report["session"] = match &session {
        Ok(session) => json!({
            "ok": true,
            "managed_by_quick": session.managed_by_quick,
            "bearer": token_shape(&session.access_token, now),
            "bearer_expires_in_s": (session.access_token_expiry - now).round(),
            "refresh_token_present": !session.refresh_token.is_empty(),
            "refresh_token_expires_in_s": (session.refresh_token_expiry - now).round(),
            "tenant": url_summary(&session.tenant_url),
            "tenant_supported": upstream::tenant_url(session, "/").is_ok(),
            "issuer": url_summary(&session.issuer_url),
            "aws_account": mask_tail(&session.aws_account_id),
        }),
        Err(error) => json!({"ok": false, "error": format!("{error:#}")}),
    };
    if let (Ok(session), Some(local)) = (&session, report["local_credential"].as_object_mut()) {
        if let Some((field, _, _)) = alternates
            .iter()
            .find(|(_, token, _)| *token == session.access_token)
        {
            local.insert("bearer_field".into(), (*field).into());
        } else if source == "local" {
            local.insert(
                "bearer_field".into(),
                "none (session differs from stored token)".into(),
            );
        }
    }

    if online {
        let mut probes = Map::new();
        match &session {
            Ok(session) => match upstream::tenant_url(session, USER_INFO) {
                Ok(url) => {
                    let secrets = secret_values(session, &alternates);
                    probes.insert(
                        "user_info".into(),
                        probe(
                            upstream,
                            url.clone(),
                            &session.access_token,
                            json!({}),
                            &secrets,
                        )
                        .await,
                    );
                    // Try the other tokens Quick stored, so one report shows
                    // whether the server would accept a different bearer.
                    if source == "local" {
                        for (field, token, expiry) in &alternates {
                            if *token != session.access_token && !token.is_empty() {
                                let mut result =
                                    probe(upstream, url.clone(), token, json!({}), &secrets).await;
                                result["stored_expires_in_s"] = (expiry - now).round().into();
                                probes.insert(format!("user_info_with_{field}"), result);
                            }
                        }
                    }
                    if let Ok(url) = upstream::tenant_url(session, MODEL_CONFIG) {
                        let body = json!({"eventType":"invokeAgent","eventBody":{"invokeAgent":{"path":"config/all","queryParams":{"_method":"GET"},"body":{}}}});
                        let mut result =
                            probe(upstream, url, &session.access_token, body, &secrets).await;
                        // The catalog itself is large and not needed here.
                        if result["status"]
                            .as_u64()
                            .is_some_and(|s| (200..300).contains(&s))
                        {
                            result.as_object_mut().unwrap().remove("body");
                        }
                        probes.insert("model_catalog".into(), result);
                    }
                }
                Err(error) => {
                    probes.insert("skipped".into(), format!("{error:#}").into());
                }
            },
            Err(_) => {
                probes.insert("skipped".into(), "no usable session".into());
            }
        }
        report["online"] = Value::Object(probes);
    }
    report["recent_errors"] = upstream.recent_errors().into();
    report
}

/// An explicit, quota-consuming check. Its 30-second budget includes catalog
/// resolution, sending, reading the complete stream and protocol validation.
/// Never expose generated text in a diagnostics report.
pub async fn probe_model(upstream: &Upstream, name: &str) -> Value {
    let started = Instant::now();
    let cloud_mode = crate::cloud::mode(name);
    let mut result = tokio::time::timeout(Duration::from_secs(30), async {
        if let Some(mode) = cloud_mode {
            let outcome = crate::cloud::dispatch(upstream, inference_body(name, false), mode, Some("low")).await;
            let mut result = match outcome {
                Ok(crate::server::Outcome::Message(message)) => inspect_cloud_message(&message),
                Ok(crate::server::Outcome::Stream(_)) => json!({"status":"failed","ok":false,"error":{
                    "type":"api_error", "message":"Quick cloud diagnostic request unexpectedly returned an uncollected stream."
                }}),
                Err(failure) => {
                    let error = crate::model_error::ModelError::from_upstream(
                        upstream::error_kind(failure.status), &json!({"message":failure.message}));
                    json!({"status":"failed","ok":false,"http_status":failure.status,"error":error.event()["error"]})
                }
            };
            result["model"] = name.into();
            result["transport"] = "quick_cloud".into();
            return result;
        }
        let catalog = upstream.models(false).await;
        let (model, route) = match catalog.resolve(name) {
            Ok(resolved) => resolved,
            Err(_) => return json!({"status":"failed","ok":false,"error":{
                "type":"invalid_request_error", "message":"Requested model is not in the current Quick model directory. Query /v1/models or refresh the model catalog."
            }}),
        };
        let payload = match inference_payload(&model, &route) {
            Ok(payload) => payload,
            Err(_) => return json!({"status":"failed","ok":false,"model":model,"error":{
                "type":"invalid_request_error", "message":"Could not build the model diagnostic request."
            }}),
        };
        let response = match upstream.post("/integration/quick-work/bedrock-proxy-stream", &payload).await {
            Ok(response) => response,
            Err(_) => return json!({"status":"failed","ok":false,"model":model,"error":{
                "type":"api_error", "message":"Quick inference connection failed or no usable session is available."
            }}),
        };
        let mut result = inspect_inference_response(response, &model).await;
        result["model"] = model.into();
        result
    }).await.unwrap_or_else(|_| json!({"status":"timeout","ok":false,"error":{
        "type":"api_error", "message":"Quick inference check exceeded its 30-second deadline."
    }}));
    if cloud_mode.is_some() {
        // Dropping a timed-out dispatch does not stop its conversation task.
        // Explicitly clean up temporary cloud devices on success and failure.
        upstream.cloud.shutdown().await;
        result["model"] = name.into();
        result["transport"] = "quick_cloud".into();
    }
    result["elapsed_ms"] = (started.elapsed().as_millis() as u64).into();
    result
}

fn inference_body(model: &str, stream: bool) -> Value {
    json!({
        "model":model,"stream":stream,"max_tokens":32,
        "messages":[{"role":"user","content":"Reply OK."}]
    })
}

fn inference_payload(model: &str, route: &str) -> Result<Value, String> {
    let mut body = inference_body(model, true);
    crate::thinking::Effort::Off.apply(&mut body, model)?;
    let mut request = crate::converse::request(body, route)?;
    request
        .as_object_mut()
        .expect("Converse request is an object")
        .remove("modelId");
    Ok(json!({"modelId":route,"requestBody":request}))
}

fn inspect_cloud_message(message: &Value) -> Value {
    let has_text = message["content"].as_array().is_some_and(|blocks| {
        blocks.iter().any(|block| {
            block["type"] == "text"
                && block["text"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
        })
    });
    let has_usage = ["input_tokens", "output_tokens"]
        .iter()
        .all(|key| message["usage"][key].as_u64().is_some());
    if message["type"] != "message"
        || message["role"] != "assistant"
        || message["stop_reason"] != "end_turn"
        || !has_text
        || !has_usage
    {
        return json!({"status":"failed","ok":false,"error":{
            "type":"api_error", "message":"Quick cloud inference did not finish a complete text reply with token usage."
        }});
    }
    let mut usage = Map::new();
    for key in [
        "input_tokens",
        "output_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ] {
        if let Some(value) = message["usage"][key].as_u64() {
            usage.insert(key.into(), value.into());
        }
    }
    let mut result =
        json!({"status":"succeeded","ok":true,"completed":true,"received_text":true,"usage":usage});
    // Only explicit backend identifiers are reportable; never copy an entire
    // response object or arbitrary metadata into a diagnostics report.
    let mut metadata = Map::new();
    for key in ["mode", "resolved_model", "served_model"] {
        if let Some(value) = message["quickway"][key].as_str().filter(|value| {
            !value.is_empty()
                && value.len() <= 512
                && !value.contains("://")
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._:/-".contains(&b))
        }) {
            metadata.insert(key.into(), value.into());
        }
    }
    if !metadata.is_empty() {
        result["quickway"] = metadata.into();
    }
    result
}

async fn inspect_inference_response(mut response: reqwest::Response, model: &str) -> Value {
    let status = response.status().as_u16();
    if !response.status().is_success() {
        let (_, body, _) = upstream::error_body(response).await;
        return json!({"status":"failed","ok":false,"http_status":status,"error":body["error"]});
    }
    let content_type = response.headers().get("content-type").cloned();
    const MAX_RESPONSE: usize = 512 * 1024;
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= MAX_RESPONSE => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(Some(_)) => {
                return json!({"status":"failed","ok":false,"http_status":status,"error":{
                    "type":"api_error", "message":"Quick inference response exceeded the 512 KiB diagnostic limit."
                }});
            }
            Ok(None) => break,
            Err(_) => {
                return json!({"status":"failed","ok":false,"http_status":status,"error":{
                    "type":"api_error", "message":"Quick inference stream disconnected before completion."
                }});
            }
        }
    }
    // Reuse the production stream decoder after a bounded read. This accepts
    // EventStream, NDJSON and JSON arrays and requires valid terminal metadata.
    let mut response = axum::http::Response::builder();
    if let Some(content_type) = content_type {
        response = response.header("content-type", content_type);
    }
    let response = response.body(bytes).expect("valid bounded response").into();
    let events = crate::streaming::event_stream(response, model.to_owned());
    futures_util::pin_mut!(events);
    let mut complete = false;
    let mut has_text = false;
    let mut natural_stop = false;
    let mut usage = Value::Null;
    while let Some(event) = events.next().await {
        match event["type"].as_str() {
            Some("error") => {
                return json!({"status":"failed","ok":false,"http_status":status,"error":event["error"]});
            }
            Some("content_block_delta") => {
                has_text |= event["delta"]["text"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty());
            }
            Some("message_delta") => {
                natural_stop = event["delta"]["stop_reason"] == "end_turn";
                usage = event["usage"].clone();
            }
            Some("message_stop") => complete = true,
            _ => {}
        }
    }
    if complete && natural_stop && has_text {
        json!({"status":"succeeded","ok":true,"http_status":status,"completed":true,"received_text":true,"usage":usage})
    } else {
        json!({"status":"failed","ok":false,"http_status":status,"error":{
            "type":"api_error", "message":"Quick inference did not finish a complete text reply within the diagnostic token limit."
        }})
    }
}

/// Summarize Quick's credential JSON: field names, expiries and token shapes.
/// Returns the bearer candidates so callers can probe them; the values never
/// enter the summary.
type Candidates = Vec<(&'static str, String, f64)>;

fn inspect_credential(raw: &str, enterprise: bool, now: f64) -> (Value, Candidates) {
    let value: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(_) => return (json!({"error":"credential is not JSON"}), Vec::new()),
    };
    let Some(object) = value.as_object() else {
        return (
            json!({"error":"credential is not a JSON object"}),
            Vec::new(),
        );
    };
    let mut fields: Vec<_> = object.keys().cloned().collect();
    fields.sort();
    let mut expiries = Map::new();
    for (key, value) in object {
        if (key.ends_with("_expiry") || key.ends_with("expires_at")) && value.is_number() {
            let at = value.as_f64().unwrap_or_default();
            let mut entry = json!({"expires_in_s": (at - now).round()});
            if at > 1e11 {
                entry["looks_like_milliseconds"] = true.into();
            }
            expiries.insert(key.clone(), entry);
        } else if key.ends_with("_expiry") || key.ends_with("expires_at") {
            expiries.insert(key.clone(), json!({"type": type_name(value)}));
        }
    }
    let mut tokens = Map::new();
    let mut candidates = Vec::new();
    for field in ["access_token", "id_token", "refresh_token"] {
        let shape = match object.get(field) {
            None => json!({"present": false}),
            Some(Value::String(token)) => token_shape(token, now),
            Some(other) => json!({"present": true, "type": type_name(other)}),
        };
        tokens.insert(field.into(), shape);
        if field != "refresh_token"
            && let Some(Value::String(token)) = object.get(field)
        {
            let name = if field == "id_token" {
                "id_token"
            } else {
                "access_token"
            };
            let expiry = object
                .get(&format!("{field}_expiry"))
                .and_then(Value::as_f64)
                .unwrap_or_default();
            candidates.push((name, token.clone(), expiry));
        }
    }
    let mut summary = json!({
        "provider": if enterprise { "enterprise" } else { "social" },
        "fields": fields,
        "expiries": expiries,
        "tokens": tokens,
        "parses": auth::parse_local_session(raw, enterprise).map(|_| Value::Bool(true)).unwrap_or_else(|e| e.to_string().into()),
    });
    for (key, label) in [
        ("tenant_url", "tenant"),
        ("issuer", "issuer"),
        ("issuer_url", "issuer"),
        ("token_endpoint", "token_endpoint"),
    ] {
        if let Some(url) = object.get(key).and_then(Value::as_str) {
            summary[label] = url_summary(url);
        }
    }
    for key in ["region", "auth_region", "idp_type", "provider_type"] {
        if let Some(value) = object.get(key).filter(|v| v.is_string()) {
            summary[key] = value.clone();
        }
    }
    (summary, candidates)
}

/// Length, JWT header and non-personal claims. Never the token itself.
fn token_shape(token: &str, now: f64) -> Value {
    if token.is_empty() {
        return json!({"present": true, "empty": true});
    }
    let mut shape = json!({"present": true, "length": token.len()});
    let parts: Vec<_> = token.split('.').collect();
    let decode = |part: &str| -> Option<Value> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(part.trim_end_matches('='))
            .ok()?;
        serde_json::from_slice(&bytes).ok()
    };
    let (Some(header), Some(claims)) = (
        parts.first().and_then(|p| decode(p)),
        parts.get(1).and_then(|p| decode(p)),
    ) else {
        shape["format"] = "opaque".into();
        return shape;
    };
    shape["format"] = if parts.len() == 3 {
        "jwt"
    } else {
        "jwe-or-other"
    }
    .into();
    shape["header"] = json!({"alg": header["alg"], "typ": header["typ"]});
    if let Some(claims) = claims.as_object() {
        let mut keys: Vec<_> = claims.keys().cloned().collect();
        keys.sort();
        shape["claim_keys"] = keys.into();
        let mut safe = Map::new();
        for key in SAFE_CLAIMS {
            if let Some(value) = claims.get(*key) {
                safe.insert((*key).into(), value.clone());
            }
        }
        for key in TIME_CLAIMS {
            if let Some(at) = claims.get(*key).and_then(Value::as_f64) {
                safe.insert(format!("{key}_in_s"), (at - now).round().into());
            }
        }
        shape["claims"] = Value::Object(safe);
    }
    shape
}

async fn probe(
    upstream: &Upstream,
    url: reqwest::Url,
    bearer: &str,
    body: Value,
    secrets: &[String],
) -> Value {
    let started = Instant::now();
    let sent = upstream::authorized(&upstream.client, url, bearer)
        .timeout(Duration::from_secs(20))
        .json(&body)
        .send()
        .await;
    let elapsed = started.elapsed().as_millis() as u64;
    let response = match sent {
        Ok(response) => response,
        Err(error) => {
            return json!({"error": upstream::describe_send_error(&error), "elapsed_ms": elapsed});
        }
    };
    let mut headers = Map::new();
    for name in RESPONSE_HEADERS {
        if let Some(value) = response.headers().get(*name).and_then(|v| v.to_str().ok()) {
            headers.insert((*name).into(), scrub(value, secrets).into());
        }
    }
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    let mut response = response;
    while let Ok(Some(chunk)) = response.chunk().await {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > BODY_LIMIT * 4 {
            break;
        }
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut body: String = scrub(&text, secrets).chars().take(BODY_LIMIT).collect();
    if text.chars().count() > BODY_LIMIT {
        body.push('…');
    }
    let mut result = json!({"status": status, "elapsed_ms": elapsed, "headers": headers, "body_bytes": bytes.len()});
    // Successful user-info carries account details; only error bodies are kept.
    if !(200..300).contains(&status) {
        result["body"] = body.into();
    } else if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
        result["body_keys"] = value
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
            .into();
    }
    result
}

fn secret_values(session: &auth::QuickSession, alternates: &Candidates) -> Vec<String> {
    let mut secrets: Vec<String> = [
        &session.access_token,
        &session.refresh_token,
        &session.client_secret,
    ]
    .into_iter()
    .chain(alternates.iter().map(|(_, token, _)| token))
    .filter(|s| s.len() >= 8)
    .cloned()
    .collect();
    secrets.sort();
    secrets.dedup();
    secrets
}

/// Remove known secrets and anything shaped like a JWT or an email address.
fn scrub(text: &str, secrets: &[String]) -> String {
    let mut text = text.to_owned();
    for secret in secrets {
        text = text.replace(secret.as_str(), "[redacted]");
    }
    text.split_inclusive(|c: char| c.is_whitespace() || "\"',;:()[]{}<>".contains(c))
        .map(|piece| {
            let word =
                piece.trim_end_matches(|c: char| c.is_whitespace() || "\"',;:()[]{}<>".contains(c));
            let tail = &piece[word.len()..];
            if (word.starts_with("eyJ") && word.len() > 30)
                || (word.contains('@') && word.contains('.') && word.len() > 5)
            {
                format!("[redacted]{tail}")
            } else {
                piece.to_owned()
            }
        })
        .collect()
}

/// Scheme and host, with the tenant-specific first label shortened.
fn url_summary(url: &str) -> Value {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return json!({"valid": false});
    };
    let host = parsed.host_str().unwrap_or_default();
    let masked = match host.split_once('.') {
        Some((first, rest)) if first.len() > 4 => {
            format!("{}…({} chars).{rest}", &first[..4], first.len())
        }
        _ => host.to_owned(),
    };
    // Long path segments are instance identifiers (e.g. /ssoins-…); keep a prefix only.
    let path = parsed
        .path()
        .split('/')
        .map(|segment| match segment.char_indices().nth(8) {
            Some((cut, _)) if segment.len() > 12 => format!("{}…", &segment[..cut]),
            _ => segment.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("/");
    json!({"scheme": parsed.scheme(), "host": masked, "port": parsed.port(), "path": path})
}

fn mask_tail(value: &str) -> Value {
    if value.is_empty() {
        return "".into();
    }
    let tail: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail} ({} chars)", value.chars().count()).into()
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_stream(stop: &str) -> Vec<Value> {
        vec![
            json!({"eventType":"messageStart","payload":{"role":"assistant"}}),
            json!({"eventType":"contentBlockDelta","payload":{"contentBlockIndex":0,"delta":{"text":"GENERATED-CONTENT-PRIVATE"}}}),
            json!({"eventType":"contentBlockStop","payload":{"contentBlockIndex":0}}),
            json!({"eventType":"messageStop","payload":{"stopReason":stop}}),
            json!({"eventType":"metadata","payload":{"usage":{"inputTokens":6,"outputTokens":1}}}),
        ]
    }

    fn model_response(body: Value) -> reqwest::Response {
        axum::http::Response::builder()
            .header("content-type", "application/json")
            .body(body.to_string())
            .unwrap()
            .into()
    }

    #[tokio::test]
    async fn inference_probe_reports_completion_and_usage_without_generated_content() {
        let result =
            inspect_inference_response(model_response(json!(model_stream("end_turn"))), "fixture")
                .await;
        assert_eq!(result["status"], "succeeded");
        assert_eq!(result["completed"], true);
        assert_eq!(result["usage"]["output_tokens"], 1);
        assert!(!result.to_string().contains("GENERATED-CONTENT-PRIVATE"));
    }

    #[test]
    fn cloud_probe_reports_usage_and_backend_without_generated_content() {
        let message = json!({
            "id":"PRIVATE-RESPONSE-ID", "type":"message", "role":"assistant", "stop_reason":"end_turn",
            "content":[{"type":"text","text":"GENERATED-CONTENT-PRIVATE"}],
            "usage":{"input_tokens":4,"output_tokens":2,"cache_read_input_tokens":1,"private":"PRIVATE-USAGE"},
            "quickway":{"mode":"smart","resolved_model":"us.anthropic.claude-opus-5-5",
                "served_model":"https://private.example/token","private":"PRIVATE-METADATA"}
        });
        let result = inspect_cloud_message(&message);
        assert_eq!(result["status"], "succeeded");
        assert_eq!(
            result["usage"],
            json!({"input_tokens":4,"output_tokens":2,"cache_read_input_tokens":1})
        );
        assert_eq!(
            result["quickway"],
            json!({"mode":"smart","resolved_model":"us.anthropic.claude-opus-5-5"})
        );
        assert!(!result.to_string().contains("PRIVATE"));
        assert!(!result.to_string().contains("private.example"));
        for (key, value) in [
            ("stop_reason", json!("tool_use")),
            ("usage", json!({"input_tokens":4})),
            ("content", json!([])),
        ] {
            let mut partial = message.clone();
            partial[key] = value;
            let result = inspect_cloud_message(&partial);
            assert_eq!(result["status"], "failed");
            assert!(!result.to_string().contains("PRIVATE"));
        }
    }

    #[test]
    fn cloud_probe_uses_a_non_streaming_synthetic_request() {
        for mode in ["fast", "balanced", "smart", "advanced"] {
            let body = inference_body(&format!("quick-{mode}"), false);
            let request =
                crate::cloud_request::ValidatedRequest::from_body(body, mode, Some("low")).unwrap();
            assert_eq!(request.body["stream"], false);
            assert_eq!(request.turn.text, "Reply OK.");
            assert_eq!(request.requested_max_tokens, Some(32));
            assert_eq!(request.thinking_effort, "low");
            assert!(request.tools.is_empty());
        }
    }

    #[tokio::test]
    async fn inference_probe_rejects_truncation_token_limits_and_http_200_errors() {
        let mut missing_metadata = model_stream("end_turn");
        missing_metadata.pop();
        let mut interrupted = model_stream("end_turn");
        interrupted.truncate(2);
        for stream in [missing_metadata, interrupted, model_stream("max_tokens")] {
            let result = inspect_inference_response(model_response(json!(stream)), "fixture").await;
            assert_eq!(result["status"], "failed");
            assert_eq!(result["ok"], false);
            assert!(!result.to_string().contains("GENERATED-CONTENT-PRIVATE"));
        }
        let denial = json!({"eventType":"error","payload":{
            "errorType":"AccessDeniedException","error":"This client is not supported for this request."
        }});
        let result = inspect_inference_response(model_response(denial), "fixture").await;
        assert_eq!(result["http_status"], 200);
        assert_eq!(result["status"], "failed");
        assert_eq!(result["error"]["type"], "permission_error");
        assert_eq!(result["error"]["code"], "AccessDeniedException");
    }

    #[tokio::test]
    async fn inference_probe_bounds_success_status_bodies() {
        let response = model_response(json!({"large":"PRIVATE".repeat(100_000)}));
        let result = inspect_inference_response(response, "fixture").await;
        assert_eq!(result["status"], "failed");
        assert!(
            result["error"]["message"]
                .as_str()
                .unwrap()
                .contains("512 KiB")
        );
        assert!(!result.to_string().contains("PRIVATE"));
    }

    #[test]
    fn inference_probe_keeps_a_small_shared_converse_request() {
        for (model, route) in [
            ("claude-sonnet-4-6", "global.anthropic.claude-sonnet-4-6"),
            ("gpt-6-astra", "us.openai.gpt-6-astra"),
        ] {
            let payload = inference_payload(model, route).unwrap();
            assert_eq!(payload["modelId"], route);
            assert_eq!(payload["requestBody"]["inferenceConfig"]["maxTokens"], 32);
            assert!(payload["requestBody"].get("modelId").is_none());
            assert!(payload["requestBody"].get("toolConfig").is_none());
        }
    }

    fn jwt(claims: Value) -> String {
        let encode =
            |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        format!(
            "{}.{}.signature-part",
            encode(&json!({"alg":"RS256","typ":"JWT"})),
            encode(&claims)
        )
    }

    #[test]
    fn credential_summary_never_contains_secrets_or_personal_claims() {
        let id = jwt(
            json!({"iss":"https://idp.example.invalid","aud":"client-1","exp":1100.0,"email":"person@example.invalid","sub":"user-123","name":"Person Name"}),
        );
        let raw = json!({
            "access_token":"opaque-access-SECRET-VALUE",
            "access_token_expiry":2000.0,
            "id_token":id,
            "id_token_expiry":1100.0,
            "refresh_token":"refresh-SECRET-VALUE",
            "refresh_token_expiry":1_700_000_000_000.0,
            "client_secret":"client-SECRET-VALUE",
            "tenant_url":"https://abcdef123.dp.appintegrations.us-east-1.prod.plato.ai.aws.dev",
            "issuer":"https://idp.example.invalid/oauth",
            "aws_account_id":"123456789012",
            "bad_expiry":"soon"
        })
        .to_string();
        let (summary, candidates) = inspect_credential(&raw, true, 1000.0);
        let text = summary.to_string();
        for secret in [
            "SECRET",
            "person@example",
            "user-123",
            "Person Name",
            "abcdef123",
            "123456789012",
            &id,
        ] {
            assert!(!text.contains(secret), "leaked {secret}: {text}");
        }
        assert_eq!(summary["tokens"]["id_token"]["format"], "jwt");
        assert_eq!(summary["tokens"]["id_token"]["claims"]["aud"], "client-1");
        assert_eq!(summary["tokens"]["id_token"]["claims"]["exp_in_s"], 100.0);
        assert_eq!(summary["tokens"]["access_token"]["format"], "opaque");
        assert_eq!(
            summary["expiries"]["refresh_token_expiry"]["looks_like_milliseconds"],
            true
        );
        assert_eq!(summary["expiries"]["bad_expiry"]["type"], "string");
        assert_eq!(summary["parses"], true);
        assert!(
            summary["fields"]
                .as_array()
                .unwrap()
                .contains(&"client_secret".into())
        );
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn scrub_removes_tokens_and_emails_but_keeps_error_text() {
        let token = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ4In0.c2lnbmF0dXJl";
        let body = format!(
            r#"{{"message":"Token {token} invalid for a@b.example","code":"Unauthorized","raw":"known-secret-123"}}"#
        );
        let out = scrub(&body, &["known-secret-123".into()]);
        assert!(
            !out.contains(token)
                && !out.contains("a@b.example")
                && !out.contains("known-secret-123")
        );
        assert!(out.contains("Unauthorized") && out.contains("invalid for"));
    }

    #[test]
    fn url_summary_shortens_tenant_label() {
        let summary =
            url_summary("https://abcdef123.dp.appintegrations.us-east-1.prod.plato.ai.aws.dev/x");
        assert_eq!(
            summary["host"],
            "abcd…(9 chars).dp.appintegrations.us-east-1.prod.plato.ai.aws.dev"
        );
        assert_eq!(mask_tail("123456789012"), "…9012 (12 chars)");
        assert_eq!(
            url_summary("https://idc.example/ssoins-7223da8169c0d818")["path"],
            "/ssoins-7…"
        );
    }
}
