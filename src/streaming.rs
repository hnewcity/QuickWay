//! Convert the Quick BedrockStreamEvent envelope observed on the wire to SSE.
use crate::eventstream::Decoder;
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    pin::Pin,
};

struct Block {
    kind: &'static str,
    output_index: u64,
    stopped: bool,
    tool_json: String,
}
pub struct Translator {
    model: String,
    started: bool,
    stop: Option<String>,
    stop_sequence: Option<String>,
    usage: Option<Value>,
    blocks: BTreeMap<u64, Block>,
}
impl Translator {
    pub fn new(model: String) -> Self {
        Self {
            model,
            started: false,
            stop: None,
            stop_sequence: None,
            usage: None,
            blocks: BTreeMap::new(),
        }
    }
    pub fn ingest(&mut self, event: Value) -> Result<Vec<Value>, String> {
        let kind = event["eventType"]
            .as_str()
            .ok_or("Quick stream event has no eventType")?
            .to_ascii_lowercase();
        let p = &event["payload"];
        if !p.is_object() {
            return Err("Quick stream event has no object payload".into());
        }
        let mut out = Vec::new();
        if kind == "messagestart" {
            if self.started || p["role"] != "assistant" {
                return Err("Invalid messageStart".into());
            }
            self.started = true;
            out.push(json!({"type":"message_start","message":{"id":format!("msg_{}",uuid::Uuid::new_v4().simple()),"type":"message","role":"assistant","model":self.model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}}));
            return Ok(out);
        }
        if kind.contains("error") || kind.contains("exception") || kind == "usagelimitexceeded" {
            return Err(
                "Quick returned an error inside the model stream; check login, request and quota"
                    .into(),
            );
        }
        if !self.started {
            return Err("Quick stream content precedes messageStart".into());
        }
        if kind == "metadata" {
            if self.usage.is_some() {
                return Err("Duplicate stream metadata".into());
            }
            let u = &p["usage"];
            let input = u["inputTokens"].as_u64().ok_or("Missing input usage")?;
            let output = u["outputTokens"].as_u64().ok_or("Missing output usage")?;
            self.usage = Some(
                json!({"input_tokens":input,"output_tokens":output,"cache_creation_input_tokens":u["cacheWriteInputTokens"].as_u64().unwrap_or(0),"cache_read_input_tokens":u["cacheReadInputTokens"].as_u64().unwrap_or(0)}),
            );
            return Ok(out);
        }
        if self.stop.is_some() {
            return Err("Unexpected content after messageStop".into());
        }
        if kind == "messagestop" {
            let reason = p["stopReason"].as_str().ok_or("Missing stop reason")?;
            let reason = match reason {
                "end_turn"
                | "max_tokens"
                | "tool_use"
                | "stop_sequence"
                | "model_context_window_exceeded" => reason,
                "guardrail_intervened" | "content_filtered" => "refusal",
                _ => return Err("Unknown Quick model stop reason".into()),
            };
            if self.blocks.values().any(|b| !b.stopped) {
                return Err("messageStop arrived before contentBlockStop".into());
            }
            // A token-limited tool block can legitimately end midway through
            // JSON. Preserve max_tokens so Claude Code can request more output.
            if reason != "max_tokens"
                && self.blocks.values().any(|b| {
                    // A call to a tool without parameters may stream no input at all.
                    b.kind == "tool"
                        && !b.tool_json.trim().is_empty()
                        && !serde_json::from_str::<Value>(&b.tool_json).is_ok_and(|v| v.is_object())
                })
            {
                return Err("Quick returned invalid JSON for a completed tool call".into());
            }
            let sequence = &p["additionalModelResponseFields"]["stop_sequence"];
            if !sequence.is_null() && !sequence.is_string() {
                return Err("Invalid model stop sequence".into());
            }
            self.stop_sequence = sequence.as_str().map(str::to_string);
            self.stop = Some(reason.to_string());
            return Ok(out);
        }
        let index = p["contentBlockIndex"]
            .as_u64()
            .filter(|i| *i < 65536)
            .ok_or("Invalid content block index")?;
        match kind.as_str() {
            "contentblockstart" => {
                let tool = &p["start"]["toolUse"];
                let id = tool["toolUseId"]
                    .as_str()
                    .ok_or("Unsupported contentBlockStart")?;
                let name = tool["name"].as_str().ok_or("Tool start has no name")?;
                self.open(
                    index,
                    "tool",
                    json!({"type":"tool_use","id":id,"name":name,"input":{}}),
                    &mut out,
                )?;
            }
            "contentblockdelta" => {
                let delta = &p["delta"];
                if crate::models::is_gpt(&self.model)
                    && delta.as_object().is_some_and(|v| v.is_empty())
                {
                    if let Some(block) = self.blocks.get(&index) {
                        if block.stopped {
                            return Err("Empty delta followed block stop".into());
                        }
                    } else {
                        self.open(index, "empty", Value::Null, &mut out)?;
                    }
                    return Ok(out);
                }
                let (block_kind, start, diff) = if let Some(text) = delta["text"].as_str() {
                    (
                        "text",
                        json!({"type":"text","text":""}),
                        json!({"type":"text_delta","text":text}),
                    )
                } else if let Some(input) = delta["toolUse"]["input"].as_str() {
                    let block = self
                        .blocks
                        .get_mut(&index)
                        .ok_or("Tool delta without tool start")?;
                    if block.tool_json.len() + input.len() > 16 * 1024 * 1024 {
                        return Err("Tool JSON exceeds 16 MiB".into());
                    }
                    block.tool_json.push_str(input);
                    (
                        "tool",
                        Value::Null,
                        json!({"type":"input_json_delta","partial_json":input}),
                    )
                } else if let Some(text) = delta["reasoningContent"]["text"].as_str() {
                    (
                        "thinking",
                        json!({"type":"thinking","thinking":"","signature":""}),
                        json!({"type":"thinking_delta","thinking":text}),
                    )
                } else if let Some(sig) = delta["reasoningContent"]["signature"].as_str() {
                    (
                        "thinking",
                        json!({"type":"thinking","thinking":"","signature":""}),
                        json!({"type":"signature_delta","signature":sig}),
                    )
                } else if let Some(data) = delta["reasoningContent"]["redactedContent"].as_str() {
                    self.open(
                        index,
                        "redacted",
                        json!({"type":"redacted_thinking","data":data}),
                        &mut out,
                    )?;
                    return Ok(out);
                } else {
                    return Err("Unsupported Quick content delta".into());
                };
                if !self.blocks.contains_key(&index) || self.blocks[&index].kind == "empty" {
                    self.open(index, block_kind, start, &mut out)?;
                }
                let b = &self.blocks[&index];
                if b.stopped || b.kind != block_kind {
                    return Err("Content delta changed block type or followed block stop".into());
                }
                out.push(json!({"type":"content_block_delta","index":b.output_index,"delta":diff}));
            }
            "contentblockstop" => {
                let b = self
                    .blocks
                    .get_mut(&index)
                    .ok_or("Block stop without content")?;
                if b.stopped {
                    return Err("Duplicate contentBlockStop".into());
                }
                b.stopped = true;
                if b.kind != "empty" {
                    out.push(json!({"type":"content_block_stop","index":b.output_index}));
                }
            }
            _ => return Err("Unknown Quick stream event".into()),
        }
        Ok(out)
    }
    fn open(
        &mut self,
        index: u64,
        kind: &'static str,
        content: Value,
        out: &mut Vec<Value>,
    ) -> Result<(), String> {
        let promoting = self
            .blocks
            .get(&index)
            .is_some_and(|b| b.kind == "empty" && !b.stopped)
            && index + 1 == self.blocks.len() as u64;
        if !promoting && (self.blocks.contains_key(&index) || index != self.blocks.len() as u64) {
            return Err("Duplicate or out-of-order content block".into());
        }
        let output_index = self.blocks.values().filter(|b| b.kind != "empty").count() as u64;
        self.blocks.insert(
            index,
            Block {
                kind,
                output_index,
                stopped: false,
                tool_json: String::new(),
            },
        );
        if kind != "empty" {
            out.push(
                json!({"type":"content_block_start","index":output_index,"content_block":content}),
            );
        }
        Ok(())
    }
    pub fn finish(&self) -> Result<Vec<Value>, String> {
        let reason = self
            .stop
            .as_ref()
            .ok_or("Quick stream ended without messageStop")?;
        let usage = self
            .usage
            .as_ref()
            .ok_or("Quick stream ended without usage metadata")?;
        Ok(vec![
            json!({"type":"message_delta","delta":{"stop_reason":reason,"stop_sequence":self.stop_sequence},"usage":usage}),
            json!({"type":"message_stop"}),
        ])
    }
}

type ByteStream = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>;
struct State {
    source: ByteStream,
    decoder: Decoder,
    binary: bool,
    json_buffer: Vec<u8>,
    translator: Translator,
    pending: VecDeque<Value>,
    done: bool,
}
/// Anthropic stream events as JSON values. A failure ends with a single
/// `{"type":"error"}` event; other protocol adapters translate from these.
pub fn event_stream(
    response: reqwest::Response,
    model: String,
) -> impl Stream<Item = Value> + Send + 'static {
    let binary = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("application/vnd.amazon.eventstream"));
    let state = State {
        source: Box::pin(response.bytes_stream()),
        decoder: Decoder::new_proxy(),
        binary,
        json_buffer: Vec::new(),
        translator: Translator::new(model),
        pending: VecDeque::new(),
        done: false,
    };
    futures_util::stream::unfold(state, |mut s| async move {
        loop {
            if let Some(event) = s.pending.pop_front() {
                return Some((event, s));
            }
            if s.done {
                return None;
            }
            let result = match s.source.next().await {
                Some(Ok(chunk)) => decode(&mut s, &chunk, false),
                Some(Err(_)) => Err("Quick model stream disconnected or timed out".into()),
                None => {
                    s.done = true;
                    decode(&mut s, &[], true)
                }
            };
            match result {
                Ok(events) => s.pending.extend(events),
                Err(message) => {
                    s.done = true;
                    s.pending.push_back(
                        json!({"type":"error","error":{"type":"api_error","message":message}}),
                    );
                }
            }
        }
    })
}
fn decode(s: &mut State, chunk: &[u8], eof: bool) -> Result<Vec<Value>, String> {
    let events = if s.binary {
        let events = s.decoder.push(chunk)?;
        if eof {
            s.decoder.finish()?;
        }
        events
    } else {
        if s.json_buffer.len() + chunk.len() > 16 * 1024 * 1024 {
            return Err("Quick JSON event exceeds 16 MiB".into());
        }
        s.json_buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        if s.json_buffer.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[') {
            if !eof {
                return Ok(Vec::new());
            }
            let values: Vec<Value> = serde_json::from_slice(&s.json_buffer)
                .map_err(|_| "Invalid Quick JSON event array")?;
            s.json_buffer.clear();
            let mut out = Vec::new();
            for value in values {
                out.extend(s.translator.ingest(value)?);
            }
            out.extend(s.translator.finish()?);
            return Ok(out);
        }
        while let Some(pos) = s.json_buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = s.json_buffer.drain(..=pos).collect();
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let value: Value =
                serde_json::from_slice(&line).map_err(|_| "Invalid Quick NDJSON event")?;
            if let Value::Array(values) = value {
                events.extend(values)
            } else {
                events.push(value)
            }
        }
        if eof && !s.json_buffer.iter().all(u8::is_ascii_whitespace) {
            let value: Value =
                serde_json::from_slice(&s.json_buffer).map_err(|_| "Truncated Quick JSON event")?;
            if let Value::Array(array) = value {
                events.extend(array)
            } else {
                events.push(value)
            }
            s.json_buffer.clear();
        }
        events
    };
    let mut out = Vec::new();
    for event in events {
        out.extend(s.translator.ingest(event)?);
    }
    if eof {
        out.extend(s.translator.finish()?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(kind: &str, p: Value) -> Value {
        json!({"eventType":kind,"payload":p})
    }
    #[test]
    fn gpt_empty_blocks_do_not_create_blank_messages_or_index_gaps() {
        let mut t = Translator::new("gpt-5.6-sol".into());
        t.ingest(event("messageStart", json!({"role":"assistant"})))
            .unwrap();
        assert!(
            t.ingest(event(
                "contentBlockDelta",
                json!({"contentBlockIndex":0,"delta":{}})
            ))
            .unwrap()
            .is_empty()
        );
        assert!(
            t.ingest(event("contentBlockStop", json!({"contentBlockIndex":0})))
                .unwrap()
                .is_empty()
        );
        let text = t
            .ingest(event(
                "contentBlockDelta",
                json!({"contentBlockIndex":1,"delta":{"text":"OK"}}),
            ))
            .unwrap();
        assert_eq!(text[0]["index"], 0);
        assert_eq!(text[1]["index"], 0);
        t.ingest(event("contentBlockStop", json!({"contentBlockIndex":1})))
            .unwrap();
        assert!(
            t.ingest(event(
                "contentBlockDelta",
                json!({"contentBlockIndex":0,"delta":{}})
            ))
            .is_err()
        );
        t.ingest(event("messageStop", json!({"stopReason":"end_turn"})))
            .unwrap();
        t.ingest(event(
            "metadata",
            json!({"usage":{"inputTokens":10,"outputTokens":1}}),
        ))
        .unwrap();
        assert_eq!(t.finish().unwrap().last().unwrap()["type"], "message_stop");
    }

    #[test]
    fn empty_delta_can_precede_real_content_in_the_same_block() {
        let mut t = Translator::new("gpt-6-astra".into());
        t.ingest(event("messageStart", json!({"role":"assistant"})))
            .unwrap();
        t.ingest(event(
            "contentBlockDelta",
            json!({"contentBlockIndex":0,"delta":{}}),
        ))
        .unwrap();
        let text = t
            .ingest(event(
                "contentBlockDelta",
                json!({"contentBlockIndex":0,"delta":{"text":"OK"}}),
            ))
            .unwrap();
        assert_eq!(text[0]["type"], "content_block_start");
        assert_eq!(text[1]["delta"]["text"], "OK");
    }
    #[test]
    fn text_needs_synthesized_start_and_usage_after_stop() {
        let mut t = Translator::new("fixture".into());
        t.ingest(event("messageStart", json!({"role":"assistant"})))
            .unwrap();
        let events = t
            .ingest(event(
                "contentBlockDelta",
                json!({"contentBlockIndex":0,"delta":{"text":"中文"}}),
            ))
            .unwrap();
        assert_eq!(events[0]["type"], "content_block_start");
        t.ingest(event("contentBlockStop", json!({"contentBlockIndex":0})))
            .unwrap();
        t.ingest(event("messageStop", json!({"stopReason":"end_turn"})))
            .unwrap();
        assert!(t.finish().is_err());
        t.ingest(event(
            "metadata",
            json!({"usage":{"inputTokens":5,"outputTokens":2}}),
        ))
        .unwrap();
        assert_eq!(t.finish().unwrap()[0]["usage"]["input_tokens"], 5);
    }
    #[test]
    fn invalid_tool_json_and_truncated_completion_fail() {
        let mut t = Translator::new("fixture".into());
        t.ingest(event("messageStart", json!({"role":"assistant"})))
            .unwrap();
        t.ingest(event(
            "contentBlockStart",
            json!({"contentBlockIndex":0,"start":{"toolUse":{"toolUseId":"t","name":"read"}}}),
        ))
        .unwrap();
        t.ingest(event(
            "contentBlockDelta",
            json!({"contentBlockIndex":0,"delta":{"toolUse":{"input":"{"}}}),
        ))
        .unwrap();
        t.ingest(event("contentBlockStop", json!({"contentBlockIndex":0})))
            .unwrap();
        assert!(
            t.ingest(event("messageStop", json!({"stopReason":"tool_use"})))
                .is_err()
        );
        assert!(t.finish().is_err());
        t.ingest(event("messageStop", json!({"stopReason":"max_tokens"})))
            .unwrap();
        t.ingest(event(
            "metadata",
            json!({"usage":{"inputTokens":2,"outputTokens":1}}),
        ))
        .unwrap();
        assert_eq!(t.finish().unwrap()[0]["delta"]["stop_reason"], "max_tokens");
    }

    #[test]
    fn tool_call_without_input_completes() {
        // Quick streams no input deltas for a call to a parameterless tool.
        let mut t = Translator::new("fixture".into());
        t.ingest(event("messageStart", json!({"role":"assistant"})))
            .unwrap();
        t.ingest(event(
            "contentBlockStart",
            json!({"contentBlockIndex":0,"start":{"toolUse":{"toolUseId":"t","name":"reset"}}}),
        ))
        .unwrap();
        t.ingest(event("contentBlockStop", json!({"contentBlockIndex":0})))
            .unwrap();
        t.ingest(event("messageStop", json!({"stopReason":"tool_use"})))
            .unwrap();
    }

    #[test]
    fn forwards_the_matched_stop_sequence() {
        let mut t = Translator::new("fixture".into());
        t.ingest(event("messageStart", json!({"role":"assistant"})))
            .unwrap();
        t.ingest(event("messageStop",json!({"stopReason":"stop_sequence","additionalModelResponseFields":{"stop_sequence":"DONE"}}))).unwrap();
        t.ingest(event(
            "metadata",
            json!({"usage":{"inputTokens":2,"outputTokens":1}}),
        ))
        .unwrap();
        assert_eq!(t.finish().unwrap()[0]["delta"]["stop_sequence"], "DONE");
    }
}
