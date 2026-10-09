//! Headless capture check: grab one frame of the primary monitor and save it.
//!
//! Usage: `cargo run -p trace-engine --example capture_png -- frame.png`

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "frame.png".to_string());

    trace_engine::capture::save_primary_monitor_png(&out)?;
    println!("saved {out}");
    Ok(())
}
