//! Diagnostic: list capture devices and capture from one of them, reporting the
//! peak/RMS level so a specific microphone can be checked in isolation.
//!
//! Usage: `cargo run -p trace-engine --example mic_probe [device-id-or-name-fragment]`

use std::time::{Duration, Instant};

use crossbeam_channel::unbounded;
use trace_engine::audio::{self, AudioTrack, PcmChunk};

const SECONDS: u64 = 5;

fn main() -> anyhow::Result<()> {
    println!("--- capture devices ---");
    let devices = audio::list_devices(AudioTrack::Microphone)?;
    for device in &devices {
        println!("{} | {} Hz, {} ch | {}", device.name, device.sample_rate, device.channels, device.id);
    }

    let filter = std::env::args().nth(1);
    let target = devices
        .iter()
        .find(|device| match &filter {
            Some(value) => device.id == *value || device.name.to_lowercase().contains(&value.to_lowercase()),
            None => false,
        })
        .map(|device| device.id.clone());
    match &filter {
        Some(value) => println!("selected: {value} -> {:?}", target),
        None => println!("no filter given; using the system default microphone"),
    }

    let (sender, receiver) = unbounded::<(AudioTrack, PcmChunk)>();
    let capture = audio::start_capture(AudioTrack::Microphone, sender, Instant::now(), target.as_deref())?;

    println!("--- capturing {SECONDS}s (speak now) ---");
    std::thread::sleep(Duration::from_secs(SECONDS));
    capture.stop();

    let mut chunks = 0usize;
    let mut peak = 0.0f32;
    while let Ok((_, chunk)) = receiver.try_recv() {
        chunks += 1;
        peak = peak.max(chunk.peak());
    }
    println!("chunks={chunks} peak={peak:.4}");
    if peak > 0.01 {
        println!("RESULT: microphone is capturing signal");
    } else {
        println!("RESULT: no signal (silent or muted)");
    }
    Ok(())
}
