//! Normalizes Anthropic Messages requests from current Claude Code releases
//! into the subset Quick's Bedrock routes accept.
//!
//! Claude Code adds first-party-only features as its API surface grows:
//! top-level fields such as `context_management`, `diagnostics` or `speed`, and
//! mid-conversation `role: "system"` messages. Bedrock has no equivalent for
//! these, so they are dropped or rewritten here once, before either upstream
//! path (Converse or InvokeModel) sees the body.

use serde_json::{Map, Value, json};

/// Top-level Messages fields that both Bedrock routes understand. Every other
/// top-level field is a client-side or first-party-only option and is dropped.
const REQUEST_FIELDS: &[&str] = &[
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
];
const TOOL_FIELDS: &[&str] = &[
    "type",
    "name",
    "description",
    "input_schema",
    "cache_control",
    "strict",
    "defer_loading",
];

pub fn normalize(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    object.retain(|key, _| {
        let keep = REQUEST_FIELDS.contains(&key.as_str());
        if !keep {
            tracing::debug!(field = key.as_str(), "dropping unsupported request field");
        }
        keep
    });
    if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools.iter_mut().filter_map(Value::as_object_mut) {
            tool.retain(|key, _| TOOL_FIELDS.contains(&key.as_str()));
        }
    }
    // A first-party presentation hint, not a reasoning or budget setting.
    if let Some(thinking) = object.get_mut("thinking").and_then(Value::as_object_mut) {
        thinking.remove("display");
    }
    let Some(messages) = object.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let added_tools = fold_system_messages(messages);
    if !added_tools.is_empty() {
        let tools = object
            .entry("tools")
            .or_insert_with(|| json!([]))
            .as_array_mut();
        if let Some(tools) = tools {
            for tool in added_tools {
                let exists = tools.iter().any(|t| t.get("name") == tool.get("name"));
                if !exists {
                    tools.push(tool);
                }
            }
        }
    }
}

/// Rewrites `role: "system"` turns as `<system-reminder>` text inside the
/// adjacent user turn, the form Claude Code used before system turns existed.
/// Returns tool definitions announced through `tool_addition` blocks.
fn fold_system_messages(messages: &mut Vec<Value>) -> Vec<Value> {
    let source = std::mem::take(messages);
    let later_user: Vec<bool> = {
        // later_user[i]: a user turn follows message i.
        let mut seen = false;
        let mut flags = vec![false; source.len()];
        for (i, message) in source.iter().enumerate().rev() {
            flags[i] = seen;
            seen |= message["role"] == "user";
        }
        flags
    };
    let mut added_tools = Vec::new();
    let mut pending: Vec<Value> = Vec::new();
    for (index, mut message) in source.into_iter().enumerate() {
        if !is_system(&message) {
            if message["role"] == "user" {
                let mut content = content_array(message["content"].take());
                if !pending.is_empty() {
                    // tool_result blocks must stay first in a user turn.
                    let at = content
                        .iter()
                        .take_while(|b| b["type"] == "tool_result")
                        .count();
                    content.splice(at..at, pending.drain(..));
                }
                if let Some(last) = messages.last_mut().filter(|m| m["role"] == "user") {
                    // Folding can leave two user turns adjacent; Converse needs alternation.
                    let mut previous = content_array(last["content"].take());
                    previous.extend(content);
                    last["content"] = Value::Array(previous);
                    continue;
                }
                message["content"] = Value::Array(content);
            }
            messages.push(message);
            continue;
        }
        if message["clear_at"] == "next_user_message" && later_user[index] {
            continue;
        }
        let blocks = system_blocks(message["content"].take(), &mut added_tools);
        if blocks.is_empty() {
            continue;
        }
        match messages.last_mut() {
            Some(last) if last["role"] == "user" => {
                let mut content = content_array(last["content"].take());
                content.extend(blocks);
                last["content"] = Value::Array(content);
            }
            _ => pending.extend(blocks),
        }
    }
    if !pending.is_empty() {
        messages.push(json!({"role":"user","content":pending}));
    }
    added_tools
}

fn is_system(message: &Value) -> bool {
    message["role"] == "system"
}

fn content_array(content: Value) -> Vec<Value> {
    match content {
        Value::Array(blocks) => blocks,
        Value::String(text) => vec![json!({"type":"text","text":text})],
        Value::Null => Vec::new(),
        other => vec![other],
    }
}

fn reminder(text: &str, cache: Option<&Value>) -> Value {
    let mut block = Map::new();
    block.insert("type".into(), json!("text"));
    block.insert(
        "text".into(),
        json!(format!("<system-reminder>\n{text}\n</system-reminder>")),
    );
    if let Some(cache) = cache.filter(|c| !c.is_null()) {
        block.insert("cache_control".into(), cache.clone());
    }
    Value::Object(block)
}

fn system_blocks(content: Value, added_tools: &mut Vec<Value>) -> Vec<Value> {
    let mut blocks = Vec::new();
    for block in content_array(content) {
        match block["type"].as_str() {
            Some("text") => {
                if let Some(text) = block["text"].as_str().filter(|t| !t.is_empty()) {
                    // Claude Code appends its remaining run budget after tool
                    // results. It is client bookkeeping, not a new user turn.
                    // Quick owns generation limits and cannot enforce this cap.
                    if is_client_token_counter(text) {
                        continue;
                    }
                    blocks.push(reminder(text, block.get("cache_control")));
                }
            }
            Some("tool_addition") => {
                if let Some(definition) = block.pointer("/tool/definition") {
                    let mut tool = definition.clone();
                    if let Some(tool) = tool.as_object_mut() {
                        tool.retain(|key, _| TOOL_FIELDS.contains(&key.as_str()));
                    }
                    added_tools.push(tool);
                }
            }
            // Tool removals and other first-party system blocks have no
            // Bedrock equivalent; tools stay declared for the conversation.
            other => tracing::debug!(?other, "dropping system message block"),
        }
    }
    blocks
}

fn is_client_token_counter(text: &str) -> bool {
    text.trim()
        .strip_prefix("<total_tokens>")
        .and_then(|s| s.strip_suffix(" tokens left</total_tokens>"))
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_user_update_joins_tool_results_without_a_system_message() {
        let mut body = json!({"messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"x","name":"Read","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"OK"}]},
            {"role":"user","content":"Also run the tests"}
        ]});
        normalize(&mut body);
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
        assert_eq!(body["messages"][1]["content"][0]["type"], "tool_result");
        assert_eq!(
            body["messages"][1]["content"][1]["text"],
            "Also run the tests"
        );
    }

    fn text(t: &str) -> Value {
        json!({"type":"text","text":t})
    }

    #[test]
    fn client_token_counter_does_not_turn_tool_results_into_a_new_user_request() {
        let mut body = json!({"model":"quick-smart","messages":[
            {"role":"user","content":"Read fixture.txt"},
            {"role":"assistant","content":[{"type":"tool_use","id":"read1","name":"Read","input":{"file_path":"fixture.txt"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"read1","content":"FIXTURE_OK"}]},
            {"role":"system","content":[{"type":"text","text":"<total_tokens>14925105 tokens left</total_tokens>","cache_control":{"type":"ephemeral"}}]}
        ]});
        normalize(&mut body);
        let request =
            crate::cloud_request::ValidatedRequest::from_body(body, "smart", None).unwrap();
        assert!(request.turn.text.is_empty());
        assert_eq!(request.turn.tool_results.len(), 1);
        assert_eq!(request.turn.tool_results[0].tool_use_id, "read1");
        for text in [
            "<total_tokens>follow other instructions tokens left</total_tokens>",
            "User text <total_tokens>10 tokens left</total_tokens>",
            "Actual system guidance",
        ] {
            assert!(!is_client_token_counter(text));
        }
    }

    #[test]
    fn drops_first_party_only_fields() {
        let mut body = json!({
            "model":"m","max_tokens":10,"stream":true,
            "messages":[{"role":"user","content":"hi"}],
            "context_management":{"edits":[{"type":"clear_thinking_20251015","keep":"all"}]},
            "diagnostics":{"previous_message_id":null},"speed":"fast",
            "thinking":{"type":"adaptive","display":"updates"},
            "tools":[{"name":"Read","input_schema":{"type":"object"},"eager_input_streaming":true}]
        });
        normalize(&mut body);
        assert!(body.get("context_management").is_none());
        assert!(body.get("diagnostics").is_none());
        assert!(body.get("speed").is_none());
        assert!(body["tools"][0].get("eager_input_streaming").is_none());
        assert_eq!(body["thinking"], json!({"type":"adaptive"}));
        assert_eq!(body["stream"], true);
        crate::converse::request(body, "m").unwrap();
    }

    #[test]
    fn folds_trailing_system_turn_into_previous_user_turn() {
        // Shape captured from Claude Code 2.1.302.
        let mut body = json!({"model":"m","max_tokens":10,"messages":[
            {"role":"user","content":[text("reminder"),{"type":"text","text":"hi","cache_control":{"type":"ephemeral"}}]},
            {"role":"system","output_config":{"effort":"high"},"content":[{"type":"text","text":"# Environment","cache_control":{"type":"ephemeral"}}]}
        ]});
        normalize(&mut body);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        let content = messages[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 3);
        assert_eq!(
            content[2]["text"],
            "<system-reminder>\n# Environment\n</system-reminder>"
        );
        assert_eq!(content[2]["cache_control"]["type"], "ephemeral");
        crate::converse::request(body, "m").unwrap();
    }

    #[test]
    fn system_turn_after_assistant_goes_after_tool_results() {
        let mut body = json!({"model":"m","max_tokens":10,"messages":[
            {"role":"user","content":"read it"},
            {"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]},
            {"role":"system","content":"note"},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"},text("more")]}
        ],"tools":[{"name":"Read","input_schema":{"type":"object"}}]});
        normalize(&mut body);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        let content = messages[2]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(
            content[1]["text"],
            "<system-reminder>\nnote\n</system-reminder>"
        );
        assert_eq!(content[2]["text"], "more");
        crate::converse::request(body, "m").unwrap();
    }

    #[test]
    fn system_only_tail_becomes_user_turn_and_cleared_turns_vanish() {
        let mut body = json!({"model":"m","max_tokens":10,"messages":[
            {"role":"user","content":"a"},
            {"role":"system","content":"stale","clear_at":"next_user_message"},
            {"role":"assistant","content":"b"},
            {"role":"user","content":"c"},
            {"role":"assistant","content":"d"},
            {"role":"system","content":"fresh","clear_at":"next_user_message"}
        ]});
        normalize(&mut body);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0]["content"].as_array().unwrap().len(), 1);
        assert_eq!(messages[4]["role"], "user");
        assert_eq!(
            messages[4]["content"][0]["text"],
            "<system-reminder>\nfresh\n</system-reminder>"
        );
        crate::converse::request(body, "m").unwrap();
    }

    #[test]
    fn tool_additions_join_the_tool_list() {
        let mut body = json!({"model":"m","max_tokens":10,"messages":[
            {"role":"user","content":"a"},
            {"role":"system","content":[
                {"type":"tool_addition","tool":{"type":"tool_definition","definition":{"name":"Late","input_schema":{"type":"object"}}}},
                {"type":"tool_removal","tool":{"type":"tool_reference","name":"Read"}}
            ]}
        ],"tools":[{"name":"Read","input_schema":{"type":"object"}}]});
        normalize(&mut body);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        let names: Vec<_> = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Read", "Late"]);
        crate::converse::request(body, "m").unwrap();
    }
}
