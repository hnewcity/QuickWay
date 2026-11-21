//! Black-box the received callback boundary (not remote registration or model choice).
//! Only a TCP loopback socket, synthetic credentials, and an inert `echo` declaration.
use super::*;
use crate::{cloud_wire::tests::event_frame, openai};
use tokio::net::TcpListener;

async fn chat_choice_with_peer_events(events: Vec<Value>) -> (Value, Vec<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut peer = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let Message::Binary(req) = peer.next().await.unwrap().unwrap() else {
            panic!("expected a user turn")
        };
        let envelope: Value = serde_json::from_slice(&req).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(envelope["payload"]["payload"].as_str().unwrap())
            .unwrap();
        let event: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(event["type"], "user_message");
        let prompt = event["payload"]["custom_prompt"].as_str().unwrap();
        assert!(prompt.contains(&crate::cloud_request::model_tool_name(
            &crate::cloud_request::outpost_name("fixture-thread"),
            "echo"
        )));
        // Several EventStream frames can arrive in one WebSocket message.
        let batch = events.iter().flat_map(event_frame).collect::<Vec<_>>();
        peer.send(Message::Binary(batch.into())).await.unwrap();
    };
    let client = async {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        let session = crate::portable::tests::fixture(false);
        let mut connection = Connection {
            socket,
            session: session.clone(),
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            outpost: "local-device".into(),
            thread: "fixture-thread".into(),
            working: "local-work".into(),
            imported: false,
            registered: false,
            pending: vec![],
            backlog: VecDeque::new(),
            current_message: None,
            text_prefix: String::new(),
        };
        let converted = openai::chat_request(&json!({"model":"quick-smart", "messages":[{"role":"user","content":"Use echo."}],
            "tools":[{"type":"function","function":{"name":"echo","parameters":{"type":"object","properties":{"text":{"type":"string"}}}}}]})).unwrap();
        let request = ValidatedRequest::from_body(converted.body, "smart", None).unwrap();
        let (result, _keep_receiver) = oneshot::channel();
        let command = Command {
            request,
            result,
            events: None,
            session,
        };
        let (msg, _) = connection.turn(&command, true).await.unwrap();
        (
            openai::chat_response(&msg)["choices"][0].clone(),
            connection.pending,
        )
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(peer, client).1
    })
    .await
    .unwrap()
}

fn usage() -> Value {
    json!({"type":"turn_usage","thread_id":"fixture-thread","payload":{"input_tokens":9,"output_tokens":2}})
}

#[tokio::test]
async fn websocket_callback_roundtrips_as_one_chat_tool_call() {
    let callback = |thread: &str, id: &str, text: &str| {
        json!({"thread_id":thread,"type":"tool_call","payload":{
        "tool_name":crate::cloud_request::tool_alias("echo"), "tool_call_id":id, "arguments":{"params":{"text":text}}}})
    };
    let (choice, pending) = chat_choice_with_peer_events(vec![
        usage(),
        callback("different-thread", "foreign", "other"),
        callback("fixture-thread", "owned", "mine"),
    ])
    .await;
    assert_eq!(pending, ["owned"]);
    assert_eq!(choice["finish_reason"], "tool_calls");
    let calls = choice["message"]["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["id"], "owned");
    assert_eq!(calls[0]["function"]["name"], "echo");
    assert_eq!(
        serde_json::from_str::<Value>(calls[0]["function"]["arguments"].as_str().unwrap()).unwrap(),
        json!({"text":"mine"})
    );
}

#[tokio::test]
async fn websocket_text_without_callback_stays_an_ordinary_chat_reply() {
    let text = "echo is unavailable in this conversation.";
    let (choice, pending) = chat_choice_with_peer_events(vec![
        usage(),
        json!({"type":"message_complete","thread_id":"fixture-thread","payload":{"text":text}}),
    ])
    .await;
    assert!(pending.is_empty());
    assert_eq!(choice["finish_reason"], "stop");
    assert_eq!(choice["message"]["content"], text);
    assert!(choice["message"]["tool_calls"].is_null());
}
