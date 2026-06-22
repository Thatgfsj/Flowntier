//! Probe the SSE stream from MiniMax, print raw chunks so we can
//! see what the server actually sends.

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let api_key = std::env::var("MINIMAX_API_KEY")?;
    let base = std::env::var("MINIMAX_BASE_URL")
        .unwrap_or_else(|_| "https://api.minimaxi.com/v1".into());
    let model = std::env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-Text-01".into());

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 64,
        "stream": true,
        "messages": [{"role":"user","content":"用中文一句话回答：1+1=?"}],
    });

    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    eprintln!("→ POST {url}");

    let resp = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&api_key)
        .header("content-type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?;
    eprintln!("← HTTP {}", resp.status());

    use futures::StreamExt;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let s = String::from_utf8_lossy(&chunk);
        eprint!("{s}");
    }
    Ok(())
}