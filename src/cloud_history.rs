//! Convert completed Messages history into Quick's native session events.
//!
//! This is an offline, deterministic conversion. The caller supplies only the
//! prefix before the current turn, creates a fresh incognito session, and adds
//! import timestamps before posting the batch. It never resumes a live tool.
use crate::cloud_request::model_tool_name;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

struct Call {
    message_id: String,
    name: String,
}

struct Builder<'a> {
    thread: &'a str,
    events: Vec<Value>,
    pending: BTreeMap<String, Call>,
    seen: BTreeSet<String>,
}

/// Produce native events for a complete, validated history prefix.
///
/// Events deliberately omit `timestamp`: input Messages have no trustworthy
/// wall-clock time. The transport must stamp the batch at import time. IDs are
/// stable for retries to the same newly created session.
pub fn events(messages: &[Value], thread: &str) -> Result<Vec<Value>, String> {
    if thread.trim().is_empty() {
        return Err("History import requires a nonempty session ID".into());
    }
    let mut builder = Builder {
        thread,
        events: Vec::new(),
        pending: BTreeMap::new(),
        seen: BTreeSet::new(),
    };
    for (index, value) in messages.iter().enumerate() {
        let message = object(value, "message")?;
        only(message, &["role", "content"], "message")?;
        let role = required_text(message, "role")?;
        if index == 0 && role != "user" {
            return Err("Imported history must start with a user message".into());
        }
        let content = message
            .get("content")
            .ok_or("History message.content is required")?;
        let blocks = match content {
            Value::String(text) => vec![json!({"type":"text","text":text})],
            Value::Array(blocks) => blocks.clone(),
            _ => return Err("History content must be text or a block array".into()),
        };
        if blocks.is_empty() {
            return Err("History message.content must not be empty".into());
        }
        let message_id = identity("msg", thread, &format!("{index}:{value}"));
        match role {
            "user" => builder.user(&message_id, &blocks)?,
            "assistant" => builder.assistant(&message_id, &blocks)?,
            _ => return Err("Imported history supports user and assistant roles only".into()),
        }
    }
    if !builder.pending.is_empty() {
        return Err("Cannot import history with unresolved tool calls; a new cloud session cannot resume the former session's pending tools".into());
    }
    Ok(builder.events)
}

impl Builder<'_> {
    fn push(&mut self, kind: &str, message_id: &str, data: Value) {
        let id = identity(
            "evt",
            self.thread,
            &format!("{}:{kind}:{message_id}:{data}", self.events.len()),
        );
        self.events.push(json!({
            "id":id,"type":kind,"session_id":self.thread,"message_id":message_id,
            "thread_id":null,"root_group_id":null,"server_sequence_number":null,"data":data
        }));
    }

    fn assistant(&mut self, message_id: &str, blocks: &[Value]) -> Result<(), String> {
        if !self.pending.is_empty() {
            return Err("Assistant history appears before all required tool results".into());
        }
        let mut text = String::new();
        let mut tools_started = false;
        self.push("message_assistant_start", message_id, json!({}));
        for value in blocks {
            let block = object(value, "assistant block")?;
            match required_text(block, "type")? {
                "text" => {
                    only(block, &["type", "text", "cache_control"], "text block")?;
                    let part = required_text(block, "text")?;
                    if tools_started && !part.is_empty() {
                        return Err("Cannot faithfully import assistant text after a tool_use block; Quick history places assistant text before tools".into());
                    }
                    text.push_str(part);
                }
                "tool_use" => {
                    only(block, &["type", "id", "name", "input", "cache_control"], "tool_use")?;
                    tools_started = true;
                    let id = nonempty(block, "id")?;
                    let name = nonempty(block, "name")?;
                    if name.len() > 128 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
                        return Err("Invalid tool name in imported history".into());
                    }
                    let input = block.get("input").ok_or("History tool_use.input is required")?;
                    object(input, "tool input")?;
                    if !self.seen.insert(id.into()) {
                        return Err("Duplicate tool_use ID in imported history".into());
                    }
                    let name = model_tool_name(name);
                    self.push("tool_start", message_id, json!({"tool_use_id":id,"tool_name":name}));
                    self.push("tool_arguments", message_id, json!({"tool_use_id":id,"arguments":input}));
                    self.pending.insert(id.into(), Call { message_id:message_id.into(), name });
                }
                "thinking" | "redacted_thinking" => return Err("Quick history import cannot faithfully replay signed or redacted reasoning blocks".into()),
                _ => return Err("Unsupported assistant block in imported history".into()),
            }
        }
        if text.is_empty() && !tools_started {
            return Err("Imported assistant message has no text or tools".into());
        }
        self.push(
            "message_assistant_complete",
            message_id,
            json!({"full_content":text}),
        );
        Ok(())
    }

    fn user(&mut self, message_id: &str, blocks: &[Value]) -> Result<(), String> {
        let mut text = String::new();
        let mut saw_text = false;
        let mut results = 0;
        for value in blocks {
            let block = object(value, "user block")?;
            match required_text(block, "type")? {
                "text" => {
                    only(block, &["type", "text", "cache_control"], "text block")?;
                    let part = required_text(block, "text")?;
                    saw_text |= !part.is_empty();
                    text.push_str(part);
                }
                "tool_result" => {
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
                    if saw_text {
                        return Err("Cannot faithfully import user text before tool results; Quick history places tool results first".into());
                    }
                    let id = nonempty(block, "tool_use_id")?;
                    let call = self
                        .pending
                        .remove(id)
                        .ok_or("History tool_result has no unresolved matching tool_use")?;
                    let content = result_blocks(block.get("content"))?;
                    let is_error = match block.get("is_error") {
                        None => false,
                        Some(Value::Bool(value)) => *value,
                        _ => return Err("History tool_result.is_error must be boolean".into()),
                    };
                    let status = if is_error { "error" } else { "success" };
                    let fallback: String =
                        content.iter().filter_map(|v| v["text"].as_str()).collect();
                    // Store both the ordinary result and its official model
                    // projection. A raw MCP object here would become Python
                    // repr text and would lose isError and text boundaries.
                    self.push(
                        "tool_result",
                        &call.message_id,
                        json!({
                            "tool_use_id":id,"tool_name":call.name,"result":fallback,"status":status
                        }),
                    );
                    self.push(
                        "tool_result_projection",
                        &call.message_id,
                        json!({
                            "tool_use_id":id,"tool_name":call.name,"result":fallback,
                            "content":content,"status":status
                        }),
                    );
                    results += 1;
                }
                _ => return Err("Unsupported user block in imported history".into()),
            }
        }
        if !self.pending.is_empty() {
            return Err("Imported user history is missing one or more tool results".into());
        }
        if !text.is_empty() {
            self.push(
                "message_user",
                message_id,
                json!({"content":text,"attachments":[]}),
            );
        } else if results == 0 {
            return Err("Imported user message has no text or tool results".into());
        }
        Ok(())
    }
}

fn result_blocks(value: Option<&Value>) -> Result<Vec<Value>, String> {
    let blocks = match value {
        None => Vec::new(),
        Some(Value::String(text)) => vec![json!({"text":text})],
        Some(Value::Array(values)) => {
            let mut blocks = Vec::new();
            for value in values {
                let block = object(value, "tool result content")?;
                only(block, &["type", "text"], "tool result content")?;
                if required_text(block, "type")? != "text" {
                    return Err("History import supports text tool results only".into());
                }
                blocks.push(json!({"text":required_text(block,"text")?}));
            }
            blocks
        }
        _ => return Err("History tool result content must be text or text blocks".into()),
    };
    // Empty projections fall back to the ordinary result. A blank text block
    // retains an empty output without adding an invented success message.
    Ok(if blocks.is_empty() {
        vec![json!({"text":""})]
    } else {
        blocks
    })
}

fn identity(prefix: &str, thread: &str, value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(thread.as_bytes());
    hash.update([0]);
    hash.update(value.as_bytes());
    format!("{prefix}-{}", &format!("{:x}", hash.finalize())[..32])
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("History {label} must be an object"))
}

fn only(value: &Map<String, Value>, keys: &[&str], label: &str) -> Result<(), String> {
    if let Some(key) = value.keys().find(|key| !keys.contains(&key.as_str())) {
        return Err(format!("Unsupported history {label} field: {key}"));
    }
    Ok(())
}

fn required_text<'a>(value: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("History {key} must be text"))
}

fn nonempty<'a>(value: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    let text = required_text(value, key)?;
    if text.is_empty() {
        Err(format!("History {key} must not be empty"))
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_history() -> Vec<Value> {
        vec![
            json!({"role":"user","content":"Run it."}),
            json!({"role":"assistant","content":[{"type":"text","text":"Checking."},{"type":"tool_use","id":"call-1","name":"read-file","input":{"path":"a.txt","nested":[true,null,3]}}]}),
            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"call-1","content":[{"type":"text","text":"first\n"},{"type":"text","text":"second"}],"is_error":true},{"type":"text","text":"Explain it."}]}),
            json!({"role":"assistant","content":"Done."}),
        ]
    }

    #[test]
    fn text_history_has_correct_roles_full_content_and_stable_ids() {
        let input = vec![
            json!({"role":"user","content":"a\nb"}),
            json!({"role":"assistant","content":[{"type":"text","text":"Hello"},{"type":"text","text":" world"}]}),
        ];
        let output = events(&input, "thread-1").unwrap();
        assert_eq!(output, events(&input, "thread-1").unwrap());
        assert_ne!(
            output[0]["id"],
            events(&input, "thread-2").unwrap()[0]["id"]
        );
        assert_eq!(output[0]["data"]["content"], "a\nb");
        assert_eq!(output[2]["data"], json!({"full_content":"Hello world"}));
        assert_eq!(output[1]["message_id"], output[2]["message_id"]);
        assert!(output.iter().all(|e| e.get("timestamp").is_none()));
        assert_eq!(
            output
                .iter()
                .map(|e| e["id"].as_str().unwrap())
                .collect::<BTreeSet<_>>()
                .len(),
            output.len()
        );
    }

    #[test]
    fn tools_preserve_identity_input_result_blocks_status_and_user_text() {
        let output = events(&tool_history(), "thread").unwrap();
        let start = output.iter().find(|e| e["type"] == "tool_start").unwrap();
        assert_eq!(start["data"]["tool_name"], model_tool_name("read-file"));
        let args = output
            .iter()
            .find(|e| e["type"] == "tool_arguments")
            .unwrap();
        assert_eq!(
            args["data"]["arguments"],
            json!({"path":"a.txt","nested":[true,null,3]})
        );
        let result = output
            .iter()
            .find(|e| e["type"] == "tool_result_projection")
            .unwrap();
        assert_eq!(result["message_id"], start["message_id"]);
        assert_eq!(result["data"]["tool_use_id"], "call-1");
        assert_eq!(
            result["data"]["content"],
            json!([{"text":"first\n"},{"text":"second"}])
        );
        assert_eq!(result["data"]["status"], "error");
        assert!(
            output
                .iter()
                .all(|e| e["data"].get("approval_source").is_none())
        );
        assert!(
            output
                .iter()
                .any(|e| e["type"] == "message_user" && e["data"]["content"] == "Explain it.")
        );
    }

    #[test]
    fn empty_tool_output_stays_empty() {
        let mut input = tool_history();
        input[2]["content"] = json!([{"type":"tool_result","tool_use_id":"call-1"}]);
        let output = events(&input, "thread").unwrap();
        let result = output
            .iter()
            .find(|e| e["type"] == "tool_result_projection")
            .unwrap();
        assert_eq!(result["data"]["content"], json!([{"text":""}]));
        assert_eq!(result["data"]["result"], "");
        assert_eq!(result["data"]["status"], "success");
    }

    #[test]
    fn model_tool_names_match_official_sanitizer_and_truncation_fixtures() {
        assert_eq!(
            model_tool_name("fixture_echo"),
            "QuickWay_API_session_quickway__519bd67e66cde77c_fixture_echo"
        );
        assert_eq!(
            model_tool_name(&"x".repeat(128)),
            "t_f30692e9_n_quickway__24da1b81d0b16df6_xxxxxxxxxxxxxxxxxxxxxxxx"
        );
        for size in 1..=128 {
            assert!(model_tool_name(&"x".repeat(size)).len() <= 64);
        }
    }

    #[test]
    fn unresolved_duplicate_and_orphan_tools_are_rejected() {
        let input = tool_history();
        assert!(
            events(&input[..2], "thread")
                .unwrap_err()
                .contains("unresolved")
        );
        let mut missing = input.clone();
        missing[2]["content"] = json!([{"type":"text","text":"Missing output"}]);
        assert!(events(&missing, "thread").is_err());
        let mut orphan = input.clone();
        orphan[2]["content"][0]["tool_use_id"] = "wrong".into();
        assert!(events(&orphan, "thread").is_err());
        let mut duplicate = input;
        let repeated = [duplicate[1].clone(), duplicate[2].clone()];
        duplicate.extend(repeated);
        assert!(
            events(&duplicate, "thread")
                .unwrap_err()
                .contains("Duplicate")
        );
    }

    #[test]
    fn reordered_text_and_unsupported_blocks_are_never_silently_dropped() {
        let mut input = tool_history();
        input[1]["content"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"text","text":"after"}));
        assert!(
            events(&input, "thread")
                .unwrap_err()
                .contains("after a tool_use")
        );
        let mut input = tool_history();
        input[2]["content"].as_array_mut().unwrap().swap(0, 1);
        assert!(
            events(&input, "thread")
                .unwrap_err()
                .contains("before tool results")
        );
        for block in [
            json!({"type":"image","source":{}}),
            json!({"type":"thinking","thinking":"x","signature":"s"}),
            json!({"type":"redacted_thinking","data":"x"}),
        ] {
            let input = vec![
                json!({"role":"user","content":"hi"}),
                json!({"role":"assistant","content":[block]}),
            ];
            assert!(events(&input, "thread").is_err());
        }
    }

    #[test]
    fn malformed_inputs_fail_without_partial_output() {
        for input in [
            json!([{"role":"assistant","content":"x"}]),
            json!([{"role":"user","content":null}]),
            json!([{"role":"user","content":[]}]),
            json!([{"role":"user","content":"x","hidden":"ignored?"}]),
        ] {
            assert!(events(input.as_array().unwrap(), "thread").is_err());
        }
        assert_eq!(events(&[], "thread").unwrap(), Vec::<Value>::new());
        assert!(events(&[], "").is_err());
    }
}
