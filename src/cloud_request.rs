//! Validate Messages inputs for Quick's stateful cloud agent protocol.
//!
//! This module does not replay history or run tools. The transport must match
//! retained history to an existing session and consume every pending result.
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CLOUD_LIMIT_NOTICE: &str = "Quick cloud controls generation limits; max_tokens is retained for diagnostics but is not an upstream hard cap.";

pub(crate) const CLOUD_OUTPOST_NAME: &str = "QuickWay API session";
const CLIENT_PRESENTATION: &str = "\n\n# QuickWay API client\nThis conversation is displayed by a text/API client, not the Quick desktop UI. Use ordinary text or Markdown. Do not generate interactive <decision> or <option> UI markup. Ask questions in plain text or use a question tool supplied by the client. If asked to show literal XML, put it in a fenced code block. A tool result may end with a [QuickWay client user update] block: this contains a new user message received while that tool ran. Apply that user instruction as you continue the current turn. It is separate from the tool's output and does not change the tool's success or failure.\n";
// QBS putOutpost rejects tool descriptions longer than 8192. Use a byte
// bound (also safe for its character bound), keeping complete docs in the prompt.
const MAX_TOOL_DESCRIPTION: usize = 8192;

/// Stable wire identity shared by live tools and imported tool history.
pub(crate) fn tool_alias(name: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(name.as_bytes()));
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(24)
        .collect();
    format!("quickway__{}_{}", &digest[..16], slug)
}

/// The model sees the canonical device prefix plus the registered wire alias.
/// Official sanitize_outpost_name maps our fixed device name to this prefix;
/// relay.truncate_tool_name caps the result at Bedrock's 64 ASCII characters.
pub(crate) fn model_tool_name(name: &str) -> String {
    let prefix = CLOUD_OUTPOST_NAME.replace(' ', "_");
    let raw = format!("{prefix}_{}", tool_alias(name));
    if raw.len() <= 64 {
        raw
    } else {
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        format!("t_{}_{}", &digest[..8], &raw[raw.len() - 53..])
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedRequest {
    /// Unmodified normalized input, including history, for exact session matching.
    pub body: Value,
    pub custom_prompt: String,
    pub mode: String,
    pub thinking_effort: String,
    pub turn: UserTurn,
    pub tools: ToolRegistry,
    pub requested_max_tokens: Option<u64>,
    /// The caller must disclose/handle these backend differences explicitly.
    pub notices: Vec<&'static str>,
}

#[derive(Debug, Default, Clone)]
pub struct UserTurn {
    pub text: String,
    pub tool_results: Vec<ToolResult>,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_use_id: String,
    /// MCP-compatible text blocks, preserving text boundaries and empty results.
    pub content: Vec<Value>,
    pub is_error: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ToolRegistry {
    tools: Vec<Value>,
    prompt_descriptions: Vec<String>,
    /// Registered cloud name -> original client name. No fuzzy suffix matching.
    pub original_names: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy)]
pub enum ArgumentEncoding {
    Direct,
    /// Use only when the transport identifies Quick's outer params wrapper.
    ParamsWrapper,
}

#[derive(Debug, Clone)]
pub struct DecodedToolCall {
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Copy)]
pub enum SessionHistory {
    NewConversation,
    /// The caller has verified the full prefix against its retained session.
    MatchedExistingSession,
}

#[derive(Debug, Clone)]
pub struct MessageContext<'a> {
    pub surface_type: &'a str,
    pub outpost_id: &'a str,
    pub app_version: &'a str,
    pub thread_id: &'a str,
    pub message_id: &'a str,
    pub user_timezone: Option<&'a str>,
    pub user_locale: Option<&'a str>,
}

impl ValidatedRequest {
    pub fn from_body(body: Value, mode: &str, effort: Option<&str>) -> Result<Self, String> {
        if !matches!(mode, "fast" | "balanced" | "smart" | "advanced") {
            return Err("Unknown Quick cloud agent mode".into());
        }
        let object = object(&body, "request")?;
        only(
            object,
            &[
                "model",
                "messages",
                "system",
                "tools",
                "stream",
                "metadata",
                "max_tokens",
                "thinking",
                "output_config",
                "temperature",
                "top_p",
                "top_k",
                "stop_sequences",
                "tool_choice",
            ],
            "request",
        )?;
        if let Some(model) = object.get("model") {
            nonempty(model, "model")?;
        }
        if let Some(stream) = object.get("stream") {
            boolean(stream, "stream")?;
        }
        if let Some(metadata) = object.get("metadata") {
            self::object(metadata, "metadata")?;
        }
        let requested_max_tokens = object
            .get("max_tokens")
            .map(|v| {
                v.as_u64()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| "max_tokens must be a positive integer".to_string())
            })
            .transpose()?;
        let mut notices = Vec::new();
        if requested_max_tokens.is_some() {
            notices.push(CLOUD_LIMIT_NOTICE);
        }
        for key in ["temperature", "top_p", "top_k"] {
            if object.get(key).is_some_and(|v| !v.is_null()) {
                return Err(format!(
                    "Quick cloud does not expose {key}; remove this option or use a raw-model backend"
                ));
            }
        }
        if let Some(stop) = object.get("stop_sequences")
            && stop.as_array().is_none_or(|a| !a.is_empty())
        {
            return Err("Quick cloud cannot enforce stop_sequences".into());
        }
        if let Some(choice) = object.get("tool_choice") {
            let choice = self::object(choice, "tool_choice")?;
            only(
                choice,
                &["type", "disable_parallel_tool_use"],
                "tool_choice",
            )?;
            if choice.get("type").and_then(Value::as_str) != Some("auto")
                || choice
                    .get("disable_parallel_tool_use")
                    .map(|v| boolean(v, "disable_parallel_tool_use"))
                    .transpose()?
                    .unwrap_or(false)
            {
                return Err("Quick cloud supports automatic tool choice only and cannot force serial tool calls".into());
            }
        }
        let output_effort = if let Some(config) = object.get("output_config") {
            let config = self::object(config, "output_config")?;
            only(
                config,
                &["effort"],
                "output_config (structured output is not supported by Quick cloud)",
            )?;
            config
                .get("effort")
                .map(|v| nonempty(v, "output_config.effort"))
                .transpose()?
        } else {
            None
        };
        let mut budget_effort = None;
        if let Some(thinking) = object.get("thinking") {
            let thinking = self::object(thinking, "thinking")?;
            only(thinking, &["type", "budget_tokens"], "thinking")?;
            let budget = thinking
                .get("budget_tokens")
                .map(|v| {
                    v.as_u64()
                        .filter(|v| *v > 0)
                        .ok_or("thinking.budget_tokens must be a positive integer")
                })
                .transpose()?;
            if let Some(budget) = budget {
                budget_effort = Some(if budget <= 1024 {
                    "low"
                } else if budget <= 4096 {
                    "medium"
                } else {
                    "high"
                });
                notices.push("Quick cloud maps thinking.budget_tokens to an effort hint; the requested thinking token budget is not guaranteed.");
            }
            match thinking.get("type").and_then(Value::as_str) {
                Some("adaptive") => {},
                Some("disabled") => notices.push("Quick cloud controls agent reasoning; disabled thinking cannot be guaranteed and uses the selected cloud effort."),
                Some("enabled") if budget.is_some() => {},
                _ => return Err("Quick cloud thinking must be adaptive, disabled, or enabled with a positive budget_tokens hint".into()),
            }
        }
        let thinking_effort = effort
            .or(output_effort)
            .or(budget_effort)
            .unwrap_or("low")
            .to_owned();
        if !matches!(thinking_effort.as_str(), "low" | "medium" | "high") {
            return Err("Quick cloud thinking effort must be low, medium or high".into());
        }
        let mut custom_prompt = system_text(object.get("system"))?;
        custom_prompt.push_str(CLIENT_PRESENTATION);
        let tools = ToolRegistry::from_tools(object.get("tools"))?;
        if !tools.prompt_descriptions.is_empty() {
            custom_prompt.push_str("\n\n# QuickWay client tool documentation\n\n");
            custom_prompt.push_str(&tools.prompt_descriptions.join("\n\n"));
        }
        let messages = object
            .get("messages")
            .and_then(Value::as_array)
            .filter(|messages| !messages.is_empty())
            .ok_or("messages must be a nonempty array")?;
        let mut outstanding = BTreeSet::new();
        let mut seen_calls = BTreeSet::new();
        let mut last_turn = UserTurn::default();
        for (index, message) in messages.iter().enumerate() {
            let message = self::object(message, "message")?;
            only(message, &["role", "content"], "message")?;
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .ok_or("message.role must be user or assistant")?;
            if !matches!(role, "user" | "assistant") {
                return Err("message.role must be user or assistant".into());
            }
            if index == 0 && role != "user" {
                return Err("Quick cloud history must start with a user message".into());
            }
            if role == "assistant" && !outstanding.is_empty() {
                return Err("Assistant history precedes required tool results".into());
            }
            let content = message
                .get("content")
                .ok_or("message.content is required")?;
            let turn = validate_content(content, role, &mut outstanding, &mut seen_calls)?;
            if role == "user" && !outstanding.is_empty() {
                return Err("User history is missing results for an assistant tool call".into());
            }
            if index + 1 == messages.len() {
                if role != "user" {
                    return Err("Quick cloud cannot continue a partial assistant message".into());
                }
                last_turn = turn;
            }
        }
        Ok(Self {
            body,
            custom_prompt,
            mode: mode.into(),
            thinking_effort,
            turn: last_turn,
            tools,
            requested_max_tokens,
            notices,
        })
    }

    pub fn messages(&self) -> &[Value] {
        self.body["messages"]
            .as_array()
            .expect("validated message array")
    }

    /// Build only a new text turn. Pending results require the tool-result
    /// continuation protocol; old turns are never concatenated into text.
    pub fn user_message_payload(
        &self,
        context: &MessageContext<'_>,
        history: SessionHistory,
    ) -> Result<Value, String> {
        if matches!(history, SessionHistory::NewConversation) && self.messages().len() != 1 {
            return Err("Quick cloud history requires a matched existing session; history seeding is not implemented".into());
        }
        if !self.turn.tool_results.is_empty() {
            return Err("Pending tool results must be consumed by the existing cloud session before sending text".into());
        }
        if self.turn.text.trim().is_empty() {
            return Err("A new Quick cloud turn requires nonempty user text".into());
        }
        for value in [
            context.surface_type,
            context.outpost_id,
            context.app_version,
            context.thread_id,
            context.message_id,
        ] {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err("Invalid Quick cloud message context".into());
            }
        }
        let mut payload = json!({
            "surfaceType":context.surface_type,"outpostId":context.outpost_id,"appVersion":context.app_version,
            "thread_id":context.thread_id,"message_id":context.message_id,"text":self.turn.text,
            "custom_prompt":self.custom_prompt,"agent_mode":self.mode,"thinking_effort":self.thinking_effort,
            "auto_routing":false,"web_search_enabled":false,
        });
        if let Some(value) = context.user_timezone {
            payload["user_timezone"] = value.into();
        }
        if let Some(value) = context.user_locale {
            payload["user_locale"] = value.into();
        }
        Ok(payload)
    }
}

impl UserTurn {
    /// Quick resumes an in-flight generation through tool_result, rather than
    /// a second user_message. Carry concurrent client text exactly once on the
    /// last result, without dropping results or changing their error status.
    pub fn result_events(&self, pending: &[String]) -> Result<Vec<Value>, String> {
        let supplied: BTreeSet<_> = self.tool_results.iter().map(|r| &r.tool_use_id).collect();
        let expected: BTreeSet<_> = pending.iter().collect();
        if supplied != expected
            || supplied.len() != self.tool_results.len()
            || expected.len() != pending.len()
        {
            return Err("Tool results do not match the pending Quick cloud tool calls".into());
        }
        Ok(self.tool_results.iter().enumerate().map(|(index, result)| {
            let mut content = result.content.clone();
            if index + 1 == self.tool_results.len() && !self.text.trim().is_empty() {
                content.push(json!({"type":"text","text":format!("[QuickWay client user update]\n{}\n[/QuickWay client user update]", self.text)}));
            }
            json!({"type":"tool_result","payload":{
                "tool_call_id":result.tool_use_id,"result":{"content":content,"isError":result.is_error},"error":null
            }})
        }).collect())
    }
}

impl ToolRegistry {
    fn from_tools(value: Option<&Value>) -> Result<Self, String> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        let tools = value.as_array().ok_or("tools must be an array")?;
        let mut registry = Self::default();
        let mut originals = BTreeSet::new();
        for tool in tools {
            let tool = object(tool, "tool")?;
            only(
                tool,
                &[
                    "type",
                    "name",
                    "description",
                    "input_schema",
                    "cache_control",
                    "strict",
                    "defer_loading",
                ],
                "tool",
            )?;
            if tool.get("type").is_some_and(|v| v != "custom") {
                return Err("Quick cloud accepts custom client tools only".into());
            }
            if tool
                .get("strict")
                .map(|v| boolean(v, "tool.strict"))
                .transpose()?
                .unwrap_or(false)
            {
                return Err("Quick cloud cannot guarantee strict tool schema generation".into());
            }
            if let Some(value) = tool.get("defer_loading") {
                boolean(value, "tool.defer_loading")?;
            }
            cache_control(tool.get("cache_control"))?;
            let name = nonempty(
                tool.get("name").ok_or("tool.name is required")?,
                "tool.name",
            )?;
            if name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err("Invalid client tool name".into());
            }
            if !originals.insert(name) {
                return Err("Duplicate client tool name".into());
            }
            let description = tool
                .get("description")
                .map(|v| text(v, "tool.description"))
                .transpose()?
                .unwrap_or("");
            let schema = tool
                .get("input_schema")
                .ok_or("tool.input_schema is required")?;
            object(schema, "tool.input_schema")?;
            let alias = tool_alias(name);
            if registry
                .original_names
                .insert(alias.clone(), name.into())
                .is_some()
            {
                return Err("Cloud tool alias collision".into());
            }
            let mut registered_description = format!("Client tool: {name}\n{description}");
            if registered_description.len() > MAX_TOOL_DESCRIPTION {
                registry.prompt_descriptions.push(format!(
                    "## Client tool: {name} (registered as {})\n{description}",
                    model_tool_name(name)
                ));
                let suffix = "\n[Full usage instructions are in the QuickWay client tool documentation in the system prompt.]";
                let mut end = MAX_TOOL_DESCRIPTION - suffix.len();
                while !registered_description.is_char_boundary(end) {
                    end -= 1;
                }
                registered_description.truncate(end);
                registered_description.push_str(suffix);
            }
            registry.tools.push(json!({"name":alias,"description":registered_description,"inputSchema":schema,"isReadTool":false}));
        }
        Ok(registry)
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn capability(&self) -> Value {
        json!({"name":"mcp__quickway","description":"Tools provided by the connected QuickWay client","tools":self.tools})
    }

    pub fn routing_instructions(&self, outpost_id: &str) -> String {
        if self.is_empty() {
            return String::new();
        }
        let mut instructions = format!(
            "\n\n# QuickWay tool routing\nThis request's client executes its own tools on device {outpost_id} (QuickWay API session). Use these registered tools for client actions and return their results to the user. Use this device ID to disambiguate other sessions with the same display name. Client file paths are arguments to these tools; they are not Quick cloud filesystem paths. Do not substitute Quick's built-in file search, VFS, or another device for a client tool. Tool name mapping:\n"
        );
        for (alias, name) in &self.original_names {
            instructions.push_str(&format!(
                "- {name}: {} (wire name {alias})\n",
                model_tool_name(name)
            ));
        }
        instructions
    }

    pub fn decode_arguments(
        &self,
        cloud_name: &str,
        arguments: &Value,
        encoding: ArgumentEncoding,
    ) -> Result<DecodedToolCall, String> {
        let name = self
            .original_names
            .get(cloud_name)
            .ok_or("Quick cloud requested an unregistered client tool")?;
        let arguments_object = object(arguments, "cloud tool arguments")?;
        let input = match encoding {
            ArgumentEncoding::Direct => arguments.clone(),
            ArgumentEncoding::ParamsWrapper => {
                if arguments_object.len() != 1 {
                    return Err("Quick params wrapper contains unexpected fields".into());
                }
                let input = arguments_object
                    .get("params")
                    .ok_or("Quick params wrapper is missing params")?;
                let params = object(input, "Quick params wrapper value")?;
                if (params.contains_key("name") && params.contains_key("arguments"))
                    || params.get("_quickwork").is_some_and(|meta| {
                        meta.get("result_transport_version").is_some()
                            || meta.get("result_transport_token").is_some()
                    })
                {
                    // Current MCP dispatch includes its own protocol envelope.
                    // Its transport token and tracing fields are not tool input.
                    only(
                        params,
                        &["name", "arguments", "_quickwork"],
                        "Quick MCP dispatch envelope",
                    )?;
                    if params.get("name").and_then(Value::as_str) != Some(cloud_name)
                        || params.get("_quickwork").is_some_and(|meta| {
                            !meta.is_object()
                                || meta
                                    .get("result_transport_version")
                                    .is_some_and(|version| version.as_u64() != Some(1))
                        })
                    {
                        return Err("Quick MCP dispatch envelope does not match the registered tool or supported protocol".into());
                    }
                    let actual = params
                        .get("arguments")
                        .ok_or("Quick MCP dispatch has no tool arguments")?;
                    object(actual, "Quick MCP tool arguments")?;
                    actual.clone()
                } else {
                    input.clone()
                }
            }
        };
        Ok(DecodedToolCall {
            name: name.clone(),
            input,
        })
    }
}

fn system_text(value: Option<&Value>) -> Result<String, String> {
    match value {
        None => Ok(String::new()),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(value) => {
            let blocks = value
                .as_array()
                .ok_or("system must be text or text blocks")?;
            let mut texts = Vec::new();
            for block in blocks {
                let block = object(block, "system block")?;
                only(block, &["type", "text", "cache_control"], "system block")?;
                if block.get("type").and_then(Value::as_str) != Some("text") {
                    return Err("Quick cloud system blocks must contain text only".into());
                }
                cache_control(block.get("cache_control"))?;
                texts.push(text(
                    block.get("text").ok_or("system text is required")?,
                    "system text",
                )?);
            }
            Ok(texts.join("\n\n"))
        }
    }
}

fn validate_content(
    value: &Value,
    role: &str,
    outstanding: &mut BTreeSet<String>,
    seen: &mut BTreeSet<String>,
) -> Result<UserTurn, String> {
    if let Some(text) = value.as_str() {
        if text.trim().is_empty() {
            return Err("message.content must not be empty".into());
        }
        return Ok(UserTurn {
            text: text.into(),
            tool_results: Vec::new(),
        });
    }
    let blocks = value
        .as_array()
        .filter(|b| !b.is_empty())
        .ok_or("message.content must be text or a nonempty block array")?;
    let mut turn = UserTurn::default();
    let mut texts = Vec::new();
    for block in blocks {
        let block = object(block, "content block")?;
        cache_control(block.get("cache_control"))?;
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                only(block, &["type", "text", "cache_control"], "text block")?;
                texts.push(text(
                    block.get("text").ok_or("text block text is required")?,
                    "text block",
                )?);
            }
            Some("tool_use") if role == "assistant" => {
                only(
                    block,
                    &["type", "id", "name", "input", "cache_control"],
                    "tool_use",
                )?;
                let id = nonempty(
                    block.get("id").ok_or("tool_use.id is required")?,
                    "tool_use.id",
                )?;
                nonempty(
                    block.get("name").ok_or("tool_use.name is required")?,
                    "tool_use.name",
                )?;
                object(
                    block.get("input").ok_or("tool_use.input is required")?,
                    "tool_use.input",
                )?;
                if !seen.insert(id.into()) || !outstanding.insert(id.into()) {
                    return Err("Duplicate tool_use id in message history".into());
                }
            }
            Some("tool_result") if role == "user" => {
                only(
                    block,
                    &[
                        "type",
                        "tool_use_id",
                        "content",
                        "is_error",
                        "cache_control",
                    ],
                    "tool_result",
                )?;
                let id = nonempty(
                    block
                        .get("tool_use_id")
                        .ok_or("tool_result.tool_use_id is required")?,
                    "tool_result.tool_use_id",
                )?;
                if !outstanding.remove(id) {
                    return Err("Tool result has no matching unresolved tool call".into());
                }
                let content = result_content(block.get("content"))?;
                let is_error = block
                    .get("is_error")
                    .map(|v| boolean(v, "tool_result.is_error"))
                    .transpose()?
                    .unwrap_or(false);
                turn.tool_results.push(ToolResult {
                    tool_use_id: id.into(),
                    content,
                    is_error,
                });
            }
            Some("thinking") if role == "assistant" => {
                only(
                    block,
                    &["type", "thinking", "signature"],
                    "thinking history",
                )?;
                text(
                    block.get("thinking").ok_or("thinking text is required")?,
                    "thinking text",
                )?;
                if let Some(signature) = block.get("signature") {
                    text(signature, "thinking signature")?;
                }
            }
            Some("redacted_thinking") if role == "assistant" => {
                only(block, &["type", "data"], "redacted thinking history")?;
                nonempty(
                    block
                        .get("data")
                        .ok_or("redacted thinking data is required")?,
                    "redacted thinking data",
                )?;
            }
            Some("image" | "document") => {
                return Err("Quick cloud image and document inputs are not implemented".into());
            }
            _ => return Err("Unsupported Quick cloud content block or block role".into()),
        }
    }
    turn.text = texts.join("\n\n");
    if role == "user" && turn.text.trim().is_empty() && turn.tool_results.is_empty() {
        return Err("User message has no text or tool results".into());
    }
    Ok(turn)
}

fn result_content(value: Option<&Value>) -> Result<Vec<Value>, String> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::String(text)) => Ok(vec![json!({"type":"text","text":text})]),
        Some(value) => {
            let blocks = value
                .as_array()
                .ok_or("tool_result.content must be text or text blocks")?;
            let mut content = Vec::new();
            for block in blocks {
                let block = object(block, "tool result content")?;
                only(block, &["type", "text"], "tool result content")?;
                if block.get("type").and_then(Value::as_str) != Some("text") {
                    return Err("Quick cloud tool result images and other non-text content are not implemented".into());
                }
                content.push(json!({"type":"text","text":text(block.get("text").ok_or("tool result text is required")?, "tool result text")?}));
            }
            Ok(content)
        }
    }
}

fn cache_control(value: Option<&Value>) -> Result<(), String> {
    if let Some(value) = value {
        let cache = object(value, "cache_control")?;
        only(cache, &["type", "ttl"], "cache_control")?;
        if cache.get("type").and_then(Value::as_str) != Some("ephemeral") {
            return Err("Unsupported cache_control type".into());
        }
        if let Some(ttl) = cache.get("ttl")
            && !matches!(ttl.as_str(), Some("5m" | "1h"))
        {
            return Err("Unsupported cache_control ttl".into());
        }
    }
    Ok(())
}

fn object<'a>(value: &'a Value, field: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{field} must be an object"))
}
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{field} must be a string"))
}
fn nonempty<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    text(value, field).and_then(|text| {
        if text.is_empty() {
            Err(format!("{field} must not be empty"))
        } else {
            Ok(text)
        }
    })
}
fn boolean(value: &Value, field: &str) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("{field} must be a boolean"))
}
fn only(object: &Map<String, Value>, allowed: &[&str], field: &str) -> Result<(), String> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        Err(format!("Unsupported field in {field}"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Value {
        json!({"model":"quick-smart","max_tokens":100,"messages":[{"role":"user","content":"Hello"}]})
    }
    fn context() -> MessageContext<'static> {
        MessageContext {
            surface_type: "QUICK_WORK_DESKTOP_WINDOWS",
            outpost_id: "outpost",
            app_version: "1.2.4308",
            thread_id: "thread",
            message_id: "message",
            user_timezone: Some("Asia/Shanghai"),
            user_locale: Some("en-US"),
        }
    }

    #[test]
    fn preserves_history_and_emits_only_the_new_text_and_system() {
        let mut source = body();
        source["system"] = json!([{"type":"text","text":"First"},{"type":"text","text":"Second"}]);
        source["messages"] = json!([{"role":"user","content":"old"},{"role":"assistant","content":"answer"},{"role":"user","content":[{"type":"text","text":"New"},{"type":"text","text":"turn"}]}]);
        let request = ValidatedRequest::from_body(source.clone(), "smart", Some("high")).unwrap();
        assert_eq!(request.body, source);
        assert_eq!(request.requested_max_tokens, Some(100));
        assert!(request.notices.contains(&CLOUD_LIMIT_NOTICE));
        assert!(
            request
                .user_message_payload(&context(), SessionHistory::NewConversation)
                .is_err()
        );
        let payload = request
            .user_message_payload(&context(), SessionHistory::MatchedExistingSession)
            .unwrap();
        assert_eq!(payload["text"], "New\n\nturn");
        assert_eq!(
            payload["custom_prompt"],
            format!("First\n\nSecond{CLIENT_PRESENTATION}")
        );
        assert_eq!(payload["thinking_effort"], "high");
        assert_eq!(payload["surfaceType"], "QUICK_WORK_DESKTOP_WINDOWS");
        assert_eq!(payload["auto_routing"], false);
        assert_eq!(payload["web_search_enabled"], false);
        assert!(!payload.to_string().contains("answer"));
    }

    #[test]
    fn tools_have_stable_distinct_names_preserve_schemas_and_never_claim_read_safety() {
        let mut source = body();
        let schema = json!({"type":"object","properties":{"params":{"type":"object"}},"additionalProperties":false});
        source["tools"] = json!([{"name":"x-y","input_schema":schema},{"name":"x_y","input_schema":schema},{"name":"read_file","input_schema":schema}]);
        let request = ValidatedRequest::from_body(source.clone(), "smart", None).unwrap();
        let again = ValidatedRequest::from_body(source, "smart", None).unwrap();
        assert_eq!(request.tools.original_names, again.tools.original_names);
        let capability = request.tools.capability();
        assert_eq!(capability["name"], "mcp__quickway");
        assert_eq!(request.tools.original_names.len(), 3);
        for tool in capability["tools"].as_array().unwrap() {
            assert_eq!(tool["inputSchema"], schema);
            assert_eq!(tool["isReadTool"], false);
            assert!(
                tool["name"]
                    .as_str()
                    .unwrap()
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
            );
        }
        let alias = request
            .tools
            .original_names
            .iter()
            .find(|(_, original)| *original == "x-y")
            .unwrap()
            .0;
        let input = json!({"params":{"user_value":1}});
        let direct = request
            .tools
            .decode_arguments(alias, &input, ArgumentEncoding::Direct)
            .unwrap();
        assert_eq!(direct.name, "x-y");
        assert_eq!(direct.input, input);
        let wrapped = request
            .tools
            .decode_arguments(
                alias,
                &json!({"params":input}),
                ArgumentEncoding::ParamsWrapper,
            )
            .unwrap();
        assert_eq!(wrapped.input, input);
        assert!(
            request
                .tools
                .decode_arguments(
                    alias,
                    &json!({"params":{},"extra":1}),
                    ArgumentEncoding::ParamsWrapper
                )
                .is_err()
        );
        assert!(
            request
                .tools
                .decode_arguments("unknown", &json!({}), ArgumentEncoding::Direct)
                .is_err()
        );
    }

    #[test]
    fn pending_results_and_extra_user_text_are_preserved_until_the_session_consumes_them() {
        let mut source = body();
        source["messages"] = json!([
            {"role":"user","content":"read"},
            {"role":"assistant","content":[{"type":"tool_use","id":"call1","name":"read","input":{}},{"type":"tool_use","id":"call2","name":"read","input":{"file":"second"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call2","content":"second result"},{"type":"tool_result","tool_use_id":"call1","content":[{"type":"text","text":"failed"}],"is_error":true},{"type":"text","text":"Try another file"}]}
        ]);
        let request = ValidatedRequest::from_body(source, "smart", None).unwrap();
        assert_eq!(request.turn.text, "Try another file");
        assert_eq!(request.turn.tool_results.len(), 2);
        assert_eq!(request.turn.tool_results[0].tool_use_id, "call2");
        assert_eq!(request.turn.tool_results[1].tool_use_id, "call1");
        assert_eq!(
            request.turn.tool_results[1].content,
            json!([{"type":"text","text":"failed"}])
                .as_array()
                .unwrap()
                .clone()
        );
        assert!(!request.turn.tool_results[0].is_error);
        assert!(request.turn.tool_results[1].is_error);
        assert!(
            request
                .user_message_payload(&context(), SessionHistory::MatchedExistingSession)
                .is_err()
        );
    }

    #[test]
    fn refuses_unimplemented_semantics_and_malformed_history() {
        for (field, value) in [
            ("temperature", json!(0.5)),
            ("max_tokens", json!(0)),
            ("tool_choice", json!({"type":"any"})),
            ("output_config", json!({"format":{"type":"json_schema"}})),
            ("thinking", json!({"type":"enabled","budget_tokens":0})),
        ] {
            let mut source = body();
            source[field] = value;
            assert!(
                ValidatedRequest::from_body(source, "smart", None).is_err(),
                "{field}"
            );
        }
        for messages in [
            json!([]),
            json!([{"role":"user","content":[]}]),
            json!([{"role":"user","content":[{"type":"image","source":{}}]}]),
            json!([{"role":"user","content":[{"type":"tool_result","tool_use_id":"orphan","content":"x"}]}]),
            json!([{"role":"user","content":"x"},{"role":"assistant","content":"unfinished"}]),
        ] {
            let mut source = body();
            source["messages"] = messages;
            assert!(ValidatedRequest::from_body(source, "smart", None).is_err());
        }
        let mut source = body();
        source["tools"] =
            json!([{"name":"same","input_schema":{}},{"name":"same","input_schema":{}}]);
        assert!(ValidatedRequest::from_body(source, "smart", None).is_err());
    }

    #[test]
    fn accepts_thinking_budgets_as_disclosed_effort_hints() {
        for (budget, effort) in [(1024, "low"), (4096, "medium"), (10000, "high")] {
            let mut source = body();
            source["thinking"] = json!({"type":"enabled","budget_tokens":budget});
            let request = ValidatedRequest::from_body(source.clone(), "smart", None).unwrap();
            assert_eq!(request.thinking_effort, effort);
            assert_eq!(request.body, source);
            assert!(
                request
                    .notices
                    .iter()
                    .any(|n| n.contains("thinking.budget_tokens"))
            );
            let explicit = ValidatedRequest::from_body(source, "smart", Some("low")).unwrap();
            assert_eq!(explicit.thinking_effort, "low");
        }
    }

    #[test]
    fn long_tool_descriptions_fit_registration_without_losing_documentation() {
        let mut source = body();
        let long = format!("{}END_OF_FULL_DOCS", "工具说明🦀".repeat(1500));
        let schema = json!({"type":"object","properties":{"script":{"type":"string"}}});
        source["system"] = "Existing system instructions".into();
        source["tools"] = json!([
            {"name":"PowerShell","description":long,"input_schema":schema},
            {"name":"Read","description":"Short documentation","input_schema":{}}
        ]);
        let request = ValidatedRequest::from_body(source.clone(), "smart", None).unwrap();
        let capability = request.tools.capability();
        assert!(
            capability["tools"][0]["description"]
                .as_str()
                .unwrap()
                .len()
                <= MAX_TOOL_DESCRIPTION
        );
        assert_eq!(capability["tools"][0]["inputSchema"], schema);
        assert_eq!(
            capability["tools"][1]["description"],
            "Client tool: Read\nShort documentation"
        );
        assert!(
            request
                .custom_prompt
                .starts_with("Existing system instructions")
        );
        assert!(request.custom_prompt.contains(&long));
        assert!(
            request
                .custom_prompt
                .contains(&model_tool_name("PowerShell"))
        );
        assert_eq!(request.body, source);
        let payload = request
            .user_message_payload(&context(), SessionHistory::NewConversation)
            .unwrap();
        assert!(
            payload["custom_prompt"]
                .as_str()
                .unwrap()
                .contains("END_OF_FULL_DOCS")
        );
    }

    #[test]
    fn mcp_dispatch_unwraps_real_arguments_without_exposing_transport_metadata() {
        let mut source = body();
        source["tools"] = json!([{"name":"Read","input_schema":{"type":"object"}}]);
        let request = ValidatedRequest::from_body(source, "smart", None).unwrap();
        let alias = tool_alias("Read");
        let input = json!({"file_path":"fixture.txt","params":{"nested":"preserved"}});
        let mut envelope = json!({"params":{"name":alias,"arguments":input,"_quickwork":{
            "result_transport_version":1,"result_transport_token":"synthetic-internal-token","tool_use_id":"synthetic-native-id"
        }}});
        let call = request
            .tools
            .decode_arguments(&alias, &envelope, ArgumentEncoding::ParamsWrapper)
            .unwrap();
        assert_eq!(call.name, "Read");
        assert_eq!(call.input, input);
        assert!(!call.input.to_string().contains("synthetic-internal-token"));
        let without_metadata = json!({"params":{"name":alias,"arguments":input}});
        assert_eq!(
            request
                .tools
                .decode_arguments(&alias, &without_metadata, ArgumentEncoding::ParamsWrapper)
                .unwrap()
                .input,
            input
        );
        envelope["params"]["name"] = "another-tool".into();
        assert!(
            request
                .tools
                .decode_arguments(&alias, &envelope, ArgumentEncoding::ParamsWrapper)
                .is_err()
        );
        envelope["params"]["name"] = alias.clone().into();
        envelope["params"]["_quickwork"]["result_transport_version"] = 2.into();
        assert!(
            request
                .tools
                .decode_arguments(&alias, &envelope, ArgumentEncoding::ParamsWrapper)
                .is_err()
        );
    }
}
