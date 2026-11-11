//! Anthropic Messages <-> standard Amazon Bedrock Converse JSON conversion.
//!
//! Model alias resolution and transport selection belong to the caller. The
//! `model_id` supplied here is passed unchanged as Converse's `modelId`.
//! Shapes follow botocore's bedrock-runtime/2023-09-30 service model. This
//! module neither synthesizes turns nor edits system prompts or tool schemas.

use std::collections::HashSet;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};

type Object = Map<String, Value>;

/// Convert a complete Anthropic request. Unsupported behavior is an error.
/// `model`, `stream`, and client `metadata` are deliberately not forwarded;
/// model mapping/streaming are caller responsibilities, and user_id is local.
pub fn request(body: Value, model_id: &str) -> Result<Value, String> {
    if model_id.is_empty() {
        return Err("model_id must not be empty".into());
    }
    let body = object(&body, "request")?;
    only(
        body,
        &[
            "model",
            "stream",
            "metadata",
            "max_tokens",
            "messages",
            "system",
            "tools",
            "tool_choice",
            "temperature",
            "top_p",
            "top_k",
            "stop_sequences",
            "thinking",
            "output_config",
        ],
        "request",
    )?;
    if let Some(model) = present(body, "model") {
        nonempty(model, "model")?;
    }
    if let Some(stream) = present(body, "stream") {
        boolean(stream, "stream")?;
    }
    if let Some(metadata) = present(body, "metadata") {
        object(metadata, "metadata")?;
    }

    let max_tokens = positive_integer(required(body, "max_tokens")?, "max_tokens")?;
    let mut inference = json!({"maxTokens":max_tokens});
    for (source, target) in [("temperature", "temperature"), ("top_p", "topP")] {
        if let Some(value) = present(body, source) {
            let number = value
                .as_f64()
                .ok_or_else(|| format!("{source} must be a number"))?;
            if !(0.0..=1.0).contains(&number) {
                return Err(format!("{source} must be between 0 and 1"));
            }
            inference[target] = value.clone();
        }
    }
    if let Some(sequences) = present(body, "stop_sequences") {
        let sequences = array(sequences, "stop_sequences")?;
        if sequences.len() > 2500 {
            return Err("stop_sequences exceeds the Converse limit of 2500".into());
        }
        for sequence in sequences {
            nonempty(sequence, "stop_sequences entry")?;
        }
        inference["stopSequences"] = json!(sequences);
    }

    let messages = array(required(body, "messages")?, "messages")?;
    if messages.is_empty() {
        return Err("messages must not be empty".into());
    }
    let mut converted_messages = Vec::new();
    let mut pending_tools = HashSet::new();
    let mut seen_tools = HashSet::new();
    for message in messages {
        let message = object(message, "message")?;
        only(message, &["role", "content"], "message")?;
        let role = text(required(message, "role")?, "message.role")?;
        if role != "user" && role != "assistant" {
            return Err("message.role must be user or assistant".into());
        }
        if role == "assistant" && !pending_tools.is_empty() {
            return Err(
                "An assistant turn begins before outstanding tool results were supplied".into(),
            );
        }
        let mut content = content_blocks(required(message, "content")?, role)?;
        if crate::models::is_gpt(model_id) {
            // Quick's GPT route rejects a cache hint immediately after a tool
            // result. Remove only that optimization hint; retain the result.
            let mut after_result = false;
            content.retain(|block| {
                if after_result && block.get("cachePoint").is_some() {
                    return false;
                }
                after_result = block.get("toolResult").is_some();
                true
            });
        }
        for block in &content {
            if let Some(call) = block.get("toolUse") {
                let id = call["toolUseId"].as_str().unwrap().to_owned();
                if !seen_tools.insert(id.clone()) {
                    return Err("Duplicate tool_use id in conversation history".into());
                }
                pending_tools.insert(id);
            }
            if let Some(result) = block.get("toolResult") {
                let id = result["toolUseId"].as_str().unwrap();
                if !pending_tools.remove(id) {
                    return Err("tool_result has no outstanding tool_use with the same id".into());
                }
            }
        }
        converted_messages.push(json!({"role":role, "content":content}));
    }
    if !pending_tools.is_empty() {
        return Err("Conversation contains tool_use blocks without tool_result blocks".into());
    }

    let mut result = json!({
        "modelId":model_id, "messages":converted_messages, "inferenceConfig":inference,
        "additionalModelResponseFieldPaths":["/stop_sequence"]
    });
    if let Some(system) = present(body, "system") {
        let system = if system.is_string() {
            vec![json!({"text":nonempty(system, "system")?})]
        } else {
            let mut blocks = Vec::new();
            for block in array(system, "system")? {
                let block = object(block, "system block")?;
                only(block, &["type", "text", "cache_control"], "system block")?;
                if text(required(block, "type")?, "system block type")? != "text" {
                    return Err("Only text system blocks have a Converse equivalent".into());
                }
                blocks.push(json!({"text":nonempty(required(block, "text")?, "system text")?}));
                append_cache(block, &mut blocks)?;
            }
            blocks
        };
        if !system.is_empty() {
            result["system"] = json!(system);
        }
    }

    let mut tool_names = HashSet::new();
    if let Some(tools) = present(body, "tools") {
        let mut converted_tools = Vec::new();
        for tool in array(tools, "tools")? {
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
            if let Some(kind) = present(tool, "type")
                && text(kind, "tool.type")? != "custom"
            {
                return Err("Server tools and tool search are not supported by the standard Converse adapter".into());
            }
            if let Some(defer) = present(tool, "defer_loading")
                && boolean(defer, "tool.defer_loading")?
            {
                return Err(
                    "Deferred tool loading requires tool search, which is not supported".into(),
                );
            }
            let name = tool_name(required(tool, "name")?)?;
            if !tool_names.insert(name.to_owned()) {
                return Err("Duplicate tool definition name".into());
            }
            let schema = required(tool, "input_schema")?;
            object(schema, "tool.input_schema")?;
            let mut spec = json!({"name":name,"inputSchema":{"json":schema}});
            if let Some(description) = present(tool, "description") {
                let description = text(description, "tool.description")?;
                // Bedrock description is NonEmptyString; omission has the same
                // meaning as an empty Anthropic tool description.
                if !description.is_empty() {
                    spec["description"] = json!(description);
                }
            }
            if let Some(strict) = present(tool, "strict") {
                spec["strict"] = json!(boolean(strict, "tool.strict")?);
            }
            converted_tools.push(json!({"toolSpec":spec}));
            append_cache(tool, &mut converted_tools)?;
        }
        if !converted_tools.is_empty() {
            result["toolConfig"] = json!({"tools":converted_tools});
        }
    }
    if let Some(choice) = present(body, "tool_choice") {
        let choice = object(choice, "tool_choice")?;
        only(
            choice,
            &["type", "name", "disable_parallel_tool_use"],
            "tool_choice",
        )?;
        if let Some(parallel) = present(choice, "disable_parallel_tool_use")
            && boolean(parallel, "disable_parallel_tool_use")?
        {
            return Err("Standard Converse cannot enforce disable_parallel_tool_use=true".into());
        }
        let kind = text(required(choice, "type")?, "tool_choice.type")?;
        if kind == "none" {
            return Err("Standard Converse has no tool_choice=none equivalent".into());
        }
        if tool_names.is_empty() {
            return Err("tool_choice requires at least one tool definition".into());
        }
        result["toolConfig"]["toolChoice"] = match kind {
            "auto" | "any" => {
                if present(choice, "name").is_some() {
                    return Err("tool_choice.name is only valid with type=tool".into());
                }
                let mut union = Map::new();
                union.insert(kind.to_owned(), json!({}));
                Value::Object(union)
            }
            "tool" => {
                let name = tool_name(required(choice, "name")?)?;
                if !tool_names.contains(name) {
                    return Err("tool_choice names a tool absent from tools".into());
                }
                json!({"tool":{"name":name}})
            }
            _ => return Err("Unsupported tool_choice.type".into()),
        };
    }

    let mut additional = Map::new();
    if let Some(top_k) = present(body, "top_k") {
        let value = top_k
            .as_u64()
            .filter(|v| *v <= i32::MAX as u64)
            .ok_or("top_k must be a nonnegative 32-bit integer")?;
        additional.insert("top_k".into(), json!(value));
    }
    if let Some(thinking) = present(body, "thinking") {
        let thinking_object = object(thinking, "thinking")?;
        only(
            thinking_object,
            &["type", "budget_tokens", "display"],
            "thinking",
        )?;
        let kind = text(required(thinking_object, "type")?, "thinking.type")?;
        match kind {
            "enabled" => {
                let budget = positive_integer(
                    required(thinking_object, "budget_tokens")?,
                    "thinking.budget_tokens",
                )?;
                if budget < 1024 || budget >= max_tokens {
                    return Err(
                        "thinking.budget_tokens must be at least 1024 and less than max_tokens"
                            .into(),
                    );
                }
            }
            "between_tools" if !model_id.contains("claude-sonnet-5-5") => {
                return Err("thinking.type=between_tools is only supported for Sonnet 5.5".into());
            }
            "adaptive" | "disabled" | "between_tools" => {
                if present(thinking_object, "budget_tokens").is_some() {
                    return Err("budget_tokens is only valid with thinking.type=enabled".into());
                }
            }
            _ => return Err("Unsupported thinking.type".into()),
        }
        if let Some(display) = present(thinking_object, "display")
            && !matches!(text(display, "thinking.display")?, "summarized" | "omitted")
        {
            return Err("thinking.display must be summarized or omitted".into());
        }
        if crate::models::is_gpt(model_id) {
            if kind == "enabled" {
                return Err("GPT does not support Anthropic fixed thinking budgets. Use adaptive thinking with output_config.effort, or disabled thinking.".into());
            }
            // GPT takes its effort as `reasoning` (below). Forwarding Claude's
            // additionalModelRequestFields.thinking is rejected by Quick.
        } else {
            additional.insert("thinking".into(), thinking.clone());
        }
    }
    // Quick's Converse route ignores the standard top-level `outputConfig`
    // (verified 2026-11-11: an invalid effort or a JSON schema there has no
    // effect). Each model reads its own native fields through
    // additionalModelRequestFields instead: Claude `output_config`, GPT
    // `reasoning` and `text.format`.
    let gpt = crate::models::is_gpt(model_id);
    let mut effort = None;
    let mut schema = None;
    if let Some(config) = present(body, "output_config") {
        let config = object(config, "output_config")?;
        only(config, &["effort", "format"], "output_config")?;
        if let Some(value) = present(config, "effort") {
            let value = text(value, "output_config.effort")?;
            if !(matches!(value, "low" | "medium" | "high" | "xhigh" | "max")
                || value == "none" && gpt)
            {
                return Err("Unsupported output_config.effort".into());
            }
            effort = Some(value);
        }
        if let Some(format) = present(config, "format") {
            let format = object(format, "output_config.format")?;
            only(format, &["type", "schema"], "output_config.format")?;
            if text(required(format, "type")?, "output_config.format.type")? != "json_schema" {
                return Err("Only output_config.format.type=json_schema is supported".into());
            }
            let value = required(format, "schema")?;
            object(value, "output_config.format.schema")?;
            schema = Some(value);
        }
    }
    if gpt {
        if effort.is_none() && present(body, "thinking").is_some() {
            effort = Some(if body["thinking"]["type"] == "disabled" {
                "none"
            } else {
                "high"
            });
        }
        if let Some(effort) = effort {
            additional.insert("reasoning".into(), json!({"effort":effort}));
        }
        if let Some(schema) = schema {
            additional.insert(
                "text".into(),
                json!({"format":{"type":"json_schema","name":"response","schema":schema,"strict":true}}),
            );
        }
    } else {
        let mut output = Map::new();
        if let Some(effort) = effort {
            output.insert("effort".into(), json!(effort));
        }
        if let Some(schema) = schema {
            output.insert(
                "format".into(),
                json!({"type":"json_schema","schema":schema}),
            );
        }
        if !output.is_empty() {
            additional.insert("output_config".into(), Value::Object(output));
        }
    }
    if !additional.is_empty() {
        result["additionalModelRequestFields"] = Value::Object(additional);
    }
    Ok(result)
}

fn content_blocks(value: &Value, role: &str) -> Result<Vec<Value>, String> {
    if let Some(text) = value.as_str() {
        return Ok(vec![json!({"text":text})]);
    }
    let blocks = array(value, "message.content")?;
    if blocks.is_empty() {
        return Err("message.content must not be an empty array".into());
    }
    let mut result = Vec::new();
    for block in blocks {
        let block = object(block, "content block")?;
        let kind = text(required(block, "type")?, "content block type")?;
        let converted = match kind {
            "text" => {
                only(
                    block,
                    &["type", "text", "cache_control", "citations"],
                    "text block",
                )?;
                if let Some(citations) = present(block, "citations")
                    && !array(citations, "text.citations")?.is_empty()
                {
                    return Err(
                        "Anthropic text citations cannot be replayed by this Converse adapter"
                            .into(),
                    );
                }
                json!({"text":text(required(block, "text")?, "text block text")?})
            }
            "image" => {
                if role != "user" {
                    return Err("Converse permits input images only in user messages".into());
                }
                only(block, &["type", "source", "cache_control"], "image block")?;
                json!({"image":image(required(block, "source")?)?})
            }
            "tool_use" => {
                if role != "assistant" {
                    return Err("tool_use blocks require role=assistant".into());
                }
                only(
                    block,
                    &["type", "id", "name", "input", "cache_control", "caller"],
                    "tool_use block",
                )?;
                if let Some(caller) = present(block, "caller") {
                    let caller = object(caller, "tool_use.caller")?;
                    only(caller, &["type"], "tool_use.caller")?;
                    if text(required(caller, "type")?, "tool_use.caller.type")? != "direct" {
                        return Err(
                            "Non-direct tool callers cannot be replayed through Converse".into(),
                        );
                    }
                }
                let input = required(block, "input")?;
                object(input, "tool_use.input")?;
                json!({"toolUse":{
                    "toolUseId":tool_id(required(block, "id")?)?,
                    "name":tool_name(required(block, "name")?)?, "input":input
                }})
            }
            "tool_result" => {
                if role != "user" {
                    return Err("tool_result blocks require role=user".into());
                }
                only(
                    block,
                    &[
                        "type",
                        "tool_use_id",
                        "content",
                        "is_error",
                        "cache_control",
                    ],
                    "tool_result block",
                )?;
                let is_error = present(block, "is_error")
                    .map(|v| boolean(v, "tool_result.is_error"))
                    .transpose()?
                    .unwrap_or(false);
                let content = tool_result_content(present(block, "content"))?;
                json!({"toolResult":{
                    "toolUseId":tool_id(required(block, "tool_use_id")?)?,
                    "content":content,"status":if is_error {"error"} else {"success"}
                }})
            }
            "thinking" => {
                if role != "assistant" {
                    return Err("thinking blocks require role=assistant".into());
                }
                only(block, &["type", "thinking", "signature"], "thinking block")?;
                json!({"reasoningContent":{"reasoningText":{
                    "text":text(required(block, "thinking")?, "thinking text")?,
                    "signature":nonempty(required(block, "signature")?, "thinking signature")?
                }}})
            }
            "redacted_thinking" => {
                if role != "assistant" {
                    return Err("redacted_thinking blocks require role=assistant".into());
                }
                only(block, &["type", "data"], "redacted_thinking block")?;
                let data = nonempty(required(block, "data")?, "redacted_thinking.data")?;
                STANDARD
                    .decode(data)
                    .map_err(|_| "redacted_thinking.data must be valid base64")?;
                json!({"reasoningContent":{"redactedContent":data}})
            }
            _ => return Err(format!("Unsupported Anthropic content block type: {kind}")),
        };
        result.push(converted);
        append_cache(block, &mut result)?;
    }
    Ok(result)
}

fn image(source: &Value) -> Result<Value, String> {
    let source = object(source, "image.source")?;
    only(source, &["type", "media_type", "data"], "image.source")?;
    if text(required(source, "type")?, "image.source.type")? != "base64" {
        return Err("Only base64 image sources are supported; remote URLs are not fetched".into());
    }
    let format = match text(required(source, "media_type")?, "image media_type")? {
        "image/jpeg" => "jpeg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return Err("Unsupported image media_type".into()),
    };
    let data = nonempty(required(source, "data")?, "image data")?;
    let decoded = STANDARD
        .decode(data)
        .map_err(|_| "Image data is not valid base64")?;
    if decoded.is_empty() || decoded.len() > 3_932_160 {
        return Err("Converse images must contain between 1 byte and 3.75 MiB".into());
    }
    Ok(json!({"format":format,"source":{"bytes":data}}))
}

fn tool_result_content(content: Option<&Value>) -> Result<Vec<Value>, String> {
    let Some(content) = content else {
        return Ok(vec![json!({"text":""})]);
    };
    if let Some(text) = content.as_str() {
        return Ok(vec![json!({"text":text})]);
    }
    let content = array(content, "tool_result.content")?;
    if content.is_empty() {
        return Ok(vec![json!({"text":""})]);
    }
    let mut result = Vec::new();
    for block in content {
        let block = object(block, "tool result content block")?;
        match text(required(block, "type")?, "tool result block type")? {
            "text" => {
                only(block, &["type", "text"], "tool result text block")?;
                result.push(json!({"text":text(required(block, "text")?, "tool result text")?}));
            }
            "image" => {
                only(block, &["type", "source"], "tool result image block")?;
                result.push(json!({"image":image(required(block, "source")?)?}));
            }
            _ => return Err("Unsupported tool_result content block type".into()),
        }
    }
    Ok(result)
}

fn append_cache(block: &Object, output: &mut Vec<Value>) -> Result<(), String> {
    if let Some(cache) = present(block, "cache_control") {
        let cache = object(cache, "cache_control")?;
        only(cache, &["type", "ttl"], "cache_control")?;
        if text(required(cache, "type")?, "cache_control.type")? != "ephemeral" {
            return Err("Only cache_control.type=ephemeral is supported".into());
        }
        let mut point = json!({"type":"default"});
        if let Some(ttl) = present(cache, "ttl") {
            let ttl = text(ttl, "cache_control.ttl")?;
            if ttl != "5m" && ttl != "1h" {
                return Err("cache_control.ttl must be 5m or 1h".into());
            }
            point["ttl"] = json!(ttl);
        }
        output.push(json!({"cachePoint":point}));
    }
    Ok(())
}

/// Convert a complete standard Converse response, preserving signed reasoning
/// and authoritative token counters. Missing usage or malformed output errors
/// are not replaced with invented successful responses.
pub fn response(body: Value, model: &str) -> Result<Value, String> {
    let root = object(&body, "Converse response")?;
    let message = body
        .pointer("/output/message")
        .ok_or("Converse response has no output.message")?;
    let message = object(message, "Converse output.message")?;
    if text(required(message, "role")?, "Converse response role")? != "assistant" {
        return Err("Converse response role must be assistant".into());
    }
    let mut content = Vec::new();
    let mut ids = HashSet::new();
    for block in array(required(message, "content")?, "Converse output content")? {
        let block = object(block, "Converse content block")?;
        if block.len() != 1 {
            return Err("Converse content block must contain exactly one union member".into());
        }
        let (kind, value) = block.iter().next().unwrap();
        match kind.as_str() {
            "text" => content.push(json!({"type":"text","text":text(value, "Converse text")?})),
            "toolUse" => {
                let call = object(value, "Converse toolUse")?;
                only(
                    call,
                    &["toolUseId", "name", "input", "type"],
                    "Converse toolUse",
                )?;
                if let Some(kind) = present(call, "type")
                    && text(kind, "Converse toolUse.type")? != "tool_use"
                {
                    return Err("Unsupported Converse toolUse.type".into());
                }
                let id = tool_id(required(call, "toolUseId")?)?;
                if !ids.insert(id.to_owned()) {
                    return Err("Duplicate tool_use id in Converse response".into());
                }
                let input = required(call, "input")?;
                object(input, "Converse toolUse.input")?;
                content.push(json!({"type":"tool_use","id":id,"name":tool_name(required(call, "name")?)?,"input":input}));
            }
            "reasoningContent" => {
                let reasoning = object(value, "reasoningContent")?;
                if reasoning.len() != 1 {
                    return Err("reasoningContent must contain exactly one union member".into());
                }
                if let Some(reasoning_text) = reasoning.get("reasoningText") {
                    let reasoning_text = object(reasoning_text, "reasoningText")?;
                    only(reasoning_text, &["text", "signature"], "reasoningText")?;
                    // A signed block must remain replayable. Fabricating or
                    // dropping its signature would break the following turn.
                    content.push(json!({
                        "type":"thinking","thinking":text(required(reasoning_text, "text")?, "reasoning text")?,
                        "signature":nonempty(required(reasoning_text, "signature")?, "reasoning signature")?
                    }));
                } else if let Some(redacted) = reasoning.get("redactedContent") {
                    let data = nonempty(redacted, "redactedContent")?;
                    STANDARD
                        .decode(data)
                        .map_err(|_| "redactedContent is not valid base64")?;
                    content.push(json!({"type":"redacted_thinking","data":data}));
                } else {
                    return Err("Unsupported reasoningContent variant".into());
                }
            }
            _ => {
                return Err(format!(
                    "Unsupported Converse response content block: {kind}"
                ));
            }
        }
    }
    let stop = text(required(root, "stopReason")?, "Converse stopReason")?;
    let stop = match stop {
        "end_turn"
        | "tool_use"
        | "max_tokens"
        | "stop_sequence"
        | "model_context_window_exceeded" => stop,
        "guardrail_intervened" | "content_filtered" => "refusal",
        "malformed_model_output" | "malformed_tool_use" => {
            return Err(format!("Bedrock rejected model output: {stop}"));
        }
        _ => return Err(format!("Unsupported Converse stopReason: {stop}")),
    };
    if stop == "tool_use" && ids.is_empty() {
        return Err("Converse stopReason=tool_use has no complete tool call".into());
    }
    let usage = object(required(root, "usage")?, "Converse usage")?;
    let mut translated_usage = json!({
        "input_tokens":counter(required(usage, "inputTokens")?, "inputTokens")?,
        "output_tokens":counter(required(usage, "outputTokens")?, "outputTokens")?
    });
    for (source, target) in [
        ("cacheReadInputTokens", "cache_read_input_tokens"),
        ("cacheWriteInputTokens", "cache_creation_input_tokens"),
    ] {
        if let Some(value) = present(usage, source) {
            translated_usage[target] = json!(counter(value, source)?);
        }
    }
    if let Some(details) = present(usage, "cacheDetails") {
        let mut five_minutes = 0u64;
        let mut hour = 0u64;
        for detail in array(details, "cacheDetails")? {
            let detail = object(detail, "cacheDetails entry")?;
            let tokens = counter(required(detail, "inputTokens")?, "cacheDetails.inputTokens")?;
            let target = match text(required(detail, "ttl")?, "cacheDetails.ttl")? {
                "5m" => &mut five_minutes,
                "1h" => &mut hour,
                _ => return Err("Unsupported cacheDetails TTL".into()),
            };
            *target = target
                .checked_add(tokens)
                .ok_or("cacheDetails token count overflow")?;
        }
        translated_usage["cache_creation"] = json!({
            "ephemeral_5m_input_tokens":five_minutes,"ephemeral_1h_input_tokens":hour
        });
    }
    let stop_sequence = match body.pointer("/additionalModelResponseFields/stop_sequence") {
        Some(value) if !value.is_null() => json!(text(value, "stop_sequence")?),
        _ => Value::Null,
    };
    Ok(json!({
        "id":format!("msg_{}", uuid::Uuid::new_v4().simple()),
        "type":"message","role":"assistant","model":model,"content":content,
        "stop_reason":stop,"stop_sequence":stop_sequence,"usage":translated_usage
    }))
}

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
fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{field} must be a string"))
}
fn nonempty<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    let value = text(value, field)?;
    if value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(value)
}
fn boolean(value: &Value, field: &str) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("{field} must be a boolean"))
}
fn present<'a>(object: &'a Object, field: &str) -> Option<&'a Value> {
    object.get(field).filter(|v| !v.is_null())
}
fn required<'a>(object: &'a Object, field: &str) -> Result<&'a Value, String> {
    present(object, field).ok_or_else(|| format!("Missing required field: {field}"))
}
fn only(object: &Object, allowed: &[&str], field: &str) -> Result<(), String> {
    for (key, value) in object {
        if !allowed.contains(&key.as_str()) && !value.is_null() {
            return Err(format!("Unsupported {field} field: {key}"));
        }
    }
    Ok(())
}
fn positive_integer(value: &Value, field: &str) -> Result<u64, String> {
    value
        .as_u64()
        .filter(|n| *n > 0 && *n <= i32::MAX as u64)
        .ok_or_else(|| format!("{field} must be a positive 32-bit integer"))
}
fn counter(value: &Value, field: &str) -> Result<u64, String> {
    value
        .as_u64()
        .ok_or_else(|| format!("{field} must be a nonnegative integer"))
}
fn tool_name(value: &Value) -> Result<&str, String> {
    identifier(value, "tool name", false)
}
fn tool_id(value: &Value) -> Result<&str, String> {
    identifier(value, "tool use id", true)
}
fn identifier<'a>(value: &'a Value, field: &str, extended: bool) -> Result<&'a str, String> {
    let value = nonempty(value, field)?;
    if value.len() > 64
        || !value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || b == b'_'
                || b == b'-'
                || (extended && matches!(b, b'.' | b':'))
        })
    {
        return Err(format!(
            "{field} does not satisfy Converse's 64-character identifier constraint"
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic() -> Value {
        json!({"model":"claude-test","max_tokens":4096,"messages":[{"role":"user","content":"你好 🦀"}]})
    }
    fn tool() -> Value {
        json!({"name":"Read","description":"Read a file","input_schema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}})
    }
    fn upstream(content: Value, stop: &str) -> Value {
        json!({"output":{"message":{"role":"assistant","content":content}},"stopReason":stop,"usage":{"inputTokens":23,"outputTokens":9,"totalTokens":32},"metrics":{"latencyMs":1}})
    }

    #[test]
    fn gpt_adaptive_thinking_uses_converse_effort_without_claude_fields() {
        let mut input = basic();
        input["thinking"] = json!({"type":"adaptive"});
        input["output_config"] = json!({"effort":"medium"});
        let converted = request(input.clone(), "us.openai.gpt-6-astra").unwrap();
        assert_eq!(
            converted["additionalModelRequestFields"],
            json!({"reasoning":{"effort":"medium"}})
        );
        assert!(converted.get("outputConfig").is_none());
        input.as_object_mut().unwrap().remove("output_config");
        let converted = request(input.clone(), "us.openai.gpt-5.6-sol").unwrap();
        assert_eq!(
            converted["additionalModelRequestFields"]["reasoning"]["effort"],
            "high"
        );
        let claude = request(input.clone(), "global.anthropic.claude-opus-4-8").unwrap();
        assert_eq!(
            claude["additionalModelRequestFields"]["thinking"]["type"],
            "adaptive"
        );
        input["thinking"] = json!({"type":"enabled","budget_tokens":1024});
        assert!(
            request(input, "us.openai.gpt-6-astra")
                .unwrap_err()
                .contains("fixed")
        );
    }

    #[test]
    fn gpt_tool_result_keeps_content_without_unsupported_cache_hint() {
        let mut input = basic();
        input["messages"] = json!([
            {"role":"user","content":"Read the fixture"},
            {"role":"assistant","content":[{"type":"tool_use","id":"call_test","name":"Read","input":{"path":"fixture.txt"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_test","content":"fixture contents","cache_control":{"type":"ephemeral"}}]}
        ]);
        let gpt = request(input.clone(), "us.openai.gpt-6-astra").unwrap();
        let claude = request(input, "global.anthropic.claude-opus-4-8").unwrap();
        let result = &gpt["messages"][2]["content"];
        assert_eq!(result.as_array().unwrap().len(), 1);
        assert_eq!(
            result[0]["toolResult"]["content"][0]["text"],
            "fixture contents"
        );
        assert_eq!(
            claude["messages"][2]["content"].as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn preserves_system_text_schema_cache_parameters_and_client_metadata_boundary() {
        let mut input = basic();
        input["metadata"] = json!({"user_id":"private-session"});
        input["stream"] = json!(true);
        input["system"] = json!([
            {"type":"text","text":"Do not modify this prompt.\n","cache_control":{"type":"ephemeral","ttl":"1h"}},
            {"type":"text","text":"Second instruction"}
        ]);
        let schema = json!({"type":"object","properties":{"files":{"type":"array","items":{"$ref":"#/$defs/path"}},"mode":{"anyOf":[{"const":"read"},{"const":"write"}]}},"required":["files"],"additionalProperties":false,"$defs":{"path":{"type":"string","minLength":1}}});
        input["tools"] = json!([{"type":"custom","name":"mcp__filesystem__read","description":"Original\ndescription","input_schema":schema,"strict":true,"defer_loading":false,"cache_control":{"type":"ephemeral"}}]);
        input["temperature"] = json!(0.4);
        input["top_p"] = json!(0.8);
        input["top_k"] = json!(42);
        input["stop_sequences"] = json!(["STOP", "结束"]);
        let result = request(input, "us.anthropic.actual-model").unwrap();
        assert_eq!(result["modelId"], "us.anthropic.actual-model");
        assert_eq!(result["messages"][0]["content"][0]["text"], "你好 🦀");
        assert_eq!(result["system"][0]["text"], "Do not modify this prompt.\n");
        assert_eq!(
            result["system"][1],
            json!({"cachePoint":{"type":"default","ttl":"1h"}})
        );
        assert_eq!(
            result["toolConfig"]["tools"][0]["toolSpec"]["inputSchema"]["json"],
            schema
        );
        assert_eq!(result["toolConfig"]["tools"][0]["toolSpec"]["strict"], true);
        assert_eq!(
            result["inferenceConfig"],
            json!({"maxTokens":4096,"temperature":0.4,"topP":0.8,"stopSequences":["STOP","结束"]})
        );
        assert_eq!(result["additionalModelRequestFields"]["top_k"], 42);
        assert!(result.get("metadata").is_none());
        assert!(result.get("stream").is_none());
    }

    #[test]
    fn tools_round_trip_with_error_results_images_and_multiturn_content() {
        let assistant = response(
            upstream(
                json!([
                    {"text":"Reading now"},
                    {"toolUse":{"toolUseId":"toolu_1","name":"Read","input":{"path":"说明.txt"}}}
                ]),
                "tool_use",
            ),
            "claude-test",
        )
        .unwrap();
        assert_eq!(assistant["stop_reason"], "tool_use");
        let mut input = basic();
        input["tools"] = json!([tool()]);
        input["messages"].as_array_mut().unwrap().extend([
            json!({"role":"assistant","content":assistant["content"]}),
            json!({"role":"user","content":[
                {"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":[
                    {"type":"text","text":"Permission denied"},
                    {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}
                ],"cache_control":{"type":"ephemeral","ttl":"5m"}},
                {"type":"text","text":"Try a different file"}
            ]})
        ]);
        let result = request(input, "actual-model").unwrap();
        assert_eq!(
            result["messages"][1]["content"][1]["toolUse"]["input"]["path"],
            "说明.txt"
        );
        let tool_result = &result["messages"][2]["content"][0]["toolResult"];
        assert_eq!(tool_result["status"], "error");
        assert_eq!(
            tool_result["content"][1],
            json!({"image":{"format":"png","source":{"bytes":"aGVsbG8="}}})
        );
        assert_eq!(
            result["messages"][2]["content"][2]["text"],
            "Try a different file"
        );
    }

    #[test]
    fn preserves_signed_and_redacted_reasoning_across_response_and_request() {
        let answer = response(upstream(json!([
            {"reasoningContent":{"reasoningText":{"text":"保留原文\n","signature":"opaque+/signature=="}}},
            {"reasoningContent":{"redactedContent":"b3BhcXVl"}},
            {"text":"Answer"}
        ]), "end_turn"), "claude-test").unwrap();
        let mut input = basic();
        input["thinking"] = json!({"type":"enabled","budget_tokens":1024});
        input["messages"].as_array_mut().unwrap().extend([
            json!({"role":"assistant","content":answer["content"]}),
            json!({"role":"user","content":"Continue"}),
        ]);
        let converted = request(input, "actual-model").unwrap();
        assert_eq!(
            converted["messages"][1]["content"][0]["reasoningContent"]["reasoningText"],
            json!({"text":"保留原文\n","signature":"opaque+/signature=="})
        );
        assert_eq!(
            converted["messages"][1]["content"][1]["reasoningContent"]["redactedContent"],
            "b3BhcXVl"
        );
        assert_eq!(
            converted["additionalModelRequestFields"]["thinking"],
            json!({"type":"enabled","budget_tokens":1024})
        );
    }

    #[test]
    fn maps_tool_choices_and_rejects_unenforceable_choices() {
        for (choice, expected) in [
            (
                json!({"type":"auto","disable_parallel_tool_use":false}),
                json!({"auto":{}}),
            ),
            (json!({"type":"any"}), json!({"any":{}})),
            (
                json!({"type":"tool","name":"Read"}),
                json!({"tool":{"name":"Read"}}),
            ),
        ] {
            let mut input = basic();
            input["tools"] = json!([tool()]);
            input["tool_choice"] = choice;
            assert_eq!(
                request(input, "m").unwrap()["toolConfig"]["toolChoice"],
                expected
            );
        }
        for choice in [
            json!({"type":"none"}),
            json!({"type":"tool","name":"Missing"}),
            json!({"type":"auto","disable_parallel_tool_use":true}),
        ] {
            let mut input = basic();
            input["tools"] = json!([tool()]);
            input["tool_choice"] = choice;
            assert!(request(input, "m").is_err());
        }
    }

    #[test]
    fn maps_structured_output_and_adaptive_effort() {
        let mut input = basic();
        input["thinking"] = json!({"type":"adaptive"});
        let schema = json!({"type":"object","properties":{"ok":{"type":"boolean"}},"required":["ok"],"additionalProperties":false});
        input["output_config"] =
            json!({"effort":"high","format":{"type":"json_schema","schema":schema}});
        let result = request(input.clone(), "m").unwrap();
        assert!(result.get("outputConfig").is_none());
        assert_eq!(
            result["additionalModelRequestFields"]["output_config"],
            json!({"effort":"high","format":{"type":"json_schema","schema":schema}})
        );
        let gpt = request(input, "us.openai.gpt-5.6-luna").unwrap();
        assert_eq!(
            gpt["additionalModelRequestFields"]["text"]["format"]["schema"],
            schema
        );
        assert_eq!(
            gpt["additionalModelRequestFields"]["reasoning"]["effort"],
            "high"
        );
        assert_eq!(
            result["additionalModelRequestFields"]["thinking"]["type"],
            "adaptive"
        );
    }

    #[test]
    fn rejects_invalid_parameter_limits_and_unsupported_required_behaviors() {
        for (key, value) in [
            ("max_tokens", json!(0)),
            ("max_tokens", json!(-1)),
            ("max_tokens", json!(2_147_483_648u64)),
            ("temperature", json!(1.1)),
            ("top_p", json!(-0.1)),
            ("top_k", json!(-1)),
            ("stop_sequences", json!([""])),
            ("stream", json!("true")),
            ("thinking", json!({"type":"enabled","budget_tokens":4096})),
            ("thinking", json!({"type":"enabled","budget_tokens":100})),
            ("thinking", json!({"type":"adaptive","budget_tokens":1024})),
            ("context_management", json!({"edits":[]})),
        ] {
            let mut input = basic();
            input[key] = value;
            assert!(request(input, "m").is_err(), "accepted invalid {key}");
        }
        let mut input = basic();
        input["tools"] = json!([{"name":"Read","input_schema":{},"defer_loading":true}]);
        assert!(request(input, "m").unwrap_err().contains("Deferred"));
        let mut input = basic();
        input["tools"] = json!([{"name":"a".repeat(65),"input_schema":{}}]);
        assert!(request(input, "m").unwrap_err().contains("64-character"));
        let mut input = basic();
        input["messages"][0]["content"] = json!([{"type":"document","source":{}}]);
        assert!(request(input, "m").unwrap_err().contains("Unsupported"));
    }

    #[test]
    fn rejects_orphaned_tool_results_missing_results_and_unsigned_reasoning() {
        let mut input = basic();
        input["messages"][0]["content"] =
            json!([{"type":"tool_result","tool_use_id":"missing","content":"x"}]);
        assert!(request(input, "m").unwrap_err().contains("no outstanding"));
        let mut input = basic();
        input["messages"].as_array_mut().unwrap().push(json!({"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{}}]}));
        assert!(
            request(input, "m")
                .unwrap_err()
                .contains("without tool_result")
        );
        let bad = upstream(
            json!([{"reasoningContent":{"reasoningText":{"text":"reasoning"}}}]),
            "end_turn",
        );
        assert!(response(bad, "m").unwrap_err().contains("signature"));
    }

    #[test]
    fn validates_image_sources_and_preserves_base64_wire_encoding() {
        let mut input = basic();
        input["messages"][0]["content"] = json!([{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGVsbG8="}}]);
        assert_eq!(
            request(input.clone(), "m").unwrap()["messages"][0]["content"][0]["image"]["source"]["bytes"],
            "aGVsbG8="
        );
        input["messages"][0]["content"][0]["source"]["data"] = json!("!");
        assert!(request(input, "m").is_err());
        let mut input = basic();
        input["messages"][0]["content"] =
            json!([{"type":"image","source":{"type":"url","url":"https://example.invalid/image"}}]);
        assert!(request(input, "m").is_err());
    }

    #[test]
    fn response_preserves_usage_cache_breakdown_and_stop_sequence() {
        let mut body = upstream(json!([{"text":"Done"}]), "stop_sequence");
        body["usage"] = json!({"inputTokens":11,"outputTokens":17,"totalTokens":28,"cacheReadInputTokens":2048,"cacheWriteInputTokens":1024,"cacheDetails":[{"ttl":"1h","inputTokens":1000},{"ttl":"5m","inputTokens":24}]});
        body["additionalModelResponseFields"] = json!({"stop_sequence":"STOP"});
        let result = response(body, "claude-visible-alias").unwrap();
        assert_eq!(result["model"], "claude-visible-alias");
        assert_eq!(result["stop_sequence"], "STOP");
        assert_eq!(
            result["usage"],
            json!({"input_tokens":11,"output_tokens":17,"cache_read_input_tokens":2048,"cache_creation_input_tokens":1024,"cache_creation":{"ephemeral_1h_input_tokens":1000,"ephemeral_5m_input_tokens":24}})
        );
    }

    #[test]
    fn response_rejects_partial_malformed_and_unknown_output() {
        for (content, stop) in [
            (json!([{"text":"partial"}]), "malformed_model_output"),
            (json!([]), "tool_use"),
            (json!([{"text":"a","toolUse":{}}]), "end_turn"),
            (json!([{"citationsContent":{}}]), "end_turn"),
            (
                json!([{"toolUse":{"toolUseId":"x","name":"Read","input":"broken"}}]),
                "tool_use",
            ),
        ] {
            assert!(response(upstream(content, stop), "m").is_err());
        }
        let mut body = upstream(json!([{"text":"Done"}]), "end_turn");
        body.as_object_mut().unwrap().remove("usage");
        assert!(response(body, "m").is_err());
        assert_eq!(
            response(
                upstream(json!([{"text":"Blocked"}]), "guardrail_intervened"),
                "m"
            )
            .unwrap()["stop_reason"],
            "refusal"
        );
    }
}
