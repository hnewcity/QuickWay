//! Stateful adapter for Quick's official social cloud conversation protocol.
//!
//! Each conversation owns its WebSocket and virtual outpost. Tools are returned
//! to the API client for execution; this module never executes a local tool.
use crate::{
    auth::QuickSession,
    cloud_request::{ArgumentEncoding, MessageContext, SessionHistory, ValidatedRequest},
    cloud_translate::Translator,
    cloud_wire::{self, MAX_SOCIAL_MESSAGE},
    model_error::ModelError,
    server::{Failure, Outcome},
    upstream::{self, Upstream},
};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, Notify, Semaphore, mpsc, oneshot, watch};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest, protocol::WebSocketConfig},
};

const APP_VERSION: &str = "1.2.4308";
const SURFACE: &str = "QUICK_WORK_DESKTOP_WINDOWS";
const IDLE_TTL: Duration = Duration::from_secs(10 * 60);
const TURN_TIMEOUT: Duration = Duration::from_secs(180);
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type HistoryKey = [u8; 32];

#[derive(Clone)]
struct Handle {
    id: String,
    sender: mpsc::Sender<Command>,
    stop: watch::Sender<bool>,
    evictable: bool,
}

struct Inner {
    idle: Mutex<HashMap<HistoryKey, Handle>>,
    slots: Arc<Semaphore>,
    active: Mutex<HashMap<String, watch::Sender<bool>>>,
    stopping: AtomicBool,
    stopped: Notify,
    shutdown_lock: Mutex<()>,
    last_model: Mutex<Option<(HistoryKey, ModelObservation)>>,
}

/// The latest completed cloud response, scoped to its signed-in account.
/// A missing served ID stays unknown; the requested tier is never a substitute.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelObservation {
    mode: String,
    served_model: Option<String>,
    at: f64,
}

pub fn served_model(metadata: &Value) -> Option<&str> {
    metadata["served_model"].as_str().filter(|id| {
        !id.is_empty()
            && id.len() <= 512
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:/-".contains(&b))
            && !id.contains("://")
    })
}

pub struct Manager(Arc<Inner>);
impl Default for Manager {
    fn default() -> Self {
        Self(Arc::new(Inner {
            idle: Mutex::new(HashMap::new()),
            slots: Arc::new(Semaphore::new(3)),
            active: Mutex::new(HashMap::new()),
            stopping: AtomicBool::new(false),
            stopped: Notify::new(),
            shutdown_lock: Mutex::new(()),
            last_model: Mutex::new(None),
        }))
    }
}

impl Manager {
    pub async fn last_model(&self, identity: [u8; 32]) -> Option<ModelObservation> {
        self.0
            .last_model
            .lock()
            .await
            .as_ref()
            .filter(|(account, _)| *account == identity)
            .map(|(_, observation)| observation.clone())
    }

    /// Stop active requests and unregister this manager's temporary devices.
    /// May be reused after shutdown (the desktop supports Stop then Start).
    pub async fn shutdown(&self) {
        let _guard = self.0.shutdown_lock.lock().await;
        self.0.stopping.store(true, Ordering::SeqCst);
        for stop in self.0.active.lock().await.values() {
            stop.send_replace(true);
        }
        loop {
            let notified = self.0.stopped.notified();
            if self.0.active.lock().await.is_empty() {
                break;
            }
            notified.await;
        }
        self.0.idle.lock().await.clear();
        self.0.stopping.store(false, Ordering::SeqCst);
    }
}

struct Command {
    request: ValidatedRequest,
    session: QuickSession,
    events: Option<mpsc::Sender<Value>>,
    result: oneshot::Sender<Result<Value, ModelError>>,
}

struct Connection {
    socket: Socket,
    client: reqwest::Client,
    session: QuickSession,
    outpost: String,
    thread: String,
    working: String,
    imported: bool,
    registered: bool,
    pending: Vec<String>,
    backlog: VecDeque<Value>,
    current_message: Option<String>,
    text_prefix: String,
}

pub fn mode(model: &str) -> Option<&str> {
    model
        .strip_prefix("quick-")
        .filter(|mode| matches!(*mode, "fast" | "balanced" | "smart" | "advanced"))
}

pub fn models() -> Vec<crate::models::Model> {
    ["fast", "balanced", "smart", "advanced"]
        .into_iter()
        .map(|mode| crate::models::Model {
            id: format!("quick-{mode}"),
            upstream: format!("quick-{mode}"),
            name: format!(
                "Quick {} (cloud)",
                match mode {
                    "fast" => "Fast",
                    "balanced" => "Balanced",
                    "smart" => "Smart",
                    _ => "Advanced",
                }
            ),
            family: "Amazon Quick".into(),
        })
        .collect()
}

/// Prefix matching ignores transport-only message IDs and text-block spelling,
/// but preserves roles, tool inputs/results and the system prompt.
fn history_key(body: &Value, messages: &[Value], identity: HistoryKey) -> HistoryKey {
    let messages: Vec<Value> = messages
        .iter()
        .map(|message| {
            let blocks: Vec<Value> = match &message["content"] {
                Value::String(text) => vec![json!({"type":"text","text":text})],
                Value::Array(blocks) => blocks
                    .iter()
                    .filter(|block| {
                        !matches!(
                            block["type"].as_str(),
                            Some("thinking" | "redacted_thinking")
                        ) && !(block["type"] == "text" && block["text"] == "")
                    })
                    .cloned()
                    .collect(),
                _ => Vec::new(),
            };
            let mut normalized: Vec<Value> = Vec::new();
            for block in blocks {
                if block["type"] == "text"
                    && normalized.last().is_some_and(|last| last["type"] == "text")
                {
                    let last = normalized.last_mut().unwrap();
                    last["text"] = format!(
                        "{}{}",
                        last["text"].as_str().unwrap_or(""),
                        block["text"].as_str().unwrap_or("")
                    )
                    .into();
                } else {
                    let mut block = block;
                    if let Some(fields) = block.as_object_mut() {
                        fields.remove("cache_control");
                    }
                    if block["type"] == "tool_use"
                        && let Some(schema) = body["tools"]
                            .as_array()
                            .and_then(|tools| {
                                tools.iter().find(|tool| tool["name"] == block["name"])
                            })
                            .and_then(|tool| tool.get("input_schema"))
                    {
                        normalize_declared_defaults(&mut block["input"], schema);
                    }
                    if block["type"] == "tool_result" {
                        block["is_error"] = block.get("is_error").cloned().unwrap_or(json!(false));
                        let result = match &block["content"] {
                            Value::String(s) => vec![json!({"type":"text","text":s})],
                            Value::Array(a) => a
                                .iter()
                                .map(|b| json!({"type":"text","text":b["text"]}))
                                .collect(),
                            _ => Vec::new(),
                        };
                        block["content"] = json!(result);
                    }
                    normalized.push(block);
                }
            }
            json!({"role":message["role"],"content":normalized})
        })
        .collect();
    let system = match &body["system"] {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    };
    let value =
        json!({"identity":identity,"model":body["model"],"system":system,"messages":messages});
    Sha256::digest(value.to_string().as_bytes()).into()
}

/// CC materializes optional schema defaults when replaying tool invocations
/// (for example Edit.replace_all=false). Canonicalize only declared defaults;
/// all supplied values, unknown fields, IDs and actual arguments stay strict.
fn normalize_declared_defaults(input: &mut Value, schema: &Value) {
    match input {
        Value::Object(fields) => {
            if let Some(properties) = schema["properties"].as_object() {
                for (name, property) in properties {
                    if let Some(value) = fields.get_mut(name) {
                        normalize_declared_defaults(value, property);
                    } else if let Some(default) = property.get("default") {
                        fields.insert(name.clone(), default.clone());
                    }
                }
            }
        }
        Value::Array(items) => {
            if let Some(schema) = schema.get("items").filter(|s| s.is_object()) {
                for item in items {
                    normalize_declared_defaults(item, schema);
                }
            }
        }
        _ => {}
    }
}

fn failure(error: ModelError) -> Failure {
    Failure {
        status: error.status(),
        message: error.message,
        retry_after: None,
    }
}

pub async fn dispatch(
    upstream: &Upstream,
    body: Value,
    mode: &str,
    effort: Option<&str>,
) -> Result<Outcome, Failure> {
    let request = ValidatedRequest::from_body(body, mode, effort).map_err(|message| Failure {
        status: 400,
        message,
        retry_after: None,
    })?;
    let identity = upstream
        .sessions
        .catalog_identity()
        .await
        .map_err(|e| Failure {
            status: 401,
            message: e.to_string(),
            retry_after: None,
        })?;
    let session = upstream
        .sessions
        .session(&upstream.client)
        .await
        .map_err(|e| Failure {
            status: 401,
            message: e.to_string(),
            retry_after: None,
        })?;
    if session.managed_by_quick {
        return Err(Failure {status:400,message:"Quick cloud modes currently support social/AIDP sign-ins; this account uses a different transport.".into(),retry_after:None});
    }
    upstream::tenant_url(&session, "/").map_err(|e| Failure {
        status: 400,
        message: e.to_string(),
        retry_after: None,
    })?;
    let after = upstream
        .sessions
        .catalog_identity()
        .await
        .map_err(|e| Failure {
            status: 401,
            message: e.to_string(),
            retry_after: None,
        })?;
    if identity != after {
        return Err(Failure {
            status: 409,
            message:
                "Quick account changed during request startup; retry with the current account."
                    .into(),
            retry_after: None,
        });
    }
    let prefix = &request.messages()[..request.messages().len() - 1];
    let key = history_key(&request.body, prefix, identity);
    let manager = upstream.cloud.0.clone();
    if manager.stopping.load(Ordering::SeqCst) {
        return Err(Failure {
            status: 503,
            message: "QuickWay is stopping cloud conversations".into(),
            retry_after: None,
        });
    }
    let existing = manager
        .idle
        .lock()
        .await
        .remove(&key)
        .filter(|h| !h.sender.is_closed());
    if existing.is_none() && !importable_history(&request).is_empty() {
        crate::cloud_history::events(
            importable_history(&request),
            &uuid::Uuid::new_v4().to_string(),
        )
        .map_err(|message| Failure {
            status: 400,
            message,
            retry_after: None,
        })?;
    }
    let stream = request.body["stream"].as_bool().unwrap_or(false);
    let (events_tx, events_rx) = mpsc::channel(64);
    let (result_tx, result_rx) = oneshot::channel();
    let command = Command {
        request,
        session: session.clone(),
        events: stream.then_some(events_tx),
        result: result_tx,
    };
    if let Some(handle) = existing {
        handle.sender.send(command).await.map_err(|_| {
            failure("Quick cloud conversation has disconnected. Start a new conversation.".into())
        })?;
    } else {
        let permit = match manager.slots.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                let mut idle = manager.idle.lock().await;
                let key = idle
                    .iter()
                    .find(|(_, handle)| handle.evictable)
                    .map(|(key, _)| *key);
                let evicted = key.and_then(|key| idle.remove(&key));
                drop(idle);
                let Some(evicted) = evicted else {
                    return Err(Failure {status:429,message:"QuickWay has three busy cloud conversations. Complete pending tool calls or retry after a request finishes.".into(),retry_after:Some("5".into())});
                };
                evicted.stop.send_replace(true);
                tokio::time::timeout(
                    Duration::from_secs(45),
                    manager.slots.clone().acquire_owned(),
                )
                .await
                .map_err(|_| failure("Quick cloud conversation cleanup timed out".into()))?
                .map_err(|_| failure("Quick cloud connection pool is closed".into()))?
            }
        };
        let (sender, receiver) = mpsc::channel(1);
        let (stop, stopped) = watch::channel(false);
        let handle = Handle {
            id: uuid::Uuid::new_v4().to_string(),
            sender,
            stop: stop.clone(),
            evictable: true,
        };
        {
            let mut active = manager.active.lock().await;
            if manager.stopping.load(Ordering::SeqCst) {
                return Err(Failure {
                    status: 503,
                    message: "QuickWay is stopping cloud conversations".into(),
                    retry_after: None,
                });
            }
            active.insert(handle.id.clone(), stop);
        }
        let client = upstream.client.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let id = handle.id.clone();
            run_conversation(
                manager.clone(),
                handle,
                receiver,
                command,
                client,
                identity,
                stopped,
            )
            .await;
            manager.active.lock().await.remove(&id);
            manager.stopped.notify_one();
        });
    }
    if stream {
        Ok(Outcome::Stream(Box::pin(futures_util::stream::unfold(
            events_rx,
            |mut receiver| async move { receiver.recv().await.map(|event| (event, receiver)) },
        ))))
    } else {
        result_rx
            .await
            .map_err(|_| failure("Quick cloud request ended without a response".into()))?
            .map(Outcome::Message)
            .map_err(failure)
    }
}

/// A disconnected session has no pending callbacks we can resume. If the API
/// caller supplied their results, however, the entire history is now complete
/// and can be imported without running those tools again. Validation still
/// rejects missing, duplicated and orphan results before reaching this point.
fn importable_history(request: &ValidatedRequest) -> &[Value] {
    let messages = request.messages();
    if request.turn.tool_results.is_empty() {
        &messages[..messages.len() - 1]
    } else {
        messages
    }
}

async fn emit(command: &Command, events: Vec<Value>) -> Result<(), ModelError> {
    if let Some(sender) = &command.events {
        for event in events {
            tokio::time::timeout(Duration::from_secs(10), sender.send(event))
                .await
                .map_err(|_| ModelError::from("The API client stopped reading the response"))?
                .map_err(|_| ModelError::from("The API client disconnected"))?;
        }
    }
    Ok(())
}

async fn send_event(socket: &mut Socket, event: Value) -> Result<(), ModelError> {
    let inner = base64::engine::general_purpose::STANDARD.encode(event.to_string());
    let outer = json!({"eventType":"rawInputEvent","payload":{"payload":inner}}).to_string();
    tokio::time::timeout(
        Duration::from_secs(10),
        socket.send(Message::Binary(outer.into_bytes().into())),
    )
    .await
    .map_err(|_| ModelError::from("Quick cloud WebSocket send timed out"))?
    .map_err(|_| "Quick cloud WebSocket send failed".into())
}

async fn receive(socket: &mut Socket) -> Result<Vec<Value>, ModelError> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Binary(bytes))) => return cloud_wire::decode_social(&bytes),
            Some(Ok(Message::Ping(bytes))) => socket
                .send(Message::Pong(bytes))
                .await
                .map_err(|_| ModelError::from("Quick cloud pong failed"))?,
            Some(Ok(Message::Pong(_))) => {}
            Some(Ok(Message::Close(_))) | None => {
                return Err("Quick cloud connection closed".into());
            }
            Some(Ok(_)) => {
                return Err("Quick cloud returned an unexpected WebSocket message".into());
            }
            Some(Err(_)) => return Err("Quick cloud WebSocket disconnected".into()),
        }
    }
}

async fn api(
    client: &reqwest::Client,
    session: &QuickSession,
    kind: &str,
    body: Value,
) -> Result<Value, ModelError> {
    let url = upstream::tenant_url(session, "/integration/quick-work/qbs-event")
        .map_err(|_| ModelError::from("Invalid Quick cloud tenant"))?;
    let mut response = upstream::authorized(client, url, &session.access_token)
        .timeout(Duration::from_secs(20))
        .json(&json!({"eventType":kind,"eventBody":{kind:body}}))
        .send()
        .await
        .map_err(|_| ModelError::from("Quick cloud registration request failed"))?;
    if !response.status().is_success() {
        let (_, value, _) = upstream::error_body(response).await;
        return Err(ModelError::from_upstream("UpstreamError", &value));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ModelError::from("Quick cloud registration response disconnected"))?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("Quick cloud registration response is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| ModelError::from("Invalid Quick cloud registration response"))?;
    if value["errorMessage"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
    {
        return Err(ModelError::from_upstream(
            "UpstreamError",
            &json!({"message":value["errorMessage"]}),
        ));
    }
    let inner = value["eventResponse"][format!("{kind}Response")].clone();
    if let Some(text) = inner.as_str() {
        serde_json::from_str(text).map_err(|_| "Invalid Quick cloud operation response".into())
    } else if inner.is_null() {
        Err("Quick cloud operation returned no result".into())
    } else {
        Ok(inner)
    }
}

impl Connection {
    async fn open(
        client: reqwest::Client,
        session: QuickSession,
        request: &ValidatedRequest,
    ) -> Result<Self, ModelError> {
        let identity_url = upstream::tenant_url(&session, "/integration/quick-work/user-info")
            .map_err(|_| ModelError::from("Invalid Quick cloud tenant"))?;
        let identity: Value = upstream::authorized(&client, identity_url, &session.access_token)
            .timeout(Duration::from_secs(20))
            .json(&json!({}))
            .send()
            .await
            .map_err(|error| {
                ModelError::from(if error.is_timeout() {
                    "Quick cloud sign-in check timed out"
                } else if error.is_connect() {
                    "Quick cloud sign-in connection failed"
                } else {
                    "Quick cloud sign-in request failed"
                })
            })?
            .error_for_status()
            .map_err(|_| ModelError::from("Quick cloud sign-in was rejected"))?
            .json()
            .await
            .map_err(|_| ModelError::from("Invalid Quick cloud identity response"))?;
        let actor = identity["userArn"]
            .as_str()
            .ok_or("Quick cloud sign-in has no actor")?;
        let outpost = uuid::Uuid::new_v4().to_string();
        let thread = uuid::Uuid::new_v4().to_string();
        let working = uuid::Uuid::new_v4().to_string();
        let mut url =
            upstream::tenant_url(&session, "/streaming/quickwork-chat-bidirectional-stream")
                .map_err(|_| ModelError::from("Invalid Quick cloud tenant"))?;
        url.set_scheme("wss")
            .map_err(|_| ModelError::from("Invalid Quick cloud WebSocket URL"))?;
        url.query_pairs_mut().extend_pairs([
            ("surfaceType", SURFACE),
            ("token", session.access_token.as_str()),
            ("outpostId", outpost.as_str()),
            ("workingSessionId", working.as_str()),
            ("channel", "prod"),
        ]);
        let mut ws_request = url
            .as_str()
            .into_client_request()
            .map_err(|_| ModelError::from("Invalid Quick cloud WebSocket request"))?;
        ws_request.headers_mut().insert(
            "User-Agent",
            format!("Amazon Quick/{APP_VERSION}").parse().unwrap(),
        );
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_SOCIAL_MESSAGE))
            .max_frame_size(Some(MAX_SOCIAL_MESSAGE));
        let (mut socket, _) = tokio::time::timeout(
            Duration::from_secs(25),
            tokio_tungstenite::connect_async_with_config(ws_request, Some(config), false),
        )
        .await
        .map_err(|_| ModelError::from("Quick cloud connection timed out"))?
        .map_err(|_| ModelError::from("Quick cloud WebSocket handshake failed"))?;
        send_event(&mut socket, json!({"type":"session_connect","payload":{
            "actor_id":actor,"surfaceType":SURFACE,"outpostId":outpost,"appVersion":APP_VERSION,"acp_first_text":1,"user_notification":1
        }})).await?;
        send_event(&mut socket, json!({"type":"federate_status","payload":{"user":{
            "name":identity["displayName"],"email":identity["email"],"role":identity["role"],"user_arn":actor
        }}})).await?;
        tokio::time::timeout(Duration::from_secs(25), async {
            loop {
                for event in receive(&mut socket).await? {
                    if event["type"] == "connected" {
                        return Ok::<_, ModelError>(());
                    }
                    if event["type"] == "error" {
                        return Err(ModelError::from_upstream(
                            "UpstreamError",
                            &event["payload"],
                        ));
                    }
                }
            }
        })
        .await
        .map_err(|_| ModelError::from("Quick cloud session startup timed out"))??;
        let mut connection = Self {
            socket,
            client,
            session,
            outpost,
            thread,
            working,
            imported: false,
            registered: false,
            pending: Vec::new(),
            backlog: VecDeque::new(),
            current_message: None,
            text_prefix: String::new(),
        };
        let setup=async {
        connection.register(request).await?;
        connection.import_history(request).await?;
        send_event(&mut connection.socket, json!({"type":"session_connect","payload":{
            "session_id":connection.thread,"focus_only":true,"incognito":true,
            "surfaceType":SURFACE,"outpostId":connection.outpost,"appVersion":APP_VERSION,"acp_first_text":1,"user_notification":1
        }})).await
        }.await;
        if let Err(error) = setup {
            connection.cleanup().await;
            return Err(error);
        }
        Ok(connection)
    }

    async fn store(&self, method: &str, path: &str, body: Value) -> Result<Value, ModelError> {
        let result = api(
            &self.client,
            &self.session,
            "invokeAgent",
            json!({
                "path":path,"queryParams":{"_method":method},"body":body,
                "outpostId":self.outpost,"workingSessionId":self.working
            }),
        )
        .await?;
        if !result["status"]
            .as_u64()
            .is_some_and(|s| (200..300).contains(&s))
        {
            return Err("Quick cloud could not import this conversation's history".into());
        }
        match &result["body"] {
            Value::String(text) => serde_json::from_str(text)
                .map_err(|_| "Invalid Quick cloud history response".into()),
            value => Ok(value.clone()),
        }
    }

    async fn import_history(&mut self, request: &ValidatedRequest) -> Result<(), ModelError> {
        let prefix = importable_history(request);
        if prefix.is_empty() {
            return Ok(());
        }
        let mut events =
            crate::cloud_history::events(prefix, &self.thread).map_err(ModelError::from)?;
        self.store("POST","store/sessions",json!({"session_id":self.thread,"title":"QuickWay API conversation","session_type":"main","incognito":true})).await?;
        let now = crate::auth::now();
        for (index, event) in events.iter_mut().enumerate() {
            event["timestamp"] = (now + index as f64 * 0.000001).into();
        }
        for batch in events.chunks(100) {
            self.store(
                "POST",
                &format!("store/sessions/{}/events/batch", self.thread),
                json!({"events":batch,"update_metadata":true}),
            )
            .await?;
        }
        self.imported = true;
        Ok(())
    }

    async fn register(&mut self, request: &ValidatedRequest) -> Result<(), ModelError> {
        if request.tools.is_empty() && !self.registered {
            return Ok(());
        }
        // Mark before sending: a lost response may still have created the entry.
        self.registered = true;
        let capabilities = if request.tools.is_empty() {
            vec![]
        } else {
            vec![request.tools.capability()]
        };
        api(&self.client, &self.session, "putOutpost", json!({
            "outpostId":self.outpost,"name":crate::cloud_request::CLOUD_OUTPOST_NAME,"outpostType":"DESKTOP","capabilities":capabilities,"status":"ONLINE"
        })).await?;
        Ok(())
    }

    async fn turn(
        &mut self,
        command: &Command,
        new: bool,
    ) -> Result<(Value, Vec<Value>), ModelError> {
        let request = &command.request;
        self.session = command.session.clone();
        if !new {
            self.register(request).await?;
        }
        let resuming = !self.pending.is_empty();
        if self.pending.is_empty() {
            self.text_prefix.clear();
            let message_id = uuid::Uuid::new_v4().to_string();
            let mut recovered;
            let message_request = if new && self.imported && !request.turn.tool_results.is_empty() {
                recovered = request.clone();
                recovered.turn.tool_results.clear();
                // Actual user text and results have already been imported,
                // with their roles and error flags intact. Only ask the new
                // generation to continue, never manufacture a missing result.
                recovered.turn.text = "Continue responding to the latest user request using the completed tool results in the conversation. Those tools have already run; do not repeat completed actions merely because this connection was restored.".into();
                &recovered
            } else {
                request
            };
            let mut payload = message_request
                .user_message_payload(
                    &MessageContext {
                        surface_type: SURFACE,
                        outpost_id: &self.outpost,
                        app_version: APP_VERSION,
                        thread_id: &self.thread,
                        message_id: &message_id,
                        user_timezone: None,
                        user_locale: None,
                    },
                    if new && !self.imported {
                        SessionHistory::NewConversation
                    } else {
                        SessionHistory::MatchedExistingSession
                    },
                )
                .map_err(ModelError::from)?;
            // The only local capabilities of this virtual device are supplied
            // by the API caller. Actual execution remains in that caller.
            payload["custom_prompt"] = format!(
                "{}{}",
                request.custom_prompt,
                request.tools.routing_instructions(&self.outpost)
            )
            .into();
            send_event(
                &mut self.socket,
                json!({"type":"user_message","payload":payload}),
            )
            .await?;
        } else {
            for event in request
                .turn
                .result_events(&self.pending)
                .map_err(ModelError::from)?
            {
                send_event(&mut self.socket, event).await?;
            }
            self.pending.clear();
        }
        let model = request.body["model"]
            .as_str()
            .unwrap_or("quick-smart")
            .to_owned();
        let mut translator = if resuming {
            Translator::continue_after_tools(model, self.text_prefix.clone())?
        } else {
            Translator::new(model)
        };
        let mut terminal = Vec::new();
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.tick().await;
        loop {
            if command.result.is_closed() && command.events.is_none() {
                return Err("The API client disconnected".into());
            }
            let events = if self.backlog.is_empty() {
                tokio::select! {
                    value = receive(&mut self.socket) => value?,
                    _ = interval.tick() => {
                        send_event(&mut self.socket,json!({"type":"ping","payload":{"timestamp":crate::auth::now()*1000.0}})).await?;
                        continue;
                    }
                    _ = async { if let Some(tx)=&command.events {tx.closed().await} else {std::future::pending::<()>().await} } => {
                        return Err("The API client disconnected".into());
                    }
                }
            } else {
                self.backlog.drain(..).collect()
            };
            let mut called = false;
            for mut event in events {
                if event
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id != self.thread)
                {
                    continue;
                }
                if event["type"] == "message_start" {
                    self.current_message = event["message_id"].as_str().map(str::to_owned);
                }
                if event["type"] == "permission_request" {
                    self.approve_virtual_callback(&event, request).await?;
                    continue;
                }
                if event["type"] == "tool_call" {
                    let p = &mut event["payload"];
                    let call_id = p["tool_call_id"]
                        .as_str()
                        .filter(|id| !id.is_empty() && id.len() <= 256)
                        .ok_or("Quick cloud tool call has no valid ID")?
                        .to_owned();
                    if self.pending.contains(&call_id) {
                        return Err("Quick cloud repeated a pending tool call".into());
                    }
                    let tool = request
                        .tools
                        .decode_arguments(
                            p["tool_name"]
                                .as_str()
                                .ok_or("Quick cloud tool call has no name")?,
                            &p["arguments"],
                            ArgumentEncoding::ParamsWrapper,
                        )
                        .map_err(ModelError::from)?;
                    p["tool_name"] = tool.name.into();
                    p["arguments"] = tool.input;
                    self.pending.push(call_id);
                    called = true;
                }
                let translated = translator.ingest(&event)?;
                let (end, progress): (Vec<_>, Vec<_>) = translated.into_iter().partition(|e| {
                    matches!(e["type"].as_str(), Some("message_delta" | "message_stop"))
                });
                terminal.extend(end);
                emit(command, progress).await?;
                if translator.is_finished() {
                    let mut message = translator.message()?;
                    message["quickway"]["notices"] = json!(request.notices);
                    return Ok((message, terminal));
                }
            }
            if called {
                terminal.extend(translator.finish_tools()?);
                self.text_prefix = translator.text_seen().to_owned();
                let mut message = translator.message()?;
                message["quickway"]["notices"] = json!(request.notices);
                return Ok((message, terminal));
            }
        }
    }

    async fn approve_virtual_callback(
        &mut self,
        event: &Value,
        request: &ValidatedRequest,
    ) -> Result<(), ModelError> {
        let p = &event["payload"];
        let connector = p
            .get("connectorId")
            .or(p.get("connector_id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let action = p
            .get("actionId")
            .or(p.get("action_id"))
            .and_then(Value::as_str)
            .unwrap_or("");
        // This grants only dispatch to our synthetic callback. The API client
        // still performs its own approval before executing the returned tool.
        let ours =
            connector == "mcp__quickway" && request.tools.original_names.contains_key(action);
        let mut response = json!({"type":if ours {"tool_approval"}else{"permission_denied"},"connectorId":connector,"actionId":action,"scope":"ALLOW_ONCE","sessionId":self.thread});
        for field in ["params", "paramsHash", "agentId", "conversationId"] {
            if let Some(value) = p.get(field) {
                response[field] = value.clone();
            }
        }
        send_event(&mut self.socket, response).await?;
        if !ours {
            return Err(
                "Quick cloud requested a permission outside this API session's tools".into(),
            );
        }
        Ok(())
    }

    async fn cleanup(&mut self) {
        if let Some(message_id) = &self.current_message {
            let _ = send_event(&mut self.socket,json!({"type":"stop_generation","payload":{"message_id":message_id,"thread_id":self.thread}})).await;
        }
        let _ = tokio::time::timeout(Duration::from_secs(3), self.socket.close(None)).await;
        if self.registered
            && api(
                &self.client,
                &self.session,
                "unregisterOutpost",
                json!({"outpostId":self.outpost}),
            )
            .await
            .is_err()
        {
            // Retry idempotent cleanup once. Only our fresh UUID is touched.
            if api(
                &self.client,
                &self.session,
                "unregisterOutpost",
                json!({"outpostId":self.outpost}),
            )
            .await
            .is_err()
            {
                tracing::warn!(
                    "Could not remove a temporary QuickWay cloud device after disconnect"
                );
            }
        }
    }
}

async fn run_conversation(
    manager: Arc<Inner>,
    handle: Handle,
    mut receiver: mpsc::Receiver<Command>,
    mut command: Command,
    client: reqwest::Client,
    identity: HistoryKey,
    mut stopped: watch::Receiver<bool>,
) {
    let opened = Connection::open(client, command.session.clone(), &command.request).await;
    let mut connection = match opened {
        Ok(connection) => connection,
        Err(error) => {
            let _ = emit(&command, vec![error.event()]).await;
            let _ = command.result.send(Err(error));
            return;
        }
    };
    let mut new = true;
    loop {
        let result = if *stopped.borrow() {
            Err("QuickWay is stopping the cloud conversation".into())
        } else {
            tokio::select! {
                value=tokio::time::timeout(TURN_TIMEOUT,connection.turn(&command,new))=>value.unwrap_or_else(|_|Err("Quick cloud reply timed out".into())),
                _=stopped.changed()=>Err("QuickWay is stopping the cloud conversation".into()),
            }
        };
        match result {
            Ok((message, terminal)) => {
                *manager.last_model.lock().await = Some((
                    identity,
                    ModelObservation {
                        mode: command.request.mode.clone(),
                        served_model: served_model(&message["quickway"]).map(str::to_owned),
                        at: crate::auth::now(),
                    },
                ));
                let mut history = command.request.messages().to_vec();
                history.push(json!({"role":"assistant","content":message["content"]}));
                let key = history_key(&command.request.body, &history, identity);
                let mut retained = handle.clone();
                retained.evictable = connection.pending.is_empty();
                if let Some(previous) = manager.idle.lock().await.insert(key, retained)
                    && previous.id != handle.id
                {
                    previous.stop.send_replace(true);
                }
                if emit(&command, terminal).await.is_err() {
                    manager.idle.lock().await.remove(&key);
                    let _ = command
                        .result
                        .send(Err("The API client disconnected".into()));
                    break;
                }
                let _ = command.result.send(Ok(message));
                drop(command.events);
                if connection.pending.is_empty() {
                    connection.current_message = None;
                }
                let mut ticker = tokio::time::interval(Duration::from_secs(15));
                ticker.tick().await;
                let idle = tokio::time::sleep(IDLE_TTL);
                tokio::pin!(idle);
                let next = 'waiting: loop {
                    tokio::select! {
                        _=stopped.changed()=>break None,
                        next=receiver.recv()=>break next,
                        _=&mut idle=>break None,
                        _=ticker.tick()=>{
                            if send_event(&mut connection.socket,json!({"type":"ping","payload":{"timestamp":crate::auth::now()*1000.0}})).await.is_err(){break None;}
                        }
                        events=receive(&mut connection.socket)=>match events {
                            Ok(events)=>{
                                for event in events {
                                    if event.get("thread_id").and_then(Value::as_str).is_some_and(|id|id!=connection.thread){continue;}
                                    if !matches!(event["type"].as_str(),Some("tool_call"|"permission_request"|"message_start"|"text_delta"|"text_complete"|"turn_usage"|"message_complete"|"error")){continue;}
                                    if connection.backlog.len()>=128 {break 'waiting None;}
                                    connection.backlog.push_back(event);
                                }
                            }
                            Err(_)=>break None,
                        }
                    }
                };
                {
                    let mut idle = manager.idle.lock().await;
                    if idle.get(&key).is_some_and(|h| h.id == handle.id) {
                        idle.remove(&key);
                    }
                }
                let Some(next) = next else {
                    break;
                };
                command = next;
                new = false;
            }
            Err(error) => {
                let _ = emit(&command, vec![error.event()]).await;
                let _ = command.result.send(Err(error));
                break;
            }
        }
    }
    connection.cleanup().await;
}

#[cfg(test)]
mod tests {
    #[test]
    fn disconnected_tools_recover_only_with_complete_results_and_keep_user_text() {
        let body = json!({"model":"quick-smart","messages":[
            {"role":"user","content":"Run the checks"},
            {"role":"assistant","content":[{"type":"tool_use","id":"call1","name":"exec","input":{"input":"run checks"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call1","is_error":true,"content":"check failed"},{"type":"text","text":"Fix that failure"}]}
        ]});
        let request = ValidatedRequest::from_body(body.clone(), "smart", None).unwrap();
        assert_eq!(importable_history(&request), request.messages());
        let events =
            crate::cloud_history::events(importable_history(&request), "recovered").unwrap();
        assert!(
            events
                .iter()
                .any(|e| e["type"] == "tool_result_projection" && e["data"]["status"] == "error")
        );
        assert!(
            events
                .iter()
                .any(|e| e["type"] == "message_user" && e["data"]["content"] == "Fix that failure")
        );
        let mut incomplete = body.clone();
        incomplete["messages"][2]["content"]
            .as_array_mut()
            .unwrap()
            .remove(0);
        assert!(ValidatedRequest::from_body(incomplete, "smart", None).is_err());
        let mut orphan = body;
        orphan["messages"][2]["content"][0]["tool_use_id"] = "other".into();
        assert!(ValidatedRequest::from_body(orphan, "smart", None).is_err());
    }
    #[test]
    fn tool_replay_accepts_declared_defaults_but_not_changed_arguments() {
        let body = json!({"model":"quick-smart","tools":[{"name":"Edit","input_schema":{
            "type":"object","properties":{"replace_all":{"type":"boolean","default":false}}
        }}]});
        let sent = vec![
            json!({"role":"assistant","content":[{"type":"tool_use","id":"edit1","name":"Edit","input":{
                "file_path":"notes.py","old_string":"FIRST","new_string":"CHANGED"
            }}]}),
        ];
        let mut replayed = sent.clone();
        replayed[0]["content"][0]["input"]["replace_all"] = json!(false);
        assert_eq!(
            history_key(&body, &sent, [1; 32]),
            history_key(&body, &replayed, [1; 32])
        );
        for changed in [
            json!({"replace_all":true}),
            json!({"old_string":"OTHER"}),
            json!({"file_path":"other.py"}),
            json!({"unknown":false}),
        ] {
            let mut modified = replayed.clone();
            modified[0]["content"][0]["input"]
                .as_object_mut()
                .unwrap()
                .extend(changed.as_object().unwrap().clone());
            assert_ne!(
                history_key(&body, &sent, [1; 32]),
                history_key(&body, &modified, [1; 32])
            );
        }
        let mut undeclared = body.clone();
        undeclared["tools"][0]["input_schema"]["properties"]["replace_all"]
            .as_object_mut()
            .unwrap()
            .remove("default");
        assert_ne!(
            history_key(&undeclared, &sent, [1; 32]),
            history_key(&undeclared, &replayed, [1; 32])
        );
        assert_ne!(
            history_key(&body, &sent, [1; 32]),
            history_key(&body, &replayed, [2; 32])
        );
    }
    #[test]
    fn concurrent_user_update_preserves_tools_and_history_identity() {
        let body = json!({"model":"quick-smart","messages":[
            {"role":"user","content":"Run the checks"},
            {"role":"assistant","content":[
                {"type":"tool_use","id":"first","name":"Read","input":{}},
                {"type":"tool_use","id":"second","name":"PowerShell","input":{}}
            ]},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"second","is_error":true,"content":"Failed"},
                {"type":"tool_result","tool_use_id":"first","content":"fixture"},
                {"type":"text","text":"The user sent a new message while you were working:\n跑不了就用wsl"}
            ]}
        ]});
        let request = ValidatedRequest::from_body(body.clone(), "smart", None).unwrap();
        let events = request
            .turn
            .result_events(&["first".into(), "second".into()])
            .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]["payload"]["result"],
            json!({"content":[{"type":"text","text":"Failed"}],"isError":true})
        );
        assert_eq!(
            events[1]["payload"]["result"]["content"][0]["text"],
            "fixture"
        );
        assert!(
            events[1]["payload"]["result"]["content"][1]["text"]
                .as_str()
                .unwrap()
                .contains(&request.turn.text)
        );
        assert_eq!(request.body, body);
        assert!(request.turn.result_events(&["first".into()]).is_err());
        assert!(
            request
                .turn
                .result_events(&["first".into(), "unrelated".into()])
                .is_err()
        );
    }
    #[tokio::test]
    async fn last_model_is_account_scoped_and_missing_usage_stays_unknown() {
        let manager = super::Manager::default();
        assert!(manager.last_model([1; 32]).await.is_none());
        *manager.0.last_model.lock().await = Some((
            [1; 32],
            super::ModelObservation {
                mode: "smart".into(),
                served_model: None,
                at: 1.0,
            },
        ));
        assert!(manager.last_model([2; 32]).await.is_none());
        let observation = manager.last_model([1; 32]).await.unwrap();
        assert!(observation.served_model.is_none());
        assert_eq!(observation.mode, "smart");
        assert!(
            super::served_model(&serde_json::json!({
                "mode":"smart", "resolved_model":"us.anthropic.claude-opus-5-5"
            }))
            .is_none()
        );
    }

    use super::*;
    #[test]
    fn claude_system_block_reformatting_preserves_the_pending_tool_session() {
        let call = json!({"role":"assistant","content":[{"type":"tool_use","id":"read1","name":"Read","input":{"file_path":"fixture.txt"}}]});
        let mut initial = json!({"model":"quick-smart","system":"Stable system prompt","messages":[
            {"role":"user","content":"Read fixture.txt"},
            {"role":"system","content":[{"type":"text","text":"Environment","cache_control":{"type":"ephemeral"}}]}
        ]});
        let mut next = json!({"model":"quick-smart","system":"Stable system prompt","messages":[
            {"role":"user","content":"Read fixture.txt"},
            {"role":"system","content":"Environment"}, call,
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"read1","content":"OK"}]},
            {"role":"system","content":"<total_tokens>14925105 tokens left</total_tokens>"}
        ]});
        crate::compat::normalize(&mut initial);
        crate::compat::normalize(&mut next);
        let mut retained = initial["messages"].as_array().unwrap().clone();
        retained.push(call);
        let replayed = next["messages"].as_array().unwrap();
        assert_eq!(
            history_key(&initial, &retained, [1; 32]),
            history_key(&next, &replayed[..replayed.len() - 1], [1; 32])
        );
        let request = ValidatedRequest::from_body(next, "smart", None).unwrap();
        assert!(request.turn.text.is_empty());
    }

    #[test]
    fn history_matching_normalizes_text_but_keeps_tools_identity_and_system() {
        let a = json!({"model":"quick-smart","system":"one"});
        let plain = vec![json!({"role":"user","content":"hello"})];
        let blocks = vec![
            json!({"role":"user","content":[{"type":"text","text":"hello"}],"id":"transport-id"}),
        ];
        assert_eq!(
            history_key(&a, &plain, [1; 32]),
            history_key(&a, &blocks, [1; 32])
        );
        assert_ne!(
            history_key(&a, &plain, [1; 32]),
            history_key(&a, &plain, [2; 32])
        );
        assert_ne!(
            history_key(&a, &plain, [1; 32]),
            history_key(
                &json!({"model":"quick-smart","system":"two"}),
                &plain,
                [1; 32]
            )
        );
    }
}
