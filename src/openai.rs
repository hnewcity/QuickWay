//! OpenAI Chat Completions and Responses adapters.
//!
//! Both protocols are converted to an Anthropic Messages body and sent
//! through the same dispatch as `/v1/messages`; results are converted back.
//! QuickWay stores nothing, so Responses features that need server-side state
//! (`previous_response_id`, item references) are rejected.
use crate::thinking::Effort;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// Used when a client omits an output limit. Within every catalog model's cap.
pub const DEFAULT_MAX_TOKENS: u64 = 32000;
// Reasoning items carry the Anthropic signature (or redacted data) here so a
// stateless client can replay signed thinking on the next turn.
const SIGNED: &str = "quickway:sig:";
const REDACTED: &str = "quickway:redacted:";

pub struct Converted {
    /// Anthropic Messages request body.
    pub body: Value,
    pub effort: Option<Effort>,
    pub stream: bool,
    pub include_usage: bool,
}

type Object = Map<String, Value>;

fn object<'a>(value: &'a Value, field: &str) -> Result<&'a Object, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{field} must be an object"))
}
fn array<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{field} must be an array"))
}
fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{field} must be a string"))
}
fn present<'a>(object: &'a Object, field: &str) -> Option<&'a Value> {
    object.get(field).filter(|v| !v.is_null())
}
fn boolean(object: &Object, field: &str) -> Result<Option<bool>, String> {
    present(object, field)
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| format!("{field} must be a boolean"))
        })
        .transpose()
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
fn id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// Rejects options that would change the result but have no upstream equivalent.
fn reject_unsupported(body: &Object, fields: &[&str]) -> Result<(), String> {
    for field in fields {
        let Some(value) = present(body, field) else {
            continue;
        };
        let neutral = match value {
            Value::Bool(b) => !b,
            Value::Number(n) => n.as_f64() == Some(0.0) || *field == "n" && n.as_u64() == Some(1),
            Value::Object(o) => o.is_empty(),
            Value::Array(a) => a.is_empty(),
            _ => false,
        };
        if !neutral {
            return Err(format!("{field} is not supported by QuickWay"));
        }
    }
    Ok(())
}

fn effort(value: &str) -> Result<Effort, String> {
    match value {
        "none" | "minimal" => Ok(Effort::Off),
        "low" => Ok(Effort::Low),
        "medium" => Ok(Effort::Medium),
        // Codex model catalogs also offer max/ultra; QuickWay's top tier is High.
        "high" | "xhigh" | "max" | "ultra" => Ok(Effort::High),
        _ => Err(
            "reasoning effort must be none, minimal, low, medium, high, xhigh, max or ultra".into(),
        ),
    }
}

fn max_tokens(body: &Object, fields: &[&str]) -> Result<u64, String> {
    for field in fields {
        if let Some(value) = present(body, field) {
            return value
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or_else(|| format!("{field} must be a positive integer"));
        }
    }
    Ok(DEFAULT_MAX_TOKENS)
}

/// Accepts base64 data URLs only; QuickWay does not fetch remote images.
fn image(url: &str) -> Result<Value, String> {
    let rest = url.strip_prefix("data:").ok_or(
        "Only base64 data: image URLs are supported; QuickWay does not download remote images",
    )?;
    let (media_type, data) = rest
        .split_once(";base64,")
        .ok_or("Image data URL must be base64 encoded")?;
    Ok(json!({"type":"image","source":{"type":"base64","media_type":media_type,"data":data}}))
}
fn image_url(part: &Object) -> Result<&str, String> {
    match present(part, "image_url") {
        Some(Value::String(url)) => Ok(url),
        Some(Value::Object(o)) => string(o.get("url").unwrap_or(&Value::Null), "image_url.url"),
        _ => Err("image part requires image_url; file_id images are not supported".into()),
    }
}

fn text_block(text: &str) -> Option<Value> {
    (!text.is_empty()).then(|| json!({"type":"text","text":text}))
}

/// Builds alternating Anthropic messages; consecutive same-role turns merge.
#[derive(Default)]
struct History {
    system: Vec<Value>,
    messages: Vec<Value>,
}
impl History {
    fn push(&mut self, role: &str, blocks: Vec<Value>) {
        if blocks.is_empty() {
            return;
        }
        if let Some(last) = self.messages.last_mut()
            && last["role"] == role
        {
            last["content"]
                .as_array_mut()
                .expect("content is array")
                .extend(blocks);
            return;
        }
        self.messages.push(json!({"role":role,"content":blocks}));
    }
    fn system(&mut self, text: &str) {
        self.system.extend(text_block(text));
    }
    fn has_tool_calls(&self) -> bool {
        self.messages.iter().any(|m| {
            m["content"]
                .as_array()
                .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_use"))
        })
    }
}

fn arguments(value: &Value) -> Result<Value, String> {
    let text = string(value, "function arguments")?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(text) {
        Ok(v @ Value::Object(_)) => Ok(v),
        _ => Err("function arguments must be a JSON object string".into()),
    }
}

fn tool(
    name: &Value,
    description: Option<&Value>,
    parameters: Option<&Value>,
    strict: Option<&Value>,
) -> Result<Value, String> {
    let mut schema = parameters
        .cloned()
        .unwrap_or_else(|| json!({"type":"object","properties":{}}));
    // Upstream input schemas must be a plain object schema; a top-level
    // oneOf/anyOf/allOf (common in MCP tools) fails the whole request.
    // `properties` still describes every field, so only the combinator goes.
    if let Some(schema) = schema.as_object_mut() {
        for combinator in ["oneOf", "anyOf", "allOf"] {
            schema.remove(combinator);
        }
        schema.entry("type").or_insert(json!("object"));
    }
    let mut tool = json!({
        "name": string(name, "tool name")?,
        "input_schema": schema,
    });
    if let Some(description) = description {
        tool["description"] = json!(string(description, "tool description")?);
    }
    if let Some(strict) = strict {
        tool["strict"] = json!(strict.as_bool().ok_or("tool strict must be a boolean")?);
    }
    Ok(tool)
}

/// Applies tool_choice / parallel_tool_calls to an Anthropic body.
fn apply_tools(
    body: &mut Value,
    tools: Vec<Value>,
    choice: Option<&Value>,
    parallel: Option<bool>,
    history: &History,
) -> Result<(), String> {
    let mut choice = match choice {
        None => None,
        Some(Value::String(kind)) => match kind.as_str() {
            "auto" => Some(json!({"type":"auto"})),
            "required" => Some(json!({"type":"any"})),
            "none" => {
                // Without tools the model cannot call one. Tool history still
                // needs the definitions, and Converse has no "none" choice.
                if history.has_tool_calls() {
                    return Err("tool_choice=none is not supported once the conversation contains tool calls".into());
                }
                return Ok(());
            }
            _ => return Err("tool_choice must be auto, required, none or a function".into()),
        },
        Some(Value::Object(o)) => {
            if present(o, "type").and_then(Value::as_str) != Some("function") {
                return Err("Only function tool_choice is supported".into());
            }
            let name = o
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| o.get("name"))
                .ok_or("tool_choice function requires a name")?;
            Some(json!({"type":"tool","name":string(name, "tool_choice name")?}))
        }
        Some(_) => return Err("tool_choice must be a string or object".into()),
    };
    if tools.is_empty() {
        // "auto" with nothing to call is a no-op, which clients send freely.
        if choice.as_ref().is_some_and(|c| c["type"] != "auto") {
            return Err("tool_choice requires at least one tool".into());
        }
        return Ok(());
    }
    if parallel == Some(false) {
        choice.get_or_insert_with(|| json!({"type":"auto"}))["disable_parallel_tool_use"] =
            json!(true);
    }
    body["tools"] = json!(tools);
    if let Some(choice) = choice {
        body["tool_choice"] = choice;
    }
    Ok(())
}

fn json_schema_format(schema: Option<&Value>) -> Result<Value, String> {
    let schema = schema.ok_or("json_schema format requires a schema")?;
    object(schema, "json_schema schema")?;
    Ok(json!({"type":"json_schema","schema":schema}))
}

fn finalize(mut body: Value, history: History) -> Result<Value, String> {
    if history.messages.is_empty() {
        return Err("At least one user or assistant message is required".into());
    }
    body["messages"] = json!(history.messages);
    if !history.system.is_empty() {
        body["system"] = json!(history.system);
    }
    Ok(body)
}

// ---------------------------------------------------------------- Chat Completions

pub fn chat_request(value: &Value) -> Result<Converted, String> {
    let body = object(value, "request")?;
    reject_unsupported(
        body,
        &[
            "n",
            "logprobs",
            "top_logprobs",
            "logit_bias",
            "frequency_penalty",
            "presence_penalty",
            "audio",
            "prediction",
            "web_search_options",
            "functions",
            "function_call",
        ],
    )?;
    if present(body, "modalities")
        .and_then(Value::as_array)
        .is_some_and(|m| m.iter().any(|v| v != "text"))
    {
        return Err("Only text output modality is supported".into());
    }
    let model = string(body.get("model").unwrap_or(&Value::Null), "model")?;
    let stream = boolean(body, "stream")?.unwrap_or(false);
    let include_usage = present(body, "stream_options")
        .and_then(|o| o.get("include_usage"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut out = json!({
        "model": model,
        "max_tokens": max_tokens(body, &["max_completion_tokens", "max_tokens"])?,
        "stream": stream,
    });
    for field in ["temperature", "top_p"] {
        if let Some(v) = present(body, field) {
            out[field] = v.clone();
        }
    }
    match present(body, "stop") {
        None => {}
        Some(Value::String(s)) => out["stop_sequences"] = json!([s]),
        Some(Value::Array(a)) if !a.is_empty() => out["stop_sequences"] = json!(a),
        Some(Value::Array(_)) => {}
        Some(_) => return Err("stop must be a string or array".into()),
    }

    let mut history = History::default();
    for message in array(body.get("messages").unwrap_or(&Value::Null), "messages")? {
        let message = object(message, "message")?;
        let role = string(message.get("role").unwrap_or(&Value::Null), "message.role")?;
        let content = present(message, "content");
        match role {
            "system" | "developer" => {
                for part in chat_parts(content)? {
                    match part {
                        Part::Text(t) => history.system(&t),
                        Part::Image(_) => return Err("system messages must be text".into()),
                    }
                }
            }
            "user" => {
                let blocks = chat_parts(content)?
                    .into_iter()
                    .filter_map(Part::block)
                    .collect();
                history.push("user", blocks);
            }
            "assistant" => {
                let mut blocks: Vec<Value> = chat_parts(content)?
                    .into_iter()
                    .filter_map(Part::block)
                    .collect();
                if let Some(calls) = present(message, "tool_calls") {
                    for call in array(calls, "tool_calls")? {
                        let call = object(call, "tool_call")?;
                        let function = object(
                            call.get("function").unwrap_or(&Value::Null),
                            "tool_call.function",
                        )?;
                        blocks.push(json!({
                            "type":"tool_use",
                            "id":string(call.get("id").unwrap_or(&Value::Null), "tool_call.id")?,
                            "name":string(function.get("name").unwrap_or(&Value::Null), "tool_call.function.name")?,
                            "input":arguments(function.get("arguments").unwrap_or(&json!("")))?,
                        }));
                    }
                }
                history.push("assistant", blocks);
            }
            "tool" => {
                let id = string(
                    message.get("tool_call_id").unwrap_or(&Value::Null),
                    "tool_call_id",
                )?;
                let text: String = chat_parts(content)?
                    .into_iter()
                    .map(|p| match p {
                        Part::Text(t) => Ok(t),
                        Part::Image(_) => Err("tool message content must be text".to_owned()),
                    })
                    .collect::<Result<_, _>>()?;
                history.push(
                    "user",
                    vec![json!({"type":"tool_result","tool_use_id":id,"content":text})],
                );
            }
            _ => return Err(format!("Unsupported message role: {role}")),
        }
    }

    let mut tools = Vec::new();
    if let Some(list) = present(body, "tools") {
        for entry in array(list, "tools")? {
            let entry = object(entry, "tool")?;
            if present(entry, "type").and_then(Value::as_str) != Some("function") {
                return Err("Only function tools are supported".into());
            }
            let f = object(
                entry.get("function").unwrap_or(&Value::Null),
                "tool.function",
            )?;
            tools.push(tool(
                f.get("name").unwrap_or(&Value::Null),
                present(f, "description"),
                present(f, "parameters"),
                present(f, "strict"),
            )?);
        }
    }
    apply_tools(
        &mut out,
        tools,
        present(body, "tool_choice"),
        boolean(body, "parallel_tool_calls")?,
        &history,
    )?;

    if let Some(format) = present(body, "response_format") {
        let format = object(format, "response_format")?;
        match present(format, "type").and_then(Value::as_str) {
            Some("text") => {}
            Some("json_schema") => {
                let spec = object(
                    format.get("json_schema").unwrap_or(&Value::Null),
                    "response_format.json_schema",
                )?;
                out["output_config"] =
                    json!({"format":json_schema_format(present(spec, "schema"))?});
            }
            Some("json_object") => {
                return Err("response_format json_object is not supported; use json_schema".into());
            }
            _ => return Err("Unsupported response_format.type".into()),
        }
    }
    let effort = present(body, "reasoning_effort")
        .map(|v| string(v, "reasoning_effort").and_then(effort))
        .transpose()?;
    Ok(Converted {
        body: finalize(out, history)?,
        effort,
        stream,
        include_usage,
    })
}

enum Part {
    Text(String),
    Image(Value),
}
impl Part {
    fn block(self) -> Option<Value> {
        match self {
            Part::Text(t) => text_block(&t),
            Part::Image(i) => Some(i),
        }
    }
}
fn chat_parts(content: Option<&Value>) -> Result<Vec<Part>, String> {
    match content {
        None => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![Part::Text(s.clone())]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| {
                let part = object(part, "content part")?;
                match present(part, "type").and_then(Value::as_str) {
                    Some("text") => Ok(Part::Text(
                        string(part.get("text").unwrap_or(&Value::Null), "text")?.to_owned(),
                    )),
                    Some("refusal") => Ok(Part::Text(
                        string(part.get("refusal").unwrap_or(&Value::Null), "refusal")?.to_owned(),
                    )),
                    Some("image_url") => Ok(Part::Image(image(image_url(part)?)?)),
                    Some(other) => Err(format!("Unsupported content part type: {other}")),
                    None => Err("content part requires a type".into()),
                }
            })
            .collect(),
        Some(_) => Err("message content must be a string or array".into()),
    }
}

fn finish_reason(stop: &str) -> &'static str {
    match stop {
        "tool_use" => "tool_calls",
        "max_tokens" | "model_context_window_exceeded" => "length",
        "refusal" => "content_filter",
        _ => "stop",
    }
}
fn chat_usage(usage: &Value) -> Value {
    let cached = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
    let input = usage["input_tokens"].as_u64().unwrap_or(0)
        + cached
        + usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
    let output = usage["output_tokens"].as_u64().unwrap_or(0);
    json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output,
        "prompt_tokens_details":{"cached_tokens":cached}})
}

pub fn chat_response(message: &Value) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    for block in message["content"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("text") => text.push_str(block["text"].as_str().unwrap_or("")),
            Some("thinking") => reasoning.push_str(block["thinking"].as_str().unwrap_or("")),
            Some("tool_use") => calls.push(json!({
                "id":block["id"],"type":"function",
                "function":{"name":block["name"],"arguments":block["input"].to_string()}
            })),
            _ => {}
        }
    }
    let mut reply = json!({"role":"assistant","content":if text.is_empty() && !calls.is_empty() {Value::Null} else {json!(text)},"refusal":null});
    if !calls.is_empty() {
        reply["tool_calls"] = json!(calls);
    }
    if !reasoning.is_empty() {
        reply["reasoning_content"] = json!(reasoning);
    }
    json!({
        "id":id("chatcmpl"),"object":"chat.completion","created":now(),"model":message["model"],
        "choices":[{"index":0,"message":reply,"logprobs":null,
            "finish_reason":finish_reason(message["stop_reason"].as_str().unwrap_or(""))}],
        "usage":chat_usage(&message["usage"]),
    })
}

/// Converts Anthropic stream events into `data:` payloads for Chat Completions.
pub struct ChatStream {
    id: String,
    created: u64,
    model: String,
    include_usage: bool,
    /// Anthropic block index -> (tool_calls index, saw arguments)
    tools: HashMap<u64, (usize, bool)>,
    usage: Value,
    done: bool,
}
impl ChatStream {
    pub fn new(include_usage: bool) -> Self {
        Self {
            id: id("chatcmpl"),
            created: now(),
            model: String::new(),
            include_usage,
            tools: HashMap::new(),
            usage: Value::Null,
            done: false,
        }
    }
    fn chunk(&self, delta: Value, finish: Option<&str>) -> String {
        let mut chunk = json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,
            "model":self.model,"choices":[{"index":0,"delta":delta,"logprobs":null,"finish_reason":finish}]});
        if self.include_usage {
            chunk["usage"] = Value::Null;
        }
        chunk.to_string()
    }
    pub fn event(&mut self, event: &Value) -> Vec<String> {
        if self.done {
            return Vec::new();
        }
        let index = event["index"].as_u64().unwrap_or(0);
        match event["type"].as_str().unwrap_or("") {
            "message_start" => {
                self.model = event["message"]["model"].as_str().unwrap_or("").to_owned();
                vec![self.chunk(json!({"role":"assistant","content":""}), None)]
            }
            "content_block_start" if event["content_block"]["type"] == "tool_use" => {
                let ordinal = self.tools.len();
                self.tools.insert(index, (ordinal, false));
                let block = &event["content_block"];
                vec![self.chunk(
                    json!({"tool_calls":[{"index":ordinal,"id":block["id"],"type":"function",
                        "function":{"name":block["name"],"arguments":""}}]}),
                    None,
                )]
            }
            "content_block_delta" => {
                let delta = &event["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => vec![self.chunk(json!({"content":delta["text"]}), None)],
                    "thinking_delta" => {
                        vec![self.chunk(json!({"reasoning_content":delta["thinking"]}), None)]
                    }
                    "input_json_delta" => {
                        let Some(tool) = self.tools.get_mut(&index) else {
                            return Vec::new();
                        };
                        tool.1 = true;
                        let ordinal = tool.0;
                        vec![self.chunk(
                            json!({"tool_calls":[{"index":ordinal,"function":{"arguments":delta["partial_json"]}}]}),
                            None,
                        )]
                    }
                    _ => Vec::new(),
                }
            }
            // A tool without input never sends a delta; give clients valid JSON.
            "content_block_stop" => match self.tools.get(&index) {
                Some((ordinal, false)) => vec![self.chunk(
                    json!({"tool_calls":[{"index":ordinal,"function":{"arguments":"{}"}}]}),
                    None,
                )],
                _ => Vec::new(),
            },
            "message_delta" => {
                self.usage = event["usage"].clone();
                let reason = finish_reason(event["delta"]["stop_reason"].as_str().unwrap_or(""));
                vec![self.chunk(json!({}), Some(reason))]
            }
            "message_stop" => {
                self.done = true;
                let mut out = Vec::new();
                if self.include_usage {
                    out.push(
                        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,
                            "model":self.model,"choices":[],"usage":chat_usage(&self.usage)})
                        .to_string(),
                    );
                }
                out.push("[DONE]".into());
                out
            }
            "error" => {
                self.done = true;
                vec![json!({"error":stream_error(&event["error"])}).to_string()]
            }
            _ => Vec::new(),
        }
    }
}

fn stream_error(error: &Value) -> Value {
    let (kind, fallback_code) = match error["type"].as_str() {
        Some("permission_error") => ("permission_error", "permission_denied"),
        Some("authentication_error") => ("invalid_request_error", "invalid_api_key"),
        Some("invalid_request_error" | "not_found_error") => {
            ("invalid_request_error", "invalid_request")
        }
        Some("rate_limit_error") => ("rate_limit_error", "rate_limit_exceeded"),
        _ => ("server_error", "server_error"),
    };
    json!({
        "message":error["message"], "type":kind, "param":null,
        "code":error["code"].as_str().unwrap_or(fallback_code),
    })
}

// ---------------------------------------------------------------- Responses

pub struct ResponsesConverted {
    pub converted: Converted,
    /// Request fields echoed on the response object.
    pub echo: Object,
    /// Restores Responses tool identities on upstream tool calls.
    pub tools: ToolNames,
}

#[derive(Clone, Debug, PartialEq)]
struct ToolIdentity {
    namespace: Option<String>,
    name: String,
    custom: bool,
}

/// Responses tools that Anthropic cannot express directly (functions grouped
/// in a `namespace`, freeform `custom` tools) are flattened to plain tools.
/// This table maps each upstream name back to the original identity.
#[derive(Clone, Debug, Default)]
pub struct ToolNames(HashMap<String, ToolIdentity>);
impl ToolNames {
    fn register(&mut self, identity: ToolIdentity) -> Result<String, String> {
        let flat = flat_name(identity.namespace.as_deref(), &identity.name);
        if let Some(existing) = self.0.get(&flat) {
            if *existing != identity {
                return Err(format!("tool names collide after conversion: {flat}"));
            }
        } else {
            self.0.insert(flat.clone(), identity);
        }
        Ok(flat)
    }
    fn custom(&self, flat: &str) -> bool {
        self.0.get(flat).is_some_and(|t| t.custom)
    }
    /// The Responses name and namespace for an upstream tool name.
    fn original(&self, flat: &str) -> (String, Option<String>) {
        match self.0.get(flat) {
            Some(t) => (t.name.clone(), t.namespace.clone()),
            None => (flat.to_owned(), None),
        }
    }
}

/// Builds an upstream tool name matching `^[a-zA-Z0-9_-]{1,64}$`.
fn flat_name(namespace: Option<&str>, name: &str) -> String {
    let joined = match namespace {
        Some(ns) if ns.ends_with('_') => format!("{ns}{name}"),
        Some(ns) => format!("{ns}__{name}"),
        None => name.to_owned(),
    };
    let clean: String = joined
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean == joined && clean.len() <= 64 && !clean.is_empty() {
        return clean;
    }
    // Lossy rewrites get a hash suffix so distinct names stay distinct.
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(joined.as_bytes()));
    format!("{}_{}", &clean[..clean.len().min(55)], &hash[..8])
}

const CUSTOM_INPUT_HINT: &str =
    "Put the raw tool input in the `input` string as plain text, not JSON-encoded.";

/// A freeform tool becomes a function with one string argument, `input`.
fn custom_tool(entry: &Object, flat: String) -> Result<Value, String> {
    let mut description = present(entry, "description")
        .map(|d| string(d, "tool description"))
        .transpose()?
        .unwrap_or("")
        .to_owned();
    if !description.is_empty() {
        description.push_str("\n\n");
    }
    description.push_str(CUSTOM_INPUT_HINT);
    if let Some(format) = present(entry, "format").and_then(Value::as_object)
        && present(format, "type").and_then(Value::as_str) == Some("grammar")
    {
        let syntax = present(format, "syntax")
            .and_then(Value::as_str)
            .unwrap_or("lark");
        let definition = present(format, "definition")
            .and_then(Value::as_str)
            .unwrap_or("");
        description.push_str(&format!(
            "\nThe input must match this {syntax} grammar exactly:\n```\n{definition}\n```"
        ));
    }
    Ok(json!({
        "name": flat,
        "description": description,
        "input_schema": {"type":"object","properties":{"input":{"type":"string"}},"required":["input"]},
    }))
}

fn function_tool(
    entry: &Object,
    namespace: Option<&str>,
    names: &mut ToolNames,
) -> Result<Value, String> {
    let name = string(entry.get("name").unwrap_or(&Value::Null), "tool name")?;
    let flat = names.register(ToolIdentity {
        namespace: namespace.map(str::to_owned),
        name: name.to_owned(),
        custom: false,
    })?;
    tool(
        &json!(flat),
        present(entry, "description"),
        present(entry, "parameters"),
        present(entry, "strict"),
    )
}

/// The namespace OpenAI uses for ordinary top-level tools.
const TOP_LEVEL_NAMESPACE: &str = "functions";

/// Converts one Responses tool definition, flattening namespaces.
fn response_tool(
    entry: &Object,
    namespace: Option<&str>,
    tools: &mut Vec<Value>,
    names: &mut ToolNames,
) -> Result<(), String> {
    match present(entry, "type").and_then(Value::as_str) {
        Some("function") => tools.push(function_tool(entry, namespace, names)?),
        Some("custom") => {
            let name = string(entry.get("name").unwrap_or(&Value::Null), "tool name")?;
            let flat = names.register(ToolIdentity {
                namespace: namespace.map(str::to_owned),
                name: name.to_owned(),
                custom: true,
            })?;
            tools.push(custom_tool(entry, flat)?);
        }
        Some("namespace") => {
            if namespace.is_some() {
                return Err("namespaces cannot be nested".into());
            }
            let inner = string(entry.get("name").unwrap_or(&Value::Null), "namespace name")?;
            let inner = (inner != TOP_LEVEL_NAMESPACE).then_some(inner);
            for tool in array(
                entry.get("tools").unwrap_or(&Value::Null),
                "namespace tools",
            )? {
                response_tool(object(tool, "namespace tool")?, inner, tools, names)?;
            }
        }
        Some(kind) if HOSTED_TOOLS.contains(&kind) => {}
        Some(other) => {
            return Err(format!(
                "Hosted tool type {other} is not supported; only function, namespace and custom tools are"
            ));
        }
        None => return Err("tool requires a type".into()),
    }
    Ok(())
}

/// Tools the OpenAI server executes itself. QuickWay cannot run them, so they
/// are left out and the model only sees the client-executed tools.
const HOSTED_TOOLS: &[&str] = &[
    "web_search",
    "web_search_preview",
    "web_search_2025_08_26",
    "image_generation",
];

pub fn responses_request(value: &Value) -> Result<ResponsesConverted, String> {
    let body = object(value, "request")?;
    reject_unsupported(body, &["background", "top_logprobs", "max_tool_calls"])?;
    for field in ["previous_response_id", "conversation", "prompt"] {
        if present(body, field).is_some() {
            return Err(format!(
                "{field} requires server-side state, which QuickWay does not store; send the full input instead"
            ));
        }
    }
    let model = string(body.get("model").unwrap_or(&Value::Null), "model")?;
    let stream = boolean(body, "stream")?.unwrap_or(false);
    let max = max_tokens(body, &["max_output_tokens"])?;
    let mut out = json!({"model":model,"max_tokens":max,"stream":stream});
    for field in ["temperature", "top_p"] {
        if let Some(v) = present(body, field) {
            out[field] = v.clone();
        }
    }

    let mut history = History::default();
    if let Some(instructions) = present(body, "instructions") {
        history.system(string(instructions, "instructions")?);
    }
    let mut tools = Vec::new();
    let mut names = ToolNames::default();
    if let Some(list) = present(body, "tools") {
        for entry in array(list, "tools")? {
            response_tool(object(entry, "tool")?, None, &mut tools, &mut names)?;
        }
    }
    let items = match body.get("input") {
        Some(Value::String(text)) => {
            history.push("user", text_block(text).into_iter().collect());
            &Vec::new()
        }
        Some(Value::Array(items)) => items,
        _ => return Err("input must be a string or array".into()),
    };
    // Codex can deliver its toolset as an `additional_tools` input item.
    // Tools are registered before any history so replayed calls get the same
    // upstream names as the definitions.
    for item in items {
        let item = object(item, "input item")?;
        if present(item, "type").and_then(Value::as_str) == Some("additional_tools") {
            for entry in array(
                item.get("tools").unwrap_or(&Value::Null),
                "additional_tools.tools",
            )? {
                response_tool(object(entry, "tool")?, None, &mut tools, &mut names)?;
            }
        }
    }
    for item in items {
        input_item(object(item, "input item")?, &mut history, &mut names)?;
    }
    // Responses clients such as Codex send parallel_tool_calls=false by
    // default and still run every returned call in order. Converse cannot
    // enforce the limit, so it is treated as a hint rather than failing.
    boolean(body, "parallel_tool_calls")?;
    apply_tools(
        &mut out,
        tools,
        present(body, "tool_choice"),
        None,
        &history,
    )?;

    if let Some(format) = present(body, "text").and_then(|t| t.get("format")) {
        let format = object(format, "text.format")?;
        match present(format, "type").and_then(Value::as_str) {
            Some("text") => {}
            Some("json_schema") => {
                out["output_config"] =
                    json!({"format":json_schema_format(present(format, "schema"))?});
            }
            Some("json_object") => {
                return Err("text.format json_object is not supported; use json_schema".into());
            }
            _ => return Err("Unsupported text.format.type".into()),
        }
    }
    let effort = present(body, "reasoning")
        .and_then(|r| r.get("effort"))
        .filter(|v| !v.is_null())
        .map(|v| string(v, "reasoning.effort").and_then(effort))
        .transpose()?;

    let mut echo = Object::new();
    for (field, default) in [
        ("instructions", Value::Null),
        ("max_output_tokens", Value::Null),
        ("metadata", json!({})),
        ("parallel_tool_calls", json!(true)),
        ("reasoning", json!({"effort":null,"summary":null})),
        ("temperature", json!(1.0)),
        ("text", json!({"format":{"type":"text"}})),
        ("tool_choice", json!("auto")),
        ("tools", json!([])),
        ("top_p", json!(1.0)),
        ("user", Value::Null),
    ] {
        echo.insert(
            field.into(),
            present(body, field).cloned().unwrap_or(default),
        );
    }
    Ok(ResponsesConverted {
        converted: Converted {
            body: finalize(out, history)?,
            effort,
            stream,
            include_usage: true,
        },
        echo,
        tools: names,
    })
}

fn call_id<'a>(item: &'a Object, kind: &str) -> Result<&'a str, String> {
    string(
        item.get("call_id").unwrap_or(&Value::Null),
        &format!("{kind}.call_id"),
    )
}

fn input_item(item: &Object, history: &mut History, names: &mut ToolNames) -> Result<(), String> {
    let kind = present(item, "type").and_then(Value::as_str);
    match kind {
        None | Some("message") => {
            let role = string(item.get("role").unwrap_or(&Value::Null), "input role")?;
            let parts = response_parts(item.get("content").unwrap_or(&Value::Null))?;
            match role {
                "system" | "developer" => {
                    for part in parts {
                        if part["type"] != "text" {
                            return Err("system input must be text".into());
                        }
                        history.system(part["text"].as_str().unwrap_or(""));
                    }
                }
                "user" | "assistant" => history.push(role, parts),
                _ => return Err(format!("Unsupported input role: {role}")),
            }
        }
        Some(kind @ ("function_call" | "custom_tool_call")) => {
            let custom = kind == "custom_tool_call";
            let name = string(
                item.get("name").unwrap_or(&Value::Null),
                &format!("{kind}.name"),
            )?;
            let namespace = present(item, "namespace")
                .map(|n| string(n, &format!("{kind}.namespace")))
                .transpose()?
                .filter(|n| *n != TOP_LEVEL_NAMESPACE);
            let flat = names.register(ToolIdentity {
                namespace: namespace.map(str::to_owned),
                name: name.to_owned(),
                custom,
            })?;
            let input = if custom {
                json!({"input":string(item.get("input").unwrap_or(&json!("")), "custom_tool_call.input")?})
            } else {
                arguments(item.get("arguments").unwrap_or(&json!("")))?
            };
            history.push(
                "assistant",
                vec![
                    json!({"type":"tool_use","id":call_id(item, kind)?,"name":flat,"input":input}),
                ],
            )
        }
        Some(kind @ ("function_call_output" | "custom_tool_call_output")) => {
            let id = call_id(item, kind)?;
            let content = match item.get("output") {
                Some(Value::String(s)) => json!(s),
                Some(output @ Value::Array(_)) => json!(response_parts(output)?),
                _ => return Err(format!("{kind}.output must be a string or array")),
            };
            history.push(
                "user",
                vec![json!({"type":"tool_result","tool_use_id":id,"content":content})],
            );
        }
        Some("reasoning") => {
            // Only QuickWay-issued reasoning can be replayed; others have no
            // signature the upstream model would accept.
            let encrypted = present(item, "encrypted_content").and_then(Value::as_str);
            if let Some(signature) = encrypted
                .and_then(|e| e.strip_prefix(SIGNED))
                .filter(|s| !s.is_empty())
            {
                let thinking: String = item
                    .get("summary")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s["text"].as_str())
                    .collect();
                history.push(
                    "assistant",
                    vec![json!({"type":"thinking","thinking":thinking,"signature":signature})],
                );
            } else if let Some(data) = encrypted
                .and_then(|e| e.strip_prefix(REDACTED))
                .filter(|s| !s.is_empty())
            {
                history.push(
                    "assistant",
                    vec![json!({"type":"redacted_thinking","data":data})],
                );
            }
        }
        // Already registered as tools by responses_request.
        Some("additional_tools") => {}
        Some("item_reference") => {
            return Err(
                "item_reference requires stored responses, which QuickWay does not keep".into(),
            );
        }
        Some(other) => return Err(format!("Unsupported input item type: {other}")),
    }
    Ok(())
}

fn response_parts(content: &Value) -> Result<Vec<Value>, String> {
    match content {
        Value::String(s) => Ok(text_block(s).into_iter().collect()),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                let part = object(part, "content part")?;
                match present(part, "type").and_then(Value::as_str) {
                    Some("input_text" | "output_text" | "text") => out.extend(text_block(string(
                        part.get("text").unwrap_or(&Value::Null),
                        "text",
                    )?)),
                    Some("refusal") => out.extend(text_block(string(
                        part.get("refusal").unwrap_or(&Value::Null),
                        "refusal",
                    )?)),
                    Some("input_image") => out.push(image(image_url(part)?)?),
                    Some(other) => return Err(format!("Unsupported content part type: {other}")),
                    None => return Err("content part requires a type".into()),
                }
            }
            Ok(out)
        }
        _ => Err("content must be a string or array".into()),
    }
}

fn responses_usage(usage: &Value) -> Value {
    let chat = chat_usage(usage);
    json!({"input_tokens":chat["prompt_tokens"],"input_tokens_details":{"cached_tokens":chat["prompt_tokens_details"]["cached_tokens"]},
        "output_tokens":chat["completion_tokens"],"output_tokens_details":{"reasoning_tokens":0},"total_tokens":chat["total_tokens"]})
}

fn reasoning_item(id: &str, thinking: &str, encrypted: String) -> Value {
    // Unsigned reasoning (e.g. GPT) cannot be replayed upstream.
    let encrypted = if encrypted == SIGNED || encrypted == REDACTED {
        Value::Null
    } else {
        json!(encrypted)
    };
    let summary = if thinking.is_empty() {
        json!([])
    } else {
        json!([{"type":"summary_text","text":thinking}])
    };
    json!({"id":id,"type":"reasoning","summary":summary,"encrypted_content":encrypted})
}
fn message_item(id: &str, text: &str, status: &str) -> Value {
    json!({"id":id,"type":"message","status":status,"role":"assistant",
        "content":[{"type":"output_text","text":text,"annotations":[]}]})
}
/// Builds a `function_call`, or a `custom_tool_call` for a freeform tool,
/// under the tool's original name and namespace.
fn call_item(
    id: &str,
    call_id: &Value,
    flat: &str,
    arguments: &str,
    status: &str,
    names: &ToolNames,
) -> Value {
    let (name, namespace) = names.original(flat);
    if names.custom(flat) {
        // The upstream call carries `{"input": "..."}`; the client wants the raw text.
        let input = serde_json::from_str::<Value>(arguments)
            .ok()
            .and_then(|v| v["input"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut item = json!({"id":id,"type":"custom_tool_call","status":status,"call_id":call_id,"name":name,"input":input});
        if let Some(namespace) = namespace {
            item["namespace"] = json!(namespace);
        }
        return item;
    }
    let mut item = json!({"id":id,"type":"function_call","status":status,"call_id":call_id,"name":name,"arguments":arguments});
    if let Some(namespace) = namespace {
        item["namespace"] = json!(namespace);
    }
    item
}

fn response_status(stop: &str) -> &'static str {
    match stop {
        "max_tokens" | "model_context_window_exceeded" | "refusal" => "incomplete",
        _ => "completed",
    }
}

pub fn responses_response(message: &Value, echo: &Object, names: &ToolNames) -> Value {
    let mut output = Vec::new();
    for block in message["content"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("thinking") => output.push(reasoning_item(
                &id("rs"),
                block["thinking"].as_str().unwrap_or(""),
                format!("{SIGNED}{}", block["signature"].as_str().unwrap_or("")),
            )),
            Some("redacted_thinking") => output.push(reasoning_item(
                &id("rs"),
                "",
                format!("{REDACTED}{}", block["data"].as_str().unwrap_or("")),
            )),
            Some("text") => output.push(message_item(
                &id("msg"),
                block["text"].as_str().unwrap_or(""),
                "completed",
            )),
            Some("tool_use") => output.push(call_item(
                &id("fc"),
                &block["id"],
                block["name"].as_str().unwrap_or(""),
                &block["input"].to_string(),
                "completed",
                names,
            )),
            _ => {}
        }
    }
    let mut state = ResponsesStream::new(echo.clone(), names.clone());
    state.model = message["model"].clone();
    state.output = output;
    state.stop = message["stop_reason"].as_str().unwrap_or("").to_owned();
    state.snapshot(response_status(&state.stop), Some(&message["usage"]))
}

struct OpenItem {
    kind: &'static str,
    id: String,
    output_index: usize,
    text: String,
    signature: String,
    call_id: Value,
    name: String,
}

/// Converts Anthropic stream events into named Responses SSE events.
pub struct ResponsesStream {
    id: String,
    created: u64,
    model: Value,
    echo: Object,
    sequence: u64,
    open: HashMap<u64, OpenItem>,
    output: Vec<Value>,
    stop: String,
    usage: Value,
    done: bool,
    names: ToolNames,
}
impl ResponsesStream {
    pub fn new(echo: Object, names: ToolNames) -> Self {
        Self {
            id: id("resp"),
            created: now(),
            model: Value::Null,
            echo,
            sequence: 0,
            open: HashMap::new(),
            output: Vec::new(),
            stop: String::new(),
            usage: Value::Null,
            done: false,
            names,
        }
    }
    fn emit(&mut self, out: &mut Vec<(String, Value)>, kind: &str, mut data: Value) {
        data["type"] = json!(kind);
        data["sequence_number"] = json!(self.sequence);
        self.sequence += 1;
        out.push((kind.to_owned(), data));
    }
    fn snapshot(&self, status: &str, usage: Option<&Value>) -> Value {
        let incomplete = match (status, self.stop.as_str()) {
            ("incomplete", "refusal") => json!({"reason":"content_filter"}),
            ("incomplete", _) => json!({"reason":"max_output_tokens"}),
            _ => Value::Null,
        };
        let mut response = json!({
            "id":self.id,"object":"response","created_at":self.created,"status":status,"error":null,
            "incomplete_details":incomplete,"model":self.model,"output":self.output,
            "previous_response_id":null,"store":false,"truncation":"disabled",
            "usage":usage.map(responses_usage),
        });
        for (k, v) in &self.echo {
            response[k] = v.clone();
        }
        response
    }
    pub fn event(&mut self, event: &Value) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        if self.done {
            return out;
        }
        let index = event["index"].as_u64().unwrap_or(0);
        match event["type"].as_str().unwrap_or("") {
            "message_start" => {
                self.model = event["message"]["model"].clone();
                let response = self.snapshot("in_progress", None);
                self.emit(&mut out, "response.created", json!({"response":response}));
                let response = self.snapshot("in_progress", None);
                self.emit(
                    &mut out,
                    "response.in_progress",
                    json!({"response":response}),
                );
            }
            "content_block_start" => {
                let block = &event["content_block"];
                let output_index = self.output.len() + self.open.len();
                let (kind, item_id, item) = match block["type"].as_str().unwrap_or("") {
                    "text" => {
                        let item_id = id("msg");
                        let mut item = message_item(&item_id, "", "in_progress");
                        item["content"] = json!([]);
                        ("text", item_id, item)
                    }
                    "thinking" | "redacted_thinking" => {
                        let item_id = id("rs");
                        let item = json!({"id":item_id,"type":"reasoning","summary":[]});
                        (
                            if block["type"] == "thinking" {
                                "thinking"
                            } else {
                                "redacted"
                            },
                            item_id,
                            item,
                        )
                    }
                    "tool_use" => {
                        let item_id = id("fc");
                        let flat = block["name"].as_str().unwrap_or("");
                        let item =
                            call_item(&item_id, &block["id"], flat, "", "in_progress", &self.names);
                        let kind = if self.names.custom(flat) {
                            "custom"
                        } else {
                            "tool"
                        };
                        (kind, item_id, item)
                    }
                    _ => return out,
                };
                self.emit(
                    &mut out,
                    "response.output_item.added",
                    json!({"output_index":output_index,"item":item}),
                );
                match kind {
                    "text" => self.emit(&mut out, "response.content_part.added", json!({"item_id":item_id,"output_index":output_index,
                        "content_index":0,"part":{"type":"output_text","text":"","annotations":[]}})),
                    "thinking" => self.emit(&mut out, "response.reasoning_summary_part.added", json!({"item_id":item_id,
                        "output_index":output_index,"summary_index":0,"part":{"type":"summary_text","text":""}})),
                    _ => {}
                }
                self.open.insert(
                    index,
                    OpenItem {
                        kind,
                        id: item_id,
                        output_index,
                        text: String::new(),
                        signature: block["data"].as_str().unwrap_or("").to_owned(),
                        call_id: block["id"].clone(),
                        name: block["name"].as_str().unwrap_or("").to_owned(),
                    },
                );
            }
            "content_block_delta" => {
                let delta = &event["delta"];
                let Some(item) = self.open.get_mut(&index) else {
                    return out;
                };
                let (item_id, output_index) = (item.id.clone(), item.output_index);
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let text = delta["text"].as_str().unwrap_or("");
                        item.text.push_str(text);
                        self.emit(
                            &mut out,
                            "response.output_text.delta",
                            json!({"item_id":item_id,"output_index":output_index,
                            "content_index":0,"delta":text,"logprobs":[]}),
                        );
                    }
                    "thinking_delta" => {
                        let text = delta["thinking"].as_str().unwrap_or("");
                        item.text.push_str(text);
                        self.emit(
                            &mut out,
                            "response.reasoning_summary_text.delta",
                            json!({"item_id":item_id,
                            "output_index":output_index,"summary_index":0,"delta":text}),
                        );
                    }
                    "signature_delta" => item
                        .signature
                        .push_str(delta["signature"].as_str().unwrap_or("")),
                    "input_json_delta" => {
                        let text = delta["partial_json"].as_str().unwrap_or("");
                        item.text.push_str(text);
                        // Custom tool input arrives JSON-wrapped; the raw text
                        // is sent whole in output_item.done instead of as deltas.
                        if item.kind == "custom" {
                            return out;
                        }
                        self.emit(
                            &mut out,
                            "response.function_call_arguments.delta",
                            json!({"item_id":item_id,
                            "output_index":output_index,"delta":text}),
                        );
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let Some(item) = self.open.remove(&index) else {
                    return out;
                };
                let (item_id, i) = (item.id.clone(), item.output_index);
                let done = match item.kind {
                    "text" => {
                        self.emit(
                            &mut out,
                            "response.output_text.done",
                            json!({"item_id":item_id,"output_index":i,
                            "content_index":0,"text":item.text,"logprobs":[]}),
                        );
                        self.emit(&mut out, "response.content_part.done", json!({"item_id":item_id,"output_index":i,
                            "content_index":0,"part":{"type":"output_text","text":item.text,"annotations":[]}}));
                        message_item(&item_id, &item.text, "completed")
                    }
                    "thinking" => {
                        self.emit(
                            &mut out,
                            "response.reasoning_summary_text.done",
                            json!({"item_id":item_id,
                            "output_index":i,"summary_index":0,"text":item.text}),
                        );
                        self.emit(&mut out, "response.reasoning_summary_part.done", json!({"item_id":item_id,
                            "output_index":i,"summary_index":0,"part":{"type":"summary_text","text":item.text}}));
                        reasoning_item(&item_id, &item.text, format!("{SIGNED}{}", item.signature))
                    }
                    "redacted" => {
                        reasoning_item(&item_id, "", format!("{REDACTED}{}", item.signature))
                    }
                    _ => {
                        let arguments = if item.text.trim().is_empty() {
                            "{}".to_owned()
                        } else {
                            item.text
                        };
                        let done = call_item(
                            &item_id,
                            &item.call_id,
                            &item.name,
                            &arguments,
                            "completed",
                            &self.names,
                        );
                        if item.kind == "custom" {
                            self.emit(
                                &mut out,
                                "response.custom_tool_call_input.done",
                                json!({"item_id":item_id,"output_index":i,"input":done["input"]}),
                            );
                        } else {
                            self.emit(
                                &mut out,
                                "response.function_call_arguments.done",
                                json!({"item_id":item_id,
                                "output_index":i,"arguments":arguments}),
                            );
                        }
                        done
                    }
                };
                self.emit(
                    &mut out,
                    "response.output_item.done",
                    json!({"output_index":i,"item":done}),
                );
                self.output.push(done);
            }
            "message_delta" => {
                self.stop = event["delta"]["stop_reason"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                self.usage = event["usage"].clone();
            }
            "message_stop" => {
                self.done = true;
                let status = response_status(&self.stop);
                let response = self.snapshot(status, Some(&self.usage.clone()));
                let kind = if status == "completed" {
                    "response.completed"
                } else {
                    "response.incomplete"
                };
                self.emit(&mut out, kind, json!({"response":response}));
            }
            "error" => {
                self.done = true;
                let mut response = self.snapshot("failed", None);
                let error = stream_error(&event["error"]);
                response["error"] = json!({"code":error["code"],"message":error["message"]});
                self.emit(&mut out, "response.failed", json!({"response":response}));
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
#[path = "openai_tests.rs"]
mod tests;
