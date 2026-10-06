//! Records the primary monitor for a few seconds, then saves the replay buffer
//! as an MP4.
//!
//! Usage: `cargo run -p clipper23-engine --example replay -- [seconds] [out-dir]`

use std::path::PathBuf;
use std::time::Duration;

use clipper23_engine::session::{ReplayConfig, ReplaySession};

fn main() -> anyhow::Result<()> {
    let seconds: f64 = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(8.0);
    let out_dir = PathBuf::from(
        std::env::args()
            .nth(2)
            .unwrap_or_else(|| "target/replay-test".to_string()),
    );

    println!("recording for {seconds}s ...");
    let session = ReplaySession::start(
        ReplayConfig {
            buffer_seconds: seconds.max(30.0),
            ..ReplayConfig::default()
        },
        out_dir,
    )?;

    std::thread::sleep(Duration::from_secs_f64(seconds));

    let clip = session.finish(seconds)?;
    println!("saved {}", clip.display());

    Ok(())
}
