//! Opt-in live checks against the signed-in Quick account (uses model quota).
//! Starts an isolated loopback proxy, runs a synthetic suite, then unregisters
//! every temporary cloud device even when that suite fails.
use anyhow::{Result, ensure};
use quickway::{auth, server, upstream::Upstream};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let suite = std::env::args().nth(1).unwrap_or_else(|| "messages".into());
    let script = match suite.as_str() {
        "messages" => "scripts/smoke-test.py",
        "openai" => "scripts/smoke-openai.py",
        "history" => "scripts/smoke-cloud.py",
        "claude" | "claude-normal" => "scripts/smoke-claude.py",
        "claude-project" | "claude-edit" => "scripts/smoke-claude-project.py",
        "codex-project" | "codex-recovery" => "scripts/smoke-codex-project.py",
        _ => anyhow::bail!(
            "suite must be messages, openai, history, claude, claude-normal, claude-project, claude-edit or codex-project"
        ),
    };
    let directory = tempfile::tempdir()?;
    let key_path = directory.path().join("proxy.key");
    let key = format!("qw_{}", uuid::Uuid::new_v4());
    quickway::local_files::write_private(&key_path, key.as_bytes())?;
    let app = Arc::new(server::App {
        upstream: Upstream::new(auth::quick_base()?)?,
        key,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let mut router = server::router(app.clone());
    if suite == "codex-recovery" {
        let recovery_app = app.clone();
        // Test-only fault injection, never installed on the production router.
        router = router.route(
            "/__smoke/restart-cloud",
            axum::routing::post(move |headers: axum::http::HeaderMap| {
                let app = recovery_app.clone();
                async move {
                    if headers.get("x-api-key").and_then(|h| h.to_str().ok())
                        != Some(app.key.as_str())
                    {
                        return axum::http::StatusCode::UNAUTHORIZED;
                    }
                    app.upstream.cloud.shutdown().await;
                    axum::http::StatusCode::NO_CONTENT
                }
            }),
        );
    }
    let task = tokio::spawn(async move { axum::serve(listener, router).await });
    let mut child = tokio::process::Command::new("python");
    child
        .arg("-X")
        .arg("utf8")
        .arg(script)
        .args(["--base", &base, "--key-file"])
        .arg(key_path)
        .args(["--model", "quick-smart"]);
    if suite == "messages" {
        child.args(["--tools", "--auto-tools"]);
    }
    if suite == "claude-normal" {
        child.arg("--normal");
    }
    if suite == "claude-edit" {
        child.arg("--edit-probe");
    }
    if suite == "codex-recovery" {
        child.arg("--restart-cloud-before-result");
    }
    let result = child.status().await;
    app.upstream.cloud.shutdown().await;
    task.abort();
    ensure!(
        result?.success(),
        "cloud smoke suite failed (temporary devices have been cleaned up)"
    );
    println!("Quick cloud suite passed; connections and temporary devices cleaned up.");
    Ok(())
}
