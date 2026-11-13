//! Risk spike (plan Task 1, spec §10): can Rust download YouTube streams at full speed
//! using the stream URLs and headers that yt-dlp reports?
//!
//! Usage: cargo run -p engine --example spike -- <youtube-url>

use std::time::Instant;

use serde_json::Value;

const RANGES: [(u64, u64); 2] = [(0, 10_485_759), (10_485_760, 20_971_519)];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::args()
        .nth(1)
        .ok_or("usage: cargo run -p engine --example spike -- <youtube-url>")?;

    // 1. Ask yt-dlp for the video's info as JSON.
    println!("Probing with yt-dlp…");
    let output = tokio::process::Command::new("yt-dlp")
        .args(["-J", "--no-playlist", &url])
        .output()
        .await?;
    if !output.status.success() {
        return Err(format!(
            "yt-dlp failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let info: Value = serde_json::from_slice(&output.stdout)?;
    println!("Title: {}", info["title"].as_str().unwrap_or("?"));

    // 2. Pick the best audio-only and best video-only https formats (yt-dlp lists worst → best).
    let formats = info["formats"].as_array().ok_or("no formats in yt-dlp output")?;
    let field = |f: &Value, k: &str| f[k].as_str().unwrap_or("").to_string();
    let audio = formats
        .iter()
        .filter(|f| field(f, "vcodec") == "none" && !matches!(field(f, "acodec").as_str(), "none" | "") && field(f, "protocol") == "https")
        .last()
        .ok_or("no audio-only https format")?;
    let video = formats
        .iter()
        .filter(|f| field(f, "acodec") == "none" && !matches!(field(f, "vcodec").as_str(), "none" | "") && field(f, "protocol") == "https")
        .last()
        .ok_or("no video-only https format")?;

    // 3. Download two 10 MiB ranges of each and measure speed.
    let client = reqwest::Client::new();
    for (name, fmt) in [("audio", audio), ("video", video)] {
        println!(
            "\n{name}: format {} ({}, {})",
            field(fmt, "format_id"),
            field(fmt, "ext"),
            if name == "audio" { field(fmt, "acodec") } else { field(fmt, "vcodec") }
        );
        let stream_url = fmt["url"].as_str().ok_or("format has no url")?;
        for (start, end) in RANGES {
            let mut req = client.get(stream_url);
            if let Some(headers) = fmt["http_headers"].as_object() {
                for (k, v) in headers {
                    if let Some(v) = v.as_str() {
                        req = req.header(k.as_str(), v);
                    }
                }
            }
            req = req.header("Range", format!("bytes={start}-{end}"));

            let t = Instant::now();
            let resp = req.send().await?;
            let status = resp.status();
            let body = resp.bytes().await?;
            let secs = t.elapsed().as_secs_f64();
            let mbps = body.len() as f64 / 1_000_000.0 / secs;
            println!(
                "  bytes={start}-{end}: status {status}, {} bytes in {secs:.2}s = {mbps:.2} MB/s",
                body.len()
            );
        }
    }
    Ok(())
}
