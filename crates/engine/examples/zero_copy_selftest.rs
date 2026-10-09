//! End-to-end check of the capture → encode pipeline.
//!
//! Records the primary monitor for a few seconds and reports which pipeline was
//! used, how many frames were captured, dropped or skipped as idle, then saves
//! the buffered clip.
//!
//! Usage: `cargo run -p trace-engine --example zero_copy_selftest -- [seconds] [out-dir]`

use std::path::PathBuf;
use std::time::Duration;

use trace_engine::session::{ReplayConfig, ReplaySession};

fn main() -> anyhow::Result<()> {
    let seconds: f64 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(5.0);
    let out_dir = PathBuf::from(
        std::env::args()
            .nth(2)
            .unwrap_or_else(|| "target/replay-test".to_string()),
    );

    println!("recording the primary monitor for {seconds}s ...");
    let session = ReplaySession::start(
        ReplayConfig {
            buffer_seconds: seconds.max(30.0),
            ..ReplayConfig::default()
        },
        out_dir,
    )?;

    std::thread::sleep(Duration::from_secs_f64(seconds));

    let stats = session.stats();
    let pipeline = stats
        .pipeline()
        .map(|kind| kind.label().to_string())
        .unwrap_or_else(|| "not started".to_string());
    let encoder = stats.encoder().unwrap_or_else(|| "none".to_string());
    let elapsed = seconds.max(0.001);
    println!("encoder:  {encoder}");
    println!("pipeline: {pipeline}");
    println!("frames:   {} ({:.1} fps)", stats.frames(), stats.frames() as f64 / elapsed);
    println!("packets:  {}", stats.packets());
    println!("dropped:  {}", stats.dropped());
    println!("idle:     {}", stats.idle());

    let clip = session.finish(seconds)?;
    println!("saved {}", clip.display());
    Ok(())
}
