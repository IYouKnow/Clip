//! Deterministic check of the encode -> ring -> mux path, independent of what
//! is on screen.
//!
//! Generates moving frames, encodes them, buffers them, then writes a clip and
//! reports what the ring selected.
//!
//! Usage: `cargo run -p trace-engine --example encode_synthetic -- out.mp4`

use std::path::PathBuf;

use trace_engine::encode::VideoEncoder;
use trace_engine::mux;
use trace_engine::ring::PacketRing;

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;
const FPS: u32 = 30;
const FRAMES: u32 = 150; // 5 seconds, ~2 keyframes at a 2s GOP

fn main() -> anyhow::Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/synthetic.mp4".to_string()),
    );

    let mut encoder = VideoEncoder::new(None, WIDTH, HEIGHT, FPS, 8_000_000)?;
    println!("encoder: {}", encoder.name());

    let mut ring = PacketRing::new(encoder.time_base(), 60.0);
    let row = (WIDTH * 4) as usize;
    let mut data = vec![0u8; row * HEIGHT as usize];

    for index in 0..FRAMES {
        fill(&mut data, index);
        let pts = index as i64 * 1_000_000 / FPS as i64;
        // The generated buffer is tightly packed, so pitch == row length.
        for packet in encoder.encode_bgra(&data, row, pts)? {
            ring.push(packet);
        }
    }
    for packet in encoder.flush()? {
        ring.push(packet);
    }

    let range = ring.snapshot_range(60.0);
    println!(
        "buffered {} packets, snapshot takes {}..{}",
        ring.len(),
        range.start,
        range.end
    );

    let time_base = ring.time_base();
    let packets = ring.slice_mut(range);
    mux::write_mp4(&out, (encoder.inner(), packets), &mut [], time_base)?;

    println!("wrote {}", out.display());
    Ok(())
}

/// A moving bright band over a gradient so consecutive frames differ.
fn fill(data: &mut [u8], index: u32) {
    let width = WIDTH as usize;
    let height = HEIGHT as usize;
    let band = (index as usize * 7) % height;

    for y in 0..height {
        let in_band = y.abs_diff(band) < 30;
        let green = (y * 255 / height) as u8;
        for x in 0..width {
            let i = (y * width + x) * 4;
            if in_band {
                data[i] = 40;
                data[i + 1] = 220;
                data[i + 2] = 255;
            } else {
                data[i] = (x * 255 / width) as u8;
                data[i + 1] = green;
                data[i + 2] = 30;
            }
            data[i + 3] = 255;
        }
    }
}
