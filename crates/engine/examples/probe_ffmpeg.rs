//! Probes the linked FFmpeg build and reports which H.264 encoders exist.
//!
//! Usage: `cargo run -p trace-engine --example probe_ffmpeg`

fn main() -> anyhow::Result<()> {
    ffmpeg_next::init()?;
    println!("libavutil: {}", ffmpeg_next::util::version());

    for encoder in trace_engine::encode::h264_encoders() {
        let marker = if encoder.available { "yes" } else { "no " };
        println!("  [{}] {}", marker, encoder.name);
    }

    match trace_engine::encode::pick_h264_encoder() {
        Some(name) => println!("selected: {name}"),
        None => println!("selected: none (no H.264 encoder available)"),
    }

    Ok(())
}
