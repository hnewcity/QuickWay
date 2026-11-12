//! Translate one Quick cloud reply into Anthropic message events.
//!
//! The transport filters conversation IDs and validates/maps external tools
//! before passing `tool_call` events here. Cloud-owned tool progress and
//! unsigned thinking are intentionally not Anthropic content blocks.

use crate::model_error::ModelError;
use serde_json::{Map, Value, json};

const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOOL_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLOCKS: usize = 65536;

pub struct Translator {
    model: String,
    id: String,
    started: bool,
    stop: Option<&'static str>,
    failure: Option<ModelError>,
    content: Vec<Value>,
    open_text: Option<usize>,
    text_prefix: String,
    text: String,
    client_text: crate::cloud_text::ClientText,
    resumed_after_tools: bool,
    usage: Option<Usage>,
    context_usage: Option<Usage>,
    tools: usize,
    tool_bytes: usize,
    metadata: Map<String, Value>,
}

#[derive(Clone)]
struct Usage {
    input: u64,
    output: u64,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
}

impl Usage {
    fn from_payload(p: &Value) -> Result<Self, ModelError> {
        let required = |name| {
            p.get(name)
                .and_then(Value::as_u64)
                .ok_or_else(|| ModelError::from("Quick cloud reported invalid token usage"))
        };
        let optional = |name| match p.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .map(Some)
                .ok_or_else(|| ModelError::from("Quick cloud reported invalid cache token usage")),
        };
        Ok(Self {
            input: required("input_tokens")?,
            output: required("output_tokens")?,
            cache_read: optional("cache_read_tokens")?,
            cache_write: optional("cache_write_tokens")?,
        })
    }

    fn add(&mut self, next: Self) -> Result<(), ModelError> {
        let sum = |a: u64, b: u64| {
            a.checked_add(b)
                .ok_or_else(|| ModelError::from("Quick cloud token usage overflow"))
        };
        self.input = sum(self.input, next.input)?;
        self.output = sum(self.output, next.output)?;
        self.cache_read = match (self.cache_read, next.cache_read) {
            (Some(a), Some(b)) => Some(sum(a, b)?),
            _ => None,
        };
        self.cache_write = match (self.cache_write, next.cache_write) {
            (Some(a), Some(b)) => Some(sum(a, b)?),
            _ => None,
        };
        Ok(())
    }

    fn value(&self) -> Value {
        let mut value = json!({"input_tokens":self.input,"output_tokens":self.output});
        if let Some(read) = self.cache_read {
            value["cache_read_input_tokens"] = read.into();
        }
        if let Some(write) = self.cache_write {
            value["cache_creation_input_tokens"] = write.into();
        }
        value
    }
}

impl Translator {
    pub fn new(public_model: String) -> Self {
        Self {
            model: public_model,
            id: format!("msg_{}", uuid::Uuid::new_v4().simple()),
            started: false,
            stop: None,
            failure: None,
            content: Vec::new(),
            open_text: None,
            text_prefix: String::new(),
            text: String::new(),
            client_text: crate::cloud_text::ClientText::new(),
            resumed_after_tools: false,
            usage: None,
            context_usage: None,
            tools: 0,
            tool_bytes: 0,
            metadata: Map::new(),
        }
    }

    /// Resume the same cloud message after an API tool boundary. The prefix
    /// has already been emitted by previous HTTP replies and is used only to
    /// reconcile cumulative text snapshots. Previous usage is not re-counted.
    pub fn continue_after_tools(
        public_model: String,
        text_prefix: String,
    ) -> Result<Self, ModelError> {
        if text_prefix.len() > MAX_TEXT_BYTES {
            return Err("Quick cloud reply text exceeds 16 MiB".into());
        }
        let mut translator = Self::new(public_model);
        translator.text_prefix = text_prefix;
        translator.resumed_after_tools = true;
        Ok(translator)
    }

    /// All text already emitted within the current cloud message, including
    /// earlier API tool boundaries. Reset by using `new` for a new user turn.
    pub fn text_seen(&self) -> String {
        let mut text = String::with_capacity(self.text_prefix.len() + self.text.len());
        text.push_str(&self.text_prefix);
        text.push_str(&self.text);
        text
    }

    /// External `tool_call` arguments must already be the validated client
    /// input, without Quick's `params` wrapper; `tool_name` must already be the
    /// caller's original name. Call `finish_tools` after collecting the batch.
    pub fn ingest(&mut self, event: &Value) -> Result<Vec<Value>, ModelError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.ingest_inner(event);
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }

    fn ingest_inner(&mut self, event: &Value) -> Result<Vec<Value>, ModelError> {
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .ok_or("Quick cloud event has no type")?;
        let p = event.get("payload").unwrap_or(event);
        if kind == "error" || kind == "exception" || kind.ends_with("Exception") {
            let mut diagnostic = p.as_object().cloned().unwrap_or_default();
            if !diagnostic.contains_key("errorType")
                && let Some(code) = p.get("error_type").and_then(Value::as_str)
            {
                diagnostic.insert("errorType".into(), code.into());
            }
            return Err(ModelError::from_upstream(kind, &Value::Object(diagnostic)));
        }
        if self.stop.is_some() {
            return match kind {
                "message_complete" => Ok(Vec::new()),
                "message_start" | "text_delta" | "text_complete" | "tool_call" | "turn_usage" => {
                    Err("Quick cloud sent reply content after completion".into())
                }
                _ => Ok(Vec::new()),
            };
        }
        let mut out = Vec::new();
        match kind {
            "message_start" => {
                if p.get("role").and_then(Value::as_str) != Some("assistant") {
                    return Err("Quick cloud message_start has no assistant role".into());
                }
                self.start(&mut out);
            }
            "text_delta" => {
                let delta = p
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or("Quick cloud text_delta has no text")?;
                self.append_text(delta, &mut out)?;
            }
            "text_complete" => {
                let text = p
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("Quick cloud text_complete has no text")?;
                self.reconcile_text(text, &mut out)?;
                self.close_text(&mut out);
            }
            "turn_usage" => {
                let next = Usage::from_payload(p)?;
                self.context_usage = Some(next.clone());
                match &mut self.usage {
                    Some(total) => total.add(next)?,
                    None => self.usage = Some(next),
                }
                // Quick may run several internal model calls for one API reply.
                // Keep their actual billed totals separately: summing their
                // repeated prompts here makes Claude Code compact prematurely.
                self.metadata
                    .insert("cloud_usage".into(), self.usage.as_ref().unwrap().value());
                for (source, target) in [
                    ("model", "mode"),
                    ("resolved_model_id", "resolved_model"),
                    ("served_model_id", "served_model"),
                ] {
                    if let Some(value) = p.get(source).and_then(Value::as_str).filter(|s| {
                        !s.is_empty()
                            && s.len() <= 512
                            && s.bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"._:/-".contains(&b))
                            && !s.contains("://")
                    }) {
                        self.metadata.insert(target.into(), value.into());
                    }
                }
            }
            "tool_call" => self.add_tool(p, &mut out)?,
            "message_complete" => {
                if self.tools != 0 {
                    return Err(
                        "Quick cloud completed while client tools are awaiting results".into(),
                    );
                }
                if let Some(text) = p.get("text").and_then(Value::as_str) {
                    self.reconcile_text(text, &mut out)?;
                }
                self.complete("end_turn", &mut out)?;
            }
            "stream_ended" => {
                return Err("Quick cloud stream ended without message_complete".into());
            }
            // Internal tool start/arguments/progress/result events and unsigned
            // thinking are not caller-owned tool_use or replayable thinking.
            _ => {}
        }
        Ok(out)
    }

    fn start(&mut self, out: &mut Vec<Value>) {
        if self.started {
            return;
        }
        self.started = true;
        // SSE starts with provisional placeholders for client compatibility.
        // Terminal events and assembled replies always require actual usage.
        out.push(json!({"type":"message_start","message":{
            "id":self.id,"type":"message","role":"assistant","model":self.model,
            "content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}
        }}));
    }

    fn append_text(&mut self, delta: &str, out: &mut Vec<Value>) -> Result<(), ModelError> {
        if delta.is_empty() {
            return Ok(());
        }
        if delta.len() > MAX_TEXT_BYTES.saturating_sub(self.text_prefix.len() + self.text.len()) {
            return Err("Quick cloud reply text exceeds 16 MiB".into());
        }
        self.text.push_str(delta);
        let rendered = self.client_text.push(delta, false);
        self.emit_text(&rendered, out)
    }

    fn emit_text(&mut self, delta: &str, out: &mut Vec<Value>) -> Result<(), ModelError> {
        if delta.is_empty() {
            return Ok(());
        }
        if self.open_text.is_none() && self.content.len() >= MAX_BLOCKS {
            return Err("Quick cloud reply has too many content blocks".into());
        }
        self.start(out);
        let index = match self.open_text {
            Some(index) => index,
            None => {
                let index = self.content.len();
                self.content.push(json!({"type":"text","text":""}));
                self.open_text = Some(index);
                out.push(json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}}));
                index
            }
        };
        if let Value::String(block) = &mut self.content[index]["text"] {
            block.push_str(delta);
        }
        out.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":delta}}));
        Ok(())
    }

    fn reconcile_text(&mut self, text: &str, out: &mut Vec<Value>) -> Result<(), ModelError> {
        let direct = text.strip_prefix(&self.text);
        let cumulative = text
            .strip_prefix(&self.text_prefix)
            .and_then(|current| current.strip_prefix(&self.text));
        // An exact current-segment snapshot must not be stripped simply
        // because its text happens to repeat the prior segment's prefix.
        let suffix = match (direct, cumulative) {
            (Some(""), _) | (_, Some("")) => "",
            (Some(direct), Some(cumulative)) if self.text_prefix.is_empty() => {
                debug_assert_eq!(direct, cumulative);
                direct
            }
            // Before any delta, a resumed full snapshot is the only source
            // of the new suffix; the caller explicitly supplied its prefix.
            (Some(_), Some(cumulative)) if self.text.is_empty() => cumulative,
            (Some(_), Some(_)) => {
                return Err("Quick cloud text snapshot is ambiguous after a tool boundary".into());
            }
            (Some(direct), None) => direct,
            (None, Some(cumulative)) => cumulative,
            (None, None)
                if self.text.starts_with(text)
                    || text
                        .strip_prefix(&self.text_prefix)
                        .is_some_and(|current| self.text.starts_with(current)) =>
            {
                ""
            }
            _ => return Err("Quick cloud completed text conflicts with streamed text".into()),
        };
        self.append_text(suffix, out)
    }

    fn close_text(&mut self, out: &mut Vec<Value>) {
        if let Some(index) = self.open_text.take() {
            out.push(json!({"type":"content_block_stop","index":index}));
        }
    }

    fn add_tool(&mut self, p: &Value, out: &mut Vec<Value>) -> Result<(), ModelError> {
        let id = p
            .get("tool_call_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control))
            .ok_or("Quick cloud tool call has an invalid ID")?;
        let name = p
            .get("tool_name")
            .and_then(Value::as_str)
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 128
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            })
            .ok_or("Quick cloud tool call has an invalid mapped name")?;
        let input = p
            .get("arguments")
            .filter(|v| v.is_object())
            .ok_or("Quick cloud tool call has no validated input object")?;
        let content = json!({"type":"tool_use","id":id,"name":name,"input":input});
        if let Some(previous) = self
            .content
            .iter()
            .find(|block| block["type"] == "tool_use" && block["id"] == id)
        {
            return if previous == &content {
                Ok(())
            } else {
                Err("Quick cloud reused a tool call ID with different input".into())
            };
        }
        let serialized = input.to_string();
        if serialized.len() > MAX_TOOL_BYTES.saturating_sub(self.tool_bytes)
            || self.content.len() >= MAX_BLOCKS
        {
            return Err("Quick cloud tool call exceeds the reply limit".into());
        }
        self.start(out);
        let rendered = self.client_text.push("", true);
        self.emit_text(&rendered, out)?;
        self.close_text(out);
        let index = self.content.len();
        self.content.push(content);
        self.tools += 1;
        self.tool_bytes += serialized.len();
        out.push(json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":id,"name":name,"input":{}}}));
        out.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":serialized}}));
        out.push(json!({"type":"content_block_stop","index":index}));
        Ok(())
    }

    fn complete(&mut self, reason: &'static str, out: &mut Vec<Value>) -> Result<(), ModelError> {
        let usage = self.final_usage(reason)?;
        self.start(out);
        let rendered = self.client_text.push("", true);
        self.emit_text(&rendered, out)?;
        self.close_text(out);
        self.stop = Some(reason);
        let mut terminal = json!({"type":"message_delta","delta":{"stop_reason":reason,"stop_sequence":null},"usage":usage});
        if !self.metadata.is_empty() {
            terminal["quickway"] = self.metadata.clone().into();
        }
        out.push(terminal);
        out.push(json!({"type":"message_stop"}));
        Ok(())
    }

    fn final_usage(&self, reason: &str) -> Result<Value, ModelError> {
        match &self.usage {
            Some(usage) => {
                let mut client_usage = self.context_usage.as_ref().unwrap_or(usage).clone();
                client_usage.output = usage.output;
                Ok(client_usage.value())
            }
            // Several callbacks from one cloud model generation can arrive
            // in different WebSocket messages. Its generation usage was
            // already reported before the first callback. Only an explicit
            // continuation may account for a later callback as zero new use.
            None if self.resumed_after_tools && reason == "tool_use" => {
                Ok(json!({"input_tokens":0,"output_tokens":0}))
            }
            None => Err("Quick cloud reply ended without token usage".into()),
        }
    }

    /// End the current HTTP response after all currently dispatched external
    /// tool calls have been ingested. The WebSocket stays owned by transport.
    pub fn finish_tools(&mut self) -> Result<Vec<Value>, ModelError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if self.stop == Some("tool_use") {
            return Ok(Vec::new());
        }
        if self.stop.is_some() || self.tools == 0 {
            return Err("Quick cloud has no pending client tools to finish".into());
        }
        let mut out = Vec::new();
        if let Err(error) = self.complete("tool_use", &mut out) {
            self.failure = Some(error.clone());
            return Err(error);
        }
        Ok(out)
    }

    pub fn is_finished(&self) -> bool {
        self.stop.is_some() && self.failure.is_none()
    }

    /// Assembled non-streaming reply. A disconnect or partial stream cannot be
    /// converted into a successful message without a terminal event and usage.
    pub fn message(&self) -> Result<Value, ModelError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let stop = self.stop.ok_or("Quick cloud reply is incomplete")?;
        let usage = self.final_usage(stop)?;
        let mut message = json!({"id":self.id,"type":"message","role":"assistant","model":self.model,
            "content":self.content,"stop_reason":stop,"stop_sequence":null,"usage":usage});
        if !self.metadata.is_empty() {
            message["quickway"] = self.metadata.clone().into();
        }
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_use_raw_snapshots_but_emit_only_readable_text() {
        let raw = "Before\n<decision question=\"Next?\"><option description=\"Run checks\">Test</option></decision>";
        let mut t = Translator::new("quick-smart".into());
        let mut events = t
            .ingest(&event("text_delta", json!({"delta":raw})))
            .unwrap();
        t.ingest(&usage(10, 20)).unwrap();
        events.extend(
            t.ingest(&event("message_complete", json!({"text":raw})))
                .unwrap(),
        );
        let actual: String = events
            .iter()
            .filter_map(|e| e["delta"]["text"].as_str())
            .collect();
        assert_eq!(actual, "Before\nNext?\n- Test: Run checks");
        assert_eq!(t.text_seen(), raw);
        let message = t.message().unwrap();
        let assembled: String = message["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect();
        assert_eq!(assembled, actual);
    }

    fn event(kind: &str, payload: Value) -> Value {
        json!({"type":kind,"payload":payload})
    }
    fn usage(input: u64, output: u64) -> Value {
        event(
            "turn_usage",
            json!({"input_tokens":input,"output_tokens":output,"cache_read_tokens":5,"cache_write_tokens":7}),
        )
    }
    fn tool(id: &str, name: &str, input: Value) -> Value {
        event(
            "tool_call",
            json!({"tool_call_id":id,"tool_name":name,"arguments":input,"timeout_ms":60000}),
        )
    }

    #[test]
    fn live_reply_shape_streams_text_and_does_not_duplicate_snapshots() {
        let mut t = Translator::new("quick-smart".into());
        let mut out = t
            .ingest(&event("message_start", json!({"role":"assistant"})))
            .unwrap();
        for delta in ["QUICKWAY_", "ECHO_ROUNDTRIP_OK"] {
            let next = t
                .ingest(&event(
                    "text_delta",
                    json!({"delta":delta,"kernel_thread_id":"thread"}),
                ))
                .unwrap();
            assert!(next.iter().any(|e| e["type"] == "content_block_delta"));
            out.extend(next);
        }
        t.ingest(&usage(2, 27)).unwrap();
        let complete = event(
            "text_complete",
            json!({"text":"QUICKWAY_ECHO_ROUNDTRIP_OK"}),
        );
        out.extend(t.ingest(&complete).unwrap());
        assert!(t.ingest(&complete).unwrap().is_empty());
        out.extend(
            t.ingest(&event(
                "message_complete",
                json!({"text":"QUICKWAY_ECHO_ROUNDTRIP_OK","tool_calls":[]}),
            ))
            .unwrap(),
        );
        assert!(
            t.ingest(&event("message_complete", json!({})))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            out.iter().filter(|e| e["type"] == "message_start").count(),
            1
        );
        assert_eq!(
            out.iter().filter(|e| e["type"] == "message_stop").count(),
            1
        );
        assert_eq!(
            out[0]["message"]["usage"],
            json!({"input_tokens":0,"output_tokens":0})
        );
        let response = t.message().unwrap();
        assert_eq!(
            response["content"],
            json!([{"type":"text","text":"QUICKWAY_ECHO_ROUNDTRIP_OK"}])
        );
        assert_eq!(response["usage"]["output_tokens"], 27);
        assert_eq!(response["stop_reason"], "end_turn");
    }

    #[test]
    fn final_snapshot_supplies_missing_suffix_without_repeating_unicode() {
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&event("text_delta", json!({"delta":"你好"})))
            .unwrap();
        t.ingest(&usage(1, 2)).unwrap();
        let out = t
            .ingest(&event("message_complete", json!({"text":"你好，世界🦀"})))
            .unwrap();
        let delta = out
            .iter()
            .find(|e| e["type"] == "content_block_delta")
            .unwrap();
        assert_eq!(delta["delta"]["text"], "，世界🦀");
        assert_eq!(t.message().unwrap()["content"][0]["text"], "你好，世界🦀");
    }

    #[test]
    fn internal_tools_and_unsigned_thinking_never_become_client_blocks() {
        let mut t = Translator::new("quick-smart".into());
        for kind in [
            "tool_start",
            "tool_progress",
            "tool_arguments",
            "tool_result",
            "tool_complete",
            "thinking_start",
            "thinking_delta",
            "thinking_complete",
        ] {
            assert!(t.ingest(&event(kind,json!({"tool_name":"cloud-internal","delta":"private thinking","result":"private result"}))).unwrap().is_empty());
        }
        t.ingest(&usage(4, 57)).unwrap();
        t.ingest(&usage(2, 80)).unwrap();
        t.ingest(&usage(2, 27)).unwrap();
        t.ingest(&event(
            "message_complete",
            json!({"text":"done","tool_calls":[{"name":"cloud-internal"}]}),
        ))
        .unwrap();
        let message = t.message().unwrap();
        assert_eq!(message["content"], json!([{"type":"text","text":"done"}]));
        assert_eq!(
            message["usage"],
            json!({"input_tokens":2,"output_tokens":164,"cache_read_input_tokens":5,"cache_creation_input_tokens":7})
        );
        assert_eq!(
            message["quickway"]["cloud_usage"],
            json!({"input_tokens":8,"output_tokens":164,"cache_read_input_tokens":15,"cache_creation_input_tokens":21})
        );
    }

    #[test]
    fn parallel_client_tools_use_distinct_indexes_and_finish_once() {
        let mut t = Translator::new("quick-smart".into());
        let mut events = t
            .ingest(&event("text_delta", json!({"delta":"Checking."})))
            .unwrap();
        t.ingest(&usage(4, 57)).unwrap();
        let first = tool("call-a", "Read", json!({"file_path":"example.rs"}));
        events.extend(t.ingest(&first).unwrap());
        assert!(t.ingest(&first).unwrap().is_empty());
        events.extend(t.ingest(&tool("call-b", "List", json!({}))).unwrap());
        events.extend(t.finish_tools().unwrap());
        assert!(t.finish_tools().unwrap().is_empty());
        let starts: Vec<_> = events
            .iter()
            .filter(|e| e["type"] == "content_block_start")
            .map(|e| e["index"].as_u64().unwrap())
            .collect();
        assert_eq!(starts, [0, 1, 2]);
        let message = t.message().unwrap();
        assert_eq!(message["stop_reason"], "tool_use");
        assert_eq!(message["content"][1]["name"], "Read");
        assert_eq!(
            message["content"][1]["input"],
            json!({"file_path":"example.rs"})
        );
        assert_eq!(message["content"][2]["input"], json!({}));
    }

    #[test]
    fn unknown_usage_and_disconnects_cannot_produce_success() {
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&event("text_delta", json!({"delta":"partial"})))
            .unwrap();
        assert!(t.message().is_err());
        assert!(
            t.ingest(&event("message_complete", json!({"text":"partial"})))
                .unwrap_err()
                .message
                .contains("without token usage")
        );
        assert!(!t.is_finished());
        assert!(t.ingest(&usage(1, 1)).is_err());
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&usage(1, 1)).unwrap();
        assert!(t.ingest(&event("stream_ended", json!({}))).is_err());
        assert!(t.message().is_err());
    }

    #[test]
    fn upstream_error_poisoning_preserves_identity_and_never_finishes_successfully() {
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&usage(1, 1)).unwrap();
        t.ingest(&event("text_delta", json!({"delta":"partial"})))
            .unwrap();
        let error=t.ingest(&event("error",json!({"error_type":"AccessDeniedException","error":"This client is not supported for this request."}))).unwrap_err();
        assert_eq!(error.kind, "permission_error");
        assert_eq!(error.code.as_deref(), Some("AccessDeniedException"));
        assert!(t.ingest(&event("message_complete", json!({}))).is_err());
        assert!(t.finish_tools().is_err());
        assert!(t.message().is_err());
    }

    #[test]
    fn conflicting_snapshots_invalid_tools_and_invalid_usage_are_not_silently_accepted() {
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&event("text_delta", json!({"delta":"first"})))
            .unwrap();
        assert!(
            t.ingest(&event("text_complete", json!({"text":"different"})))
                .is_err()
        );
        for value in [
            event(
                "tool_call",
                json!({"tool_call_id":"a","tool_name":"echo","arguments":[]}),
            ),
            tool("a", "bad.name", json!({})),
            event("turn_usage", json!({"input_tokens":1})),
            event("turn_usage", json!({"input_tokens":-1,"output_tokens":1})),
        ] {
            assert!(
                Translator::new("quick-smart".into())
                    .ingest(&value)
                    .is_err()
            );
        }
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&tool("a", "echo", json!({"x":1}))).unwrap();
        assert!(t.ingest(&tool("a", "echo", json!({"x":2}))).is_err());
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&usage(u64::MAX, 1)).unwrap();
        assert!(t.ingest(&usage(1, 1)).is_err());
    }

    #[test]
    fn omitted_cache_counts_remain_unknown_instead_of_becoming_zero() {
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&usage(1, 1)).unwrap();
        t.ingest(&event(
            "turn_usage",
            json!({"input_tokens":2,"output_tokens":3}),
        ))
        .unwrap();
        t.ingest(&event("message_complete", json!({"text":"OK"})))
            .unwrap();
        assert_eq!(
            t.message().unwrap()["usage"],
            json!({"input_tokens":2,"output_tokens":4})
        );
        assert_eq!(
            t.message().unwrap()["quickway"]["cloud_usage"],
            json!({"input_tokens":3,"output_tokens":4})
        );
    }

    #[test]
    fn internal_rounds_do_not_masquerade_as_a_full_client_context() {
        let mut t = Translator::new("quick-smart".into());
        for input in [90_000, 95_000, 100_000] {
            t.ingest(&usage(input, 10)).unwrap();
        }
        t.ingest(&tool("read1", "Read", json!({"file_path":"fixture.txt"})))
            .unwrap();
        let events = t.finish_tools().unwrap();
        let message = t.message().unwrap();
        assert_eq!(message["usage"]["input_tokens"], 100_000);
        assert_eq!(message["quickway"]["cloud_usage"]["input_tokens"], 285_000);
        let delta = events
            .iter()
            .find(|e| e["type"] == "message_delta")
            .unwrap();
        assert_eq!(delta["usage"], message["usage"]);
        assert_eq!(
            delta["quickway"]["cloud_usage"],
            message["quickway"]["cloud_usage"]
        );
    }

    #[test]
    fn reported_backend_identity_is_metadata_and_never_replaces_public_mode() {
        let mut t = Translator::new("quick-smart".into());
        t.ingest(&event(
            "turn_usage",
            json!({
                "input_tokens":4,"output_tokens":27,"model":"smart",
                "resolved_model_id":"us.anthropic.claude-opus-5-5",
                "served_model_id":"us.anthropic.claude-opus-5-5"
            }),
        ))
        .unwrap();
        let events = t
            .ingest(&event("message_complete", json!({"text":"OK"})))
            .unwrap();
        let message = t.message().unwrap();
        assert_eq!(message["model"], "quick-smart");
        assert_eq!(
            message["quickway"]["resolved_model"],
            "us.anthropic.claude-opus-5-5"
        );
        assert_eq!(message["quickway"]["mode"], "smart");
        assert_eq!(
            events
                .iter()
                .find(|event| event["type"] == "message_delta")
                .unwrap()["quickway"],
            message["quickway"]
        );
    }

    #[test]
    fn resumed_snapshots_exclude_pre_tool_text_and_preserve_new_deltas() {
        let mut t =
            Translator::continue_after_tools("quick-smart".into(), "Checking. ".into()).unwrap();
        t.ingest(&event("text_delta", json!({"delta":"Found"})))
            .unwrap();
        t.ingest(&usage(2, 5)).unwrap();
        t.ingest(&event(
            "text_complete",
            json!({"text":"Checking. Found it."}),
        ))
        .unwrap();
        t.ingest(&event(
            "message_complete",
            json!({"text":"Checking. Found it."}),
        ))
        .unwrap();
        assert_eq!(
            t.message().unwrap()["content"],
            json!([{"type":"text","text":"Found it."}])
        );
        assert_eq!(t.text_seen(), "Checking. Found it.");
    }

    #[test]
    fn resumed_segment_only_snapshot_does_not_lose_repeated_prefix() {
        let mut t =
            Translator::continue_after_tools("quick-smart".into(), "Again ".into()).unwrap();
        t.ingest(&event("text_delta", json!({"delta":"Again "})))
            .unwrap();
        t.ingest(&usage(2, 5)).unwrap();
        t.ingest(&event(
            "message_complete",
            json!({"text":"Again with a new result."}),
        ))
        .unwrap();
        assert_eq!(
            t.message().unwrap()["content"][0]["text"],
            "Again with a new result."
        );
        assert_eq!(t.text_seen(), "Again Again with a new result.");
    }

    #[test]
    fn resumed_snapshot_without_deltas_can_supply_the_new_suffix() {
        let mut t =
            Translator::continue_after_tools("quick-smart".into(), "Before. ".into()).unwrap();
        t.ingest(&usage(2, 5)).unwrap();
        t.ingest(&event("message_complete", json!({"text":"Before. After."})))
            .unwrap();
        assert_eq!(t.message().unwrap()["content"][0]["text"], "After.");
        let mut t =
            Translator::continue_after_tools("quick-smart".into(), "Before. ".into()).unwrap();
        t.ingest(&usage(2, 5)).unwrap();
        t.ingest(&event(
            "message_complete",
            json!({"text":"Only the new segment."}),
        ))
        .unwrap();
        assert_eq!(
            t.message().unwrap()["content"][0]["text"],
            "Only the new segment."
        );
    }

    #[test]
    fn only_explicit_tool_continuation_can_have_zero_additional_usage() {
        let callback = tool("call-b", "Read", json!({"path":"second.txt"}));
        let mut fresh = Translator::new("quick-smart".into());
        fresh.ingest(&callback).unwrap();
        assert!(fresh.finish_tools().is_err());
        let mut resumed =
            Translator::continue_after_tools("quick-smart".into(), String::new()).unwrap();
        resumed.ingest(&callback).unwrap();
        resumed.finish_tools().unwrap();
        assert_eq!(
            resumed.message().unwrap()["usage"],
            json!({"input_tokens":0,"output_tokens":0})
        );
        let mut resumed =
            Translator::continue_after_tools("quick-smart".into(), String::new()).unwrap();
        resumed.ingest(&callback).unwrap();
        resumed.ingest(&usage(2, 7)).unwrap();
        resumed.finish_tools().unwrap();
        assert_eq!(resumed.message().unwrap()["usage"]["output_tokens"], 7);
        let mut resumed =
            Translator::continue_after_tools("quick-smart".into(), String::new()).unwrap();
        assert!(
            resumed
                .ingest(&event("message_complete", json!({"text":"No new usage"})))
                .is_err()
        );
    }
}
