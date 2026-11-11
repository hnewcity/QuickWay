//! Opt-in live profile matrix. Uses only a synthetic prompt and prints no credentials.
use anyhow::{Result, anyhow, ensure};
use futures_util::{StreamExt, stream};
use quickway::{auth, converse, streaming, thinking::Effort, upstream::Upstream};
use serde_json::{Value, json};

async fn verify(upstream: &Upstream, model: &str, effort: Effort) -> Result<Value> {
    let (_, id) = upstream
        .models(false)
        .await
        .resolve(model)
        .map_err(|e| anyhow!(e))?;
    let mut body = json!({"model":model,"max_tokens":4096,"stream":true,
        "messages":[{"role":"user","content":"What is 17 * 19? Reply exactly QUICKWAY_THINKING_OK 323."}]});
    effort.apply(&mut body, model).map_err(|e| anyhow!(e))?;
    let mut request = converse::request(body, &id).map_err(|e| anyhow!(e))?;
    request.as_object_mut().unwrap().remove("modelId");
    let response = upstream
        .post(
            "/integration/quick-work/bedrock-proxy-stream",
            &json!({"modelId":id,"requestBody":request}),
        )
        .await?;
    ensure!(response.status().is_success(), "HTTP {}", response.status());
    let mut event_stream = Box::pin(streaming::event_stream(response, model.to_owned()));
    let mut events = Vec::new();
    let mut bytes = 0;
    while let Some(event) = event_stream.next().await {
        bytes += event.to_string().len();
        ensure!(bytes <= 4 * 1024 * 1024, "Stream response exceeds 4 MiB");
        events.push(event);
    }
    ensure!(
        !events.iter().any(|e| e["type"] == "error"),
        "Upstream stream error"
    );
    ensure!(
        events.last().is_some_and(|e| e["type"] == "message_stop"),
        "Missing clean stream completion"
    );
    let text: String = events
        .iter()
        .filter_map(|e| e["delta"]["text"].as_str())
        .collect();
    let reasoning = events
        .iter()
        .any(|e| e["delta"]["type"] == "thinking_delta");
    let output_tokens = events
        .iter()
        .filter_map(|e| e["usage"]["output_tokens"].as_u64())
        .max()
        .unwrap_or(0);
    ensure!(
        text.contains("QUICKWAY_THINKING_OK 323"),
        "Missing expected answer"
    );
    Ok(
        json!({"model":model,"effort":effort,"ok":true,"reasoning_blocks":reasoning,"output_tokens":output_tokens}),
    )
}

#[tokio::main]
async fn main() -> Result<()> {
    let filter = std::env::args().nth(1);
    let upstream = Upstream::new(auth::quick_base()?)?;
    let catalog = upstream.models(false).await;
    ensure!(
        catalog.source == "cloud",
        "Cloud model directory unavailable"
    );
    let profiles: Vec<_> = catalog
        .models
        .iter()
        .filter(|m| filter.as_ref().is_none_or(|s| m.id.contains(s)))
        .flat_map(|model| {
            [Effort::Off, Effort::Low, Effort::Medium, Effort::High]
                .map(|effort| (model.id.as_str(), effort))
        })
        .collect();
    let mut results = stream::iter(profiles)
        .map(|(model, effort)| {
            let upstream = &upstream;
            async move {
                match verify(upstream, model, effort).await {
                    Ok(result) => result,
                    Err(error) => {
                        json!({"model":model,"effort":effort,"ok":false,"error":error.to_string()})
                    }
                }
            }
        })
        .buffer_unordered(2);
    let mut passed = true;
    while let Some(result) = results.next().await {
        passed &= result["ok"] == true;
        println!("{result}");
    }
    ensure!(passed, "Some thinking profiles failed");
    Ok(())
}
