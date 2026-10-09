//! Headless end-to-end session check: starts a real replay session (monitor +
//! system audio + microphone), records a few seconds, saves a clip and stops.
//! Exercises the whole capture → encode → mux → teardown path without the UI.
//!
//! Usage: `cargo run -p trace-engine --example session_smoke`

use std::path::PathBuf;
use std::time::Duration;

use trace_engine::session::{AudioConfig, ReplayConfig, ReplaySession};

fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/smoke-clips"));
    std::fs::create_dir_all(&dir)?;

    let mode = std::env::var("SMOKE_AUDIO").unwrap_or_else(|_| "both".to_string());
    let (system, microphone) = match mode.as_str() {
        "off" | "0" => (false, false),
        "system" => (true, false),
        "mic" | "microphone" => (false, true),
        _ => (true, true),
    };
    let config = ReplayConfig {
        encoder: None,
        fps: 60,
        bitrate: 20_000_000,
        buffer_seconds: 10.0,
        audio: AudioConfig {
            system,
            microphone,
            bitrate: 192_000,
            system_device: std::env::var("SMOKE_SYSTEM_DEVICE").ok(),
            microphone_device: std::env::var("SMOKE_MIC_DEVICE").ok(),
        },
    };
    println!("audio: {mode}");

    println!("starting session...");
    let session = ReplaySession::start(config, dir.clone())?;
    println!("recording 4s...");
    std::thread::sleep(Duration::from_secs(4));

    println!("saving clip...");
    let path = session.save(4.0)?;
    println!("saved {}", path.display());

    println!("stopping session...");
    session.stop()?;
    println!("stopped cleanly");
    Ok(())
}
