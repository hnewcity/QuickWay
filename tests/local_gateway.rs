//! Black-box coverage of the CLI and its production router, without Quick credentials or quota.
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    net::SocketAddr,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn signed_out_proxy_serves_its_announced_address_and_checks_local_keys() {
    let dir = tempfile::tempdir().unwrap();
    let key_file = dir.path().join("proxy.key");
    let mut server = Server(
        Command::new(env!("CARGO_BIN_EXE_quickway"))
            .arg("--quick-base")
            .arg(dir.path())
            .args(["serve", "--listen", "127.0.0.1:0", "--key-file"])
            .arg(&key_file)
            .env_remove("QUICKWAY_API_KEY")
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stderr = server.0.stderr.take().unwrap();
    let (send, recv) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(BufReader::new(stderr).lines().next());
    });
    let line = recv
        .recv_timeout(Duration::from_secs(60))
        .expect("proxy startup timed out")
        .expect("proxy exited before announcing a listener")
        .unwrap();
    let (address, _) = line
        .strip_prefix("QuickWay listening on http://")
        .unwrap()
        .split_once(';')
        .unwrap();
    let bound: SocketAddr = address.parse().unwrap();
    assert!(bound.ip().is_loopback());
    assert_ne!(bound.port(), 0, "must announce the actual bound port");
    let base = format!("http://{address}");
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let key = std::fs::read_to_string(key_file).unwrap();
    assert!(key.len() >= 16);

    let health: Value = client
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        health,
        json!({"status":"ok", "service":"quickway", "version":env!("CARGO_PKG_VERSION")})
    );
    for (method, path) in [
        (Method::GET, "/v1/models"),
        (Method::GET, "/quickway/usage"),
        (Method::POST, "/v1/messages"),
        (Method::POST, "/v1/messages/count_tokens"),
        (Method::POST, "/v1/chat/completions"),
        (Method::POST, "/v1/responses"),
    ] {
        let mut request = client.request(method.clone(), format!("{base}{path}"));
        if method == Method::POST {
            request = request.json(&json!({}));
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(
            body["error"]["message"], "Invalid QuickWay API key",
            "{path}"
        );
    }
    let messages = json!({"messages":[{"role":"user", "content":"你好"}]});
    for (name, value) in [
        ("x-api-key", key.clone()),
        ("authorization", format!("Bearer {key}")),
        ("authorization", format!("bEaReR   {key}")),
    ] {
        let response = client
            .post(format!("{base}/v1/messages/count_tokens"))
            .header(name, value)
            .json(&messages)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-quickway-token-count"], "estimated");
        assert!(
            response.json::<Value>().await.unwrap()["input_tokens"]
                .as_u64()
                .unwrap()
                > 0
        );
    }
    let models: Value = client
        .get(format!("{base}/v1/models"))
        .bearer_auth(&key)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(models["data"], json!([]));
    assert!(models["quickway_catalog"]["error"].is_string());
    for (path, input) in [
        (
            "/v1/messages",
            json!({"messages":messages["messages"], "max_tokens":32}),
        ),
        ("/v1/chat/completions", messages.clone()),
        ("/v1/responses", json!({"input":"你好"})),
    ] {
        for stream in [true, false] {
            let mut body = input.clone();
            body["model"] = "quick-smart".into();
            body["stream"] = stream.into();
            let response = client
                .post(format!("{base}{path}"))
                .bearer_auth(&key)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{path}, stream={stream}"
            );
            let body: Value = response.json().await.unwrap();
            assert!(
                body["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("sign in to Amazon Quick")
            );
        }
    }
}
