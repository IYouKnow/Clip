//! Headless capture check: grab one frame of the primary monitor and save it.
//!
//! Usage: `cargo run -p clipper23-engine --example capture_png -- frame.png`

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "frame.png".to_string());

    clipper23_engine::capture::save_primary_monitor_png(&out)?;
    println!("saved {out}");
    Ok(())
}
