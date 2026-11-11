use super::*;

fn tools_spec() -> Value {
    json!([{"type":"function","function":{"name":"echo","description":"Echo","parameters":{"type":"object","properties":{"text":{"type":"string"}}}}}])
}

#[test]
fn chat_history_merges_tool_results_and_passes_converse_validation() {
    let request = json!({"model":"gpt-5.6-sol","stream":true,"stream_options":{"include_usage":true},
        "max_tokens":100,"temperature":0.2,"stop":"END","reasoning_effort":"medium",
        "messages":[
            {"role":"system","content":"Be brief."},
            {"role":"developer","content":[{"type":"text","text":"Use tools."}]},
            {"role":"user","content":[{"type":"text","text":"Hi"},{"type":"image_url","image_url":{"url":"data:image/png;base64,aGVsbG8="}}]},
            {"role":"assistant","content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"echo","arguments":"{\"text\":\"a\"}"}},
                {"id":"call_2","type":"function","function":{"name":"echo","arguments":""}}]},
            {"role":"tool","tool_call_id":"call_1","content":"A"},
            {"role":"tool","tool_call_id":"call_2","content":[{"type":"text","text":"B"}]},
            {"role":"user","content":"Done?"}],
        "tools":tools_spec(),"tool_choice":"required","parallel_tool_calls":true,"user":"ignored","seed":1});
    let c = chat_request(&request).unwrap();
    assert!(c.stream && c.include_usage);
    assert_eq!(c.effort, Some(Effort::Medium));
    let b = &c.body;
    assert_eq!(
        b["system"],
        json!([{"type":"text","text":"Be brief."},{"type":"text","text":"Use tools."}])
    );
    assert_eq!(b["stop_sequences"], json!(["END"]));
    assert_eq!(b["tool_choice"], json!({"type":"any"}));
    let messages = b["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[0]["content"][1]["source"]["media_type"],
        "image/png"
    );
    assert_eq!(messages[1]["content"][1]["input"], json!({}));
    let results = messages[2]["content"].as_array().unwrap();
    assert_eq!(results[0]["tool_use_id"], "call_1");
    assert_eq!(results[1]["content"], "B");
    assert_eq!(results[2]["text"], "Done?");
    crate::converse::request(c.body, "us.openai.gpt-5.6-sol").unwrap();
}

#[test]
fn chat_options_without_equivalent_are_rejected_or_mapped() {
    let base = json!({"model":"m","messages":[{"role":"user","content":"x"}]});
    let c = chat_request(&base).unwrap();
    assert_eq!(c.body["max_tokens"], DEFAULT_MAX_TOKENS);
    for (field, value) in [
        ("n", json!(2)),
        ("logprobs", json!(true)),
        ("presence_penalty", json!(0.5)),
    ] {
        let mut r = base.clone();
        r[field] = value;
        assert!(chat_request(&r).is_err(), "{field}");
    }
    let mut r = base.clone();
    r["n"] = json!(1);
    r["frequency_penalty"] = json!(0);
    assert!(chat_request(&r).is_ok());
    r["tools"] = tools_spec();
    r["tool_choice"] = json!("none");
    assert!(chat_request(&r).unwrap().body.get("tools").is_none());
    r["tool_choice"] = json!({"type":"function","function":{"name":"echo"}});
    r["parallel_tool_calls"] = json!(false);
    assert_eq!(
        chat_request(&r).unwrap().body["tool_choice"],
        json!({"type":"tool","name":"echo","disable_parallel_tool_use":true})
    );
    r["response_format"] =
        json!({"type":"json_schema","json_schema":{"name":"x","schema":{"type":"object"}}});
    assert_eq!(
        chat_request(&r).unwrap().body["output_config"]["format"]["type"],
        "json_schema"
    );
    r["response_format"] = json!({"type":"json_object"});
    assert!(chat_request(&r).is_err());
    let mut r = base.clone();
    r["messages"][0]["content"] =
        json!([{"type":"image_url","image_url":{"url":"https://example.invalid/a.png"}}]);
    assert!(chat_request(&r).is_err());
}

#[test]
fn chat_response_maps_tools_reasoning_usage_and_finish_reason() {
    let message = json!({"model":"claude-sonnet-4-6","stop_reason":"tool_use","content":[
        {"type":"thinking","thinking":"hmm","signature":"s"},
        {"type":"tool_use","id":"toolu_1","name":"echo","input":{"text":"a"}}],
        "usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":4}});
    let r = chat_response(&message);
    let choice = &r["choices"][0];
    assert_eq!(choice["finish_reason"], "tool_calls");
    assert!(choice["message"]["content"].is_null());
    assert_eq!(choice["message"]["reasoning_content"], "hmm");
    assert_eq!(
        choice["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"text\":\"a\"}"
    );
    assert_eq!(
        r["usage"],
        json!({"prompt_tokens":14,"completion_tokens":5,"total_tokens":19,"prompt_tokens_details":{"cached_tokens":4}})
    );
}

fn anthropic_stream() -> Vec<Value> {
    vec![
        json!({"type":"message_start","message":{"model":"claude-sonnet-4-6"}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"plan"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"SIG"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hel"}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"lo"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"now","input":{}}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":3,"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ]
}

#[test]
fn chat_stream_emits_openai_chunks_usage_and_done() {
    let mut t = ChatStream::new(true);
    let data: Vec<String> = anthropic_stream().iter().flat_map(|e| t.event(e)).collect();
    assert_eq!(data.last().unwrap(), "[DONE]");
    let chunks: Vec<Value> = data[..data.len() - 1]
        .iter()
        .map(|d| serde_json::from_str(d).unwrap())
        .collect();
    assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
    let text: String = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"]["content"].as_str())
        .collect();
    assert_eq!(text, "Hello");
    let tool: Vec<&Value> = chunks
        .iter()
        .filter_map(|c| c["choices"][0]["delta"].get("tool_calls"))
        .collect();
    assert_eq!(tool[0][0]["id"], "toolu_1");
    assert_eq!(tool[1][0]["function"]["arguments"], "{}");
    let finish = chunks
        .iter()
        .find_map(|c| c["choices"][0]["finish_reason"].as_str())
        .unwrap();
    assert_eq!(finish, "tool_calls");
    let last = chunks.last().unwrap();
    assert_eq!(last["choices"], json!([]));
    assert_eq!(last["usage"]["total_tokens"], 5);

    let mut t = ChatStream::new(false);
    t.event(&anthropic_stream()[0]);
    let error = t.event(&json!({"type":"error","error":{"message":"boom"}}));
    assert!(error[0].contains("boom"));
    assert!(t.event(&json!({"type":"message_stop"})).is_empty());
}

#[test]
fn responses_input_replays_signed_reasoning_and_tool_outputs() {
    let request = json!({"model":"claude-sonnet-4-6","instructions":"Be brief.","max_output_tokens":200,
        "reasoning":{"effort":"high","summary":"auto"},"store":false,"include":["reasoning.encrypted_content"],
        "input":[
            {"role":"user","content":"What time is it?"},
            {"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"plan"}],"encrypted_content":"quickway:sig:SIG"},
            {"type":"reasoning","id":"rs_2","summary":[],"encrypted_content":"opaque-from-elsewhere"},
            {"type":"function_call","id":"fc_1","call_id":"toolu_1","name":"now","arguments":"{}"},
            {"type":"function_call_output","call_id":"toolu_1","output":"12:00"},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"Thanks"}]}],
        "tools":[{"type":"function","name":"now","parameters":{"type":"object","properties":{}}}],
        "text":{"format":{"type":"json_schema","name":"r","schema":{"type":"object"}}}});
    let r = responses_request(&request).unwrap();
    assert_eq!(r.converted.effort, Some(Effort::High));
    let b = &r.converted.body;
    assert_eq!(b["max_tokens"], 200);
    assert_eq!(b["system"][0]["text"], "Be brief.");
    assert_eq!(
        b["messages"][1]["content"],
        json!([
            {"type":"thinking","thinking":"plan","signature":"SIG"},
            {"type":"tool_use","id":"toolu_1","name":"now","input":{}}])
    );
    assert_eq!(b["messages"][2]["content"][0]["content"], "12:00");
    assert_eq!(b["output_config"]["format"]["type"], "json_schema");
    assert_eq!(r.echo["reasoning"]["effort"], "high");
    crate::converse::request(r.converted.body, "global.anthropic.claude-sonnet-4-6").unwrap();

    for field in ["previous_response_id", "conversation"] {
        let mut bad = json!({"model":"m","input":"x"});
        bad[field] = json!("id");
        assert!(responses_request(&bad).is_err());
    }
    let bad = json!({"model":"m","input":"x","tools":[{"type":"file_search"}]});
    assert!(responses_request(&bad).is_err());
    // Server-executed search is dropped rather than failing the request.
    let searched =
        json!({"model":"m","input":"x","tools":[{"type":"web_search","external_web_access":true}]});
    assert!(responses_request(&searched).unwrap().converted.body["tools"].is_null());
}

/// Tool shapes taken from a captured Codex CLI 0.167 request.
fn codex_tools() -> Value {
    json!([
        {"type":"function","name":"exec_command","description":"Run a command","strict":false,
            "parameters":{"type":"object","properties":{"cmd":{"type":"string"}},"required":["cmd"]}},
        {"type":"namespace","name":"multi_agent_v1","description":"Agents","tools":[
            {"type":"function","name":"spawn_agent","strict":false,"parameters":{"type":"object","properties":{}}}]},
        {"type":"namespace","name":"mcp__docs_","description":"MCP","tools":[
            {"type":"function","name":"search","parameters":{"type":"object","properties":{}}}]},
        {"type":"custom","name":"apply_patch","description":"Edit files",
            "format":{"type":"grammar","syntax":"lark","definition":"start: begin_patch"}},
        {"type":"web_search","external_web_access":true}])
}

#[test]
fn responses_accepts_codex_namespace_and_custom_tools() {
    let request = json!({"model":"claude-sonnet-5-5","stream":true,"tools":codex_tools(),
        "tool_choice":"auto","parallel_tool_calls":true,"reasoning":{"effort":"max","summary":"auto"},
        "store":false,"include":["reasoning.encrypted_content"],"prompt_cache_key":"k","text":{"verbosity":"low"},
        "input":[
            {"type":"message","role":"developer","content":[{"type":"input_text","text":"Be careful."}]},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"Fix it"}]},
            {"type":"custom_tool_call","call_id":"toolu_1","name":"apply_patch","input":"*** Begin Patch\n*** End Patch"},
            {"type":"custom_tool_call_output","call_id":"toolu_1","output":"Done"},
            {"type":"function_call","call_id":"toolu_2","name":"spawn_agent","namespace":"multi_agent_v1","arguments":"{}"},
            {"type":"function_call_output","call_id":"toolu_2","output":[{"type":"input_text","text":"ok"}]}]});
    let r = responses_request(&request).unwrap();
    assert_eq!(r.converted.effort, Some(Effort::High));
    let b = &r.converted.body;
    let names: Vec<&str> = b["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "exec_command",
            "multi_agent_v1__spawn_agent",
            "mcp__docs_search",
            "apply_patch"
        ]
    );
    let patch = &b["tools"][3];
    assert_eq!(patch["input_schema"]["required"], json!(["input"]));
    assert!(
        patch["description"]
            .as_str()
            .unwrap()
            .contains("start: begin_patch")
    );
    assert_eq!(b["system"][0]["text"], "Be careful.");
    let calls = &b["messages"][1]["content"];
    assert_eq!(calls[0]["name"], "apply_patch");
    assert_eq!(calls[0]["input"]["input"], "*** Begin Patch\n*** End Patch");
    assert_eq!(b["messages"][2]["content"][0]["tool_use_id"], "toolu_1");
    assert_eq!(
        b["messages"][3]["content"][0]["name"],
        "multi_agent_v1__spawn_agent"
    );
    crate::converse::request(
        r.converted.body.clone(),
        "global.anthropic.claude-sonnet-5-5",
    )
    .unwrap();

    // Upstream calls map back to Codex's own item types.
    let message = json!({"model":"m","stop_reason":"tool_use","usage":{},"content":[
        {"type":"tool_use","id":"toolu_3","name":"apply_patch","input":{"input":"*** Begin Patch"}},
        {"type":"tool_use","id":"toolu_4","name":"multi_agent_v1__spawn_agent","input":{}},
        {"type":"tool_use","id":"toolu_5","name":"exec_command","input":{"cmd":"ls"}}]});
    let out = &responses_response(&message, &r.echo, &r.tools)["output"];
    assert_eq!(
        out[0],
        json!({"id":out[0]["id"],"type":"custom_tool_call","status":"completed","call_id":"toolu_3",
            "name":"apply_patch","input":"*** Begin Patch"})
    );
    assert_eq!(out[1]["type"], "function_call");
    assert_eq!(out[1]["name"], "spawn_agent");
    assert_eq!(out[1]["namespace"], "multi_agent_v1");
    assert!(out[2].get("namespace").is_none());
    assert_eq!(out[2]["arguments"], r#"{"cmd":"ls"}"#);
}

/// An MCP tool from Codex (node_repl's `js`) used a top-level oneOf, which
/// made Quick fail the whole request; and tool_choice=auto came with no tools.
#[test]
fn tool_schemas_drop_top_level_combinators() {
    let request = json!({"model":"m","input":"x","tool_choice":"auto","tools":[
        {"type":"function","name":"js","parameters":{"type":"object","properties":{"code":{"type":"string"}},
            "additionalProperties":false,"oneOf":[{"required":["code"]}],"anyOf":[{}]}},
        {"type":"function","name":"bare","parameters":{"properties":{}}}]});
    let body = responses_request(&request).unwrap().converted.body;
    assert_eq!(
        body["tools"][0]["input_schema"],
        json!({"type":"object","properties":{"code":{"type":"string"}},"additionalProperties":false})
    );
    assert_eq!(body["tools"][1]["input_schema"]["type"], "object");
    let none = json!({"model":"m","input":"x","tool_choice":"auto","tools":[]});
    assert!(
        responses_request(&none)
            .unwrap()
            .converted
            .body
            .get("tools")
            .is_none()
    );
    let forced = json!({"model":"m","input":"x","tool_choice":"required","tools":[]});
    assert!(responses_request(&forced).is_err());
}

/// With default features, Codex 0.167 sends its tools as an input item
/// inside the top-level `functions` namespace instead of in `tools`.
#[test]
fn responses_reads_tools_from_additional_tools_item() {
    let request = json!({"model":"gpt-5.6-luna","input":[
        {"type":"additional_tools","id":"at_1","role":"developer","tools":[
            {"type":"namespace","name":"functions","description":"","tools":[
                {"type":"custom","name":"exec","description":"Run JS"},
                {"type":"function","name":"wait","parameters":{"type":"object","properties":{}}}]},
            {"type":"namespace","name":"clock","tools":[
                {"type":"function","name":"sleep","parameters":{"type":"object","properties":{}}}]}]},
        {"type":"message","role":"user","content":[{"type":"input_text","text":"go"}]},
        {"type":"custom_tool_call","call_id":"c1","name":"exec","namespace":"functions","input":"text('hi')"},
        {"type":"custom_tool_call_output","call_id":"c1","output":"hi"}]});
    let r = responses_request(&request).unwrap();
    let b = &r.converted.body;
    let names: Vec<&str> = b["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["exec", "wait", "clock__sleep"]);
    // Codex's default parallel_tool_calls=false must not make Converse reject the request.
    let mut serial = request.clone();
    serial["parallel_tool_calls"] = json!(false);
    let body = responses_request(&serial).unwrap().converted.body;
    assert!(body.get("tool_choice").is_none());
    crate::converse::request(body, "us.openai.gpt-5.6-luna").unwrap();
    assert_eq!(b["messages"][1]["content"][0]["name"], "exec");
    assert_eq!(
        b["messages"][1]["content"][0]["input"]["input"],
        "text('hi')"
    );
    let message = json!({"model":"m","stop_reason":"tool_use","usage":{},"content":[
        {"type":"tool_use","id":"c2","name":"exec","input":{"input":"exit()"}}]});
    let out = &responses_response(&message, &r.echo, &r.tools)["output"][0];
    assert_eq!(out["type"], "custom_tool_call");
    assert_eq!(out["name"], "exec");
    assert!(out.get("namespace").is_none());
}

#[test]
fn responses_stream_emits_custom_tool_input_whole() {
    let r = responses_request(&json!({"model":"m","input":"x","tools":codex_tools()})).unwrap();
    let mut t = ResponsesStream::new(Object::new(), r.tools);
    let events: Vec<(String, Value)> = [
        json!({"type":"message_start","message":{"model":"m"}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"apply_patch","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"input\":\"*** Be"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"gin Patch\"}"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":1,"output_tokens":1}}),
        json!({"type":"message_stop"}),
    ]
    .iter()
    .flat_map(|e| t.event(e))
    .collect();
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();
    assert!(!kinds.contains(&"response.function_call_arguments.delta"));
    assert!(kinds.contains(&"response.custom_tool_call_input.done"));
    let added = &events[2].1["item"];
    assert_eq!(added["type"], "custom_tool_call");
    assert_eq!(added["input"], "");
    let done = &events
        .iter()
        .find(|(k, _)| k == "response.output_item.done")
        .unwrap()
        .1;
    assert_eq!(done["item"]["input"], "*** Begin Patch");
    assert_eq!(done["item"]["name"], "apply_patch");
    assert_eq!(kinds.last(), Some(&"response.completed"));
}

#[test]
fn flat_names_stay_valid_and_distinct() {
    assert_eq!(flat_name(Some("ns"), "tool"), "ns__tool");
    let long = "x".repeat(80);
    let a = flat_name(Some(&long), "a");
    let b = flat_name(Some(&long), "b");
    assert!(a.len() <= 64 && b.len() <= 64 && a != b);
    let dotted = flat_name(None, "a.b");
    assert!(dotted.starts_with("a_b_") && dotted != flat_name(None, "a_b"));
}

#[test]
fn responses_output_round_trips_through_input() {
    let message = json!({"model":"claude-sonnet-4-6","stop_reason":"max_tokens","content":[
        {"type":"thinking","thinking":"plan","signature":"SIG"},
        {"type":"text","text":"partial"}],"usage":{"input_tokens":1,"output_tokens":2}});
    let r = responses_response(&message, &Object::new(), &ToolNames::default());
    assert_eq!(r["status"], "incomplete");
    assert_eq!(r["incomplete_details"]["reason"], "max_output_tokens");
    assert_eq!(r["output"][1]["content"][0]["text"], "partial");
    let mut input = r["output"].as_array().unwrap().clone();
    input.insert(0, json!({"role":"user","content":"go"}));
    let c = responses_request(&json!({"model":"m","input":input})).unwrap();
    assert_eq!(
        c.converted.body["messages"][1]["content"][0]["signature"],
        "SIG"
    );

    // GPT reasoning has no signature and must not be replayed.
    let unsigned = json!({"model":"gpt","stop_reason":"end_turn","usage":{},
        "content":[{"type":"thinking","thinking":"t","signature":""}]});
    assert!(
        responses_response(&unsigned, &Object::new(), &ToolNames::default())["output"][0]["encrypted_content"].is_null()
    );
}

#[test]
fn responses_stream_emits_ordered_events_and_final_response() {
    let mut t = ResponsesStream::new(Object::new(), ToolNames::default());
    let events: Vec<(String, Value)> = anthropic_stream().iter().flat_map(|e| t.event(e)).collect();
    for (i, (kind, data)) in events.iter().enumerate() {
        assert_eq!(data["sequence_number"], i as u64);
        assert_eq!(&data["type"], kind);
    }
    let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(kinds[..2], ["response.created", "response.in_progress"]);
    assert!(kinds.contains(&"response.reasoning_summary_text.delta"));
    assert!(kinds.contains(&"response.function_call_arguments.done"));
    let (kind, last) = events.last().unwrap();
    assert_eq!(kind, "response.completed");
    let output = last["response"]["output"].as_array().unwrap();
    assert_eq!(output[0]["encrypted_content"], "quickway:sig:SIG");
    assert_eq!(output[1]["content"][0]["text"], "Hello");
    assert_eq!(output[2]["arguments"], "{}");
    assert_eq!(output[2]["call_id"], "toolu_1");
    assert_eq!(last["response"]["usage"]["total_tokens"], 5);
    let added: Vec<u64> = events
        .iter()
        .filter(|(k, _)| k == "response.output_item.added")
        .map(|(_, d)| d["output_index"].as_u64().unwrap())
        .collect();
    assert_eq!(added, [0, 1, 2]);
}
