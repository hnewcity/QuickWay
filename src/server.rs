use crate::{
    converse, models, openai, streaming,
    upstream::{self, Upstream},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};
use std::{convert::Infallible, pin::Pin, sync::Arc, time::Duration};

pub struct App {
    pub upstream: Upstream,
    pub key: String,
}
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async {
                Json(
                    json!({"status":"ok","service":"quickway","version":env!("CARGO_PKG_VERSION")}),
                )
            }),
        )
        .route("/v1/models", get(list_models))
        .route("/v1/messages", post(messages))
        .route("/v1/messages/count_tokens", post(count_tokens))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/responses", post(responses))
        .route("/quickway/usage", get(usage))
        .layer(DefaultBodyLimit::max(32 * 1024 * 1024))
        .with_state(app)
}
fn authorized(headers: &HeaderMap, key: &str) -> bool {
    let provided = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        })
        .unwrap_or("");
    let mut diff = provided.len() ^ key.len();
    for (a, b) in provided.bytes().zip(key.bytes()) {
        diff |= (a ^ b) as usize;
    }
    diff == 0
}
fn error(status: u16, message: impl Into<String>) -> Response {
    let kind = upstream::error_kind(status);
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
        Json(json!({"type":"error","error":{"type":kind,"message":message.into()}})),
    )
        .into_response()
}
async fn list_models(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.key) {
        return error(401, "Invalid QuickWay API key");
    }
    Json(app.upstream.models(false).await.list()).into_response()
}
async fn usage(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &app.key) {
        return error(401, "Invalid QuickWay API key");
    }
    match app.upstream.usage().await {
        Ok(value) => Json(value).into_response(),
        Err(e) => error(502, e.to_string()),
    }
}
async fn count_tokens(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&headers, &app.key) {
        return error(401, "Invalid QuickWay API key");
    }
    // Quick's inspected proxy has no CountTokens operation. A conservative
    // estimate includes system, tools and every history block, including results.
    let relevant = json!({"system":body.get("system"),"messages":body.get("messages"),"tools":body.get("tools")});
    let tokens = relevant.to_string().len().div_ceil(3).max(1);
    (
        [("x-quickway-token-count", "estimated")],
        Json(json!({"input_tokens":tokens})),
    )
        .into_response()
}
fn openai_error(status: u16, message: impl Into<String>) -> Response {
    let (kind, code) = match status {
        400 | 404 | 413 | 422 => ("invalid_request_error", Value::Null),
        401 => ("invalid_request_error", json!("invalid_api_key")),
        403 => ("permission_error", Value::Null),
        429 => ("rate_limit_error", json!("rate_limit_exceeded")),
        _ => ("server_error", Value::Null),
    };
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
        Json(json!({"error":{"message":message.into(),"type":kind,"param":null,"code":code}})),
    )
        .into_response()
}
fn keep_alive() -> KeepAlive {
    KeepAlive::new().interval(Duration::from_secs(15))
}
/// A failed dispatch, rendered by each protocol in its own error shape.
pub struct Failure {
    pub status: u16,
    pub message: String,
    pub retry_after: Option<String>,
}
impl Failure {
    fn new(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            retry_after: None,
        }
    }
    fn anthropic(self) -> Response {
        self.render(error)
    }
    fn openai(self) -> Response {
        self.render(openai_error)
    }
    fn render(self, render: fn(u16, String) -> Response) -> Response {
        let mut response = render(self.status, self.message);
        if let Some(retry) = self.retry_after.and_then(|v| v.parse().ok()) {
            response.headers_mut().insert("retry-after", retry);
        }
        response
    }
}
pub enum Outcome {
    /// A complete Anthropic Messages response.
    Message(Value),
    /// Anthropic stream events; a failure ends with an `error` event.
    Stream(Pin<Box<dyn Stream<Item = Value> + Send>>),
}
async fn messages(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&headers, &app.key) {
        return error(401, "Invalid QuickWay API key");
    }
    match dispatch(&app, &headers, body, None).await {
        Ok(Outcome::Message(value)) => Json(value).into_response(),
        Ok(Outcome::Stream(events)) => Sse::new(events.map(|event| {
            let kind = event["type"].as_str().unwrap_or("error").to_string();
            Ok::<_, Infallible>(Event::default().event(kind).data(event.to_string()))
        }))
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("ping"),
        )
        .into_response(),
        Err(failure) => failure.anthropic(),
    }
}
async fn chat_completions(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&headers, &app.key) {
        return openai_error(401, "Invalid QuickWay API key");
    }
    let request = match openai::chat_request(&body) {
        Ok(r) => r,
        Err(e) => return openai_error(400, e),
    };
    match dispatch(&app, &headers, request.body, request.effort).await {
        Ok(Outcome::Message(message)) => Json(openai::chat_response(&message)).into_response(),
        Ok(Outcome::Stream(events)) => {
            let mut translator = openai::ChatStream::new(request.include_usage);
            let chunks = events.flat_map(move |event| {
                futures_util::stream::iter(
                    translator
                        .event(&event)
                        .into_iter()
                        .map(|data| Ok::<_, Infallible>(Event::default().data(data))),
                )
            });
            Sse::new(chunks).keep_alive(keep_alive()).into_response()
        }
        Err(failure) => failure.openai(),
    }
}
async fn responses(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&headers, &app.key) {
        return openai_error(401, "Invalid QuickWay API key");
    }
    let request = match openai::responses_request(&body) {
        Ok(r) => r,
        Err(e) => return openai_error(400, e),
    };
    let converted = request.converted;
    match dispatch(&app, &headers, converted.body, converted.effort).await {
        Ok(Outcome::Message(message)) => Json(openai::responses_response(
            &message,
            &request.echo,
            &request.tools,
        ))
        .into_response(),
        Ok(Outcome::Stream(events)) => {
            let mut translator = openai::ResponsesStream::new(request.echo, request.tools);
            let events = events.flat_map(move |event| {
                futures_util::stream::iter(translator.event(&event).into_iter().map(
                    |(kind, data)| {
                        Ok::<_, Infallible>(Event::default().event(kind).data(data.to_string()))
                    },
                ))
            });
            Sse::new(events).keep_alive(keep_alive()).into_response()
        }
        Err(failure) => failure.openai(),
    }
}
/// Sends an Anthropic Messages body to Quick. Shared by every client protocol
/// so model routing and thinking controls stay in one place.
/// `effort` is a client's own reasoning setting; the desktop header wins.
pub async fn dispatch(
    app: &App,
    headers: &HeaderMap,
    mut body: Value,
    effort: Option<crate::thinking::Effort>,
) -> Result<Outcome, Failure> {
    // Current Claude Code releases send first-party-only fields and system turns.
    crate::compat::normalize(&mut body);
    let Some(name) = body["model"].as_str() else {
        return Err(Failure::new(400, "model is required"));
    };
    if let Some(mode) = crate::cloud::mode(name).map(str::to_owned) {
        let selected = headers
            .get("x-quickway-thinking-effort")
            .map(|value| {
                value
                    .to_str()
                    .map(str::to_owned)
                    .map_err(|_| Failure::new(400, "Invalid thinking effort header"))
            })
            .transpose()?
            .or_else(|| {
                effort.map(|value| {
                    match value {
                        crate::thinking::Effort::Off | crate::thinking::Effort::Low => "low",
                        crate::thinking::Effort::Medium => "medium",
                        crate::thinking::Effort::High => "high",
                    }
                    .to_owned()
                })
            });
        let selected = selected
            .as_deref()
            .map(|v| if v == "off" { "low" } else { v });
        return crate::cloud::dispatch(&app.upstream, body, &mode, selected).await;
    }
    let (model, model_id) = app
        .upstream
        .models(false)
        .await
        .resolve(name)
        .map_err(|e| Failure::new(400, e))?;
    // A per-launch desktop setting; ordinary API clients keep their own options.
    let effort = match headers.get("x-quickway-thinking-effort") {
        Some(header) => Some(
            header
                .to_str()
                .map_err(|_| "Invalid thinking effort header".to_owned())
                .and_then(crate::thinking::Effort::parse)
                .map_err(|message| Failure::new(400, message))?,
        ),
        None => effort,
    };
    if let Some(effort) = effort {
        effort
            .apply(&mut body, &model)
            .map_err(|message| Failure::new(400, message))?;
    }
    if body["max_tokens"].as_u64().is_none_or(|v| v == 0)
        || body["messages"].as_array().is_none_or(|v| v.is_empty())
    {
        return Err(Failure::new(
            400,
            "max_tokens must be positive and messages must be nonempty",
        ));
    }
    let stream = match body.get("stream") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => return Err(Failure::new(400, "stream must be a boolean")),
    };
    // Quick's Opus 5.5 InvokeModel route returns an incomplete response when
    // thinking controls are supplied. Use the same Converse options as streams.
    let converse_response =
        !stream && (!models::is_anthropic(&model_id) || model_id.contains("claude-opus-5-5"));
    let (path, payload) = if stream || converse_response {
        let mut request = converse::request(body, &model_id).map_err(|e| Failure::new(400, e))?;
        request
            .as_object_mut()
            .expect("converse request is object")
            .remove("modelId");
        if stream {
            (
                "/integration/quick-work/bedrock-proxy-stream",
                json!({"modelId":model_id,"requestBody":request}),
            )
        } else {
            (
                "/integration/quick-work/bedrock-proxy",
                json!({"modelId":model_id,"apiType":"CONVERSE","requestBody":request}),
            )
        }
    } else {
        let object = body.as_object_mut().expect("model field implies object");
        object.remove("model");
        object.remove("stream");
        object.remove("metadata");
        object.insert("anthropic_version".into(), json!("bedrock-2023-05-31"));
        (
            "/integration/quick-work/bedrock-proxy",
            json!({"modelId":model_id,"apiType":"INVOKE_MODEL","requestBody":body,"contentType":"application/json","accept":"application/json"}),
        )
    };
    let response = app
        .upstream
        .post(path, &payload)
        .await
        .map_err(|e| Failure::new(502, e.to_string()))?;
    if !response.status().is_success() {
        let (status, body, retry) = upstream::error_body(response).await;
        return Err(Failure {
            status,
            message: body["error"]["message"]
                .as_str()
                .unwrap_or("Quick upstream request failed")
                .to_owned(),
            retry_after: retry,
        });
    }
    if stream {
        return Ok(Outcome::Stream(Box::pin(streaming::event_stream(
            response, model,
        ))));
    }
    let data: Value = response
        .json()
        .await
        .map_err(|_| Failure::new(502, "Invalid Quick model response"))?;
    let response = &data["responseBody"];
    if converse_response {
        return converse::response(response.clone(), &model)
            .map(Outcome::Message)
            .map_err(|e| Failure::new(502, e));
    }
    if response["type"] != "message"
        || !response["content"].is_array()
        || response["stop_reason"].is_null()
    {
        return Err(Failure::new(
            502,
            "Quick returned an incomplete Anthropic response",
        ));
    }
    Ok(Outcome::Message(response.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn overload_response_retains_retry_guidance() {
        let response = Failure {
            status: 503,
            message: "Quick is temporarily overloaded".into(),
            retry_after: Some("30".into()),
        }
        .anthropic();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["retry-after"], "30");
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "overloaded_error");
        assert_eq!(body["error"]["message"], "Quick is temporarily overloaded");
    }

    #[test]
    fn local_keys_are_required_and_match_exactly() {
        let mut h = HeaderMap::new();
        assert!(!authorized(&h, "fixture-key"));
        h.insert("authorization", "Bearer fixture-key".parse().unwrap());
        assert!(authorized(&h, "fixture-key"));
        h.insert("x-api-key", "wrong".parse().unwrap());
        assert!(!authorized(&h, "fixture-key"));
    }
}
