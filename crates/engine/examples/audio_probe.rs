//! Checks that the default audio devices open and actually deliver samples.
//!
//! Captures two seconds from system loopback and the microphone, then reports
//! how much data arrived and its peak level.
//!
//! Usage: `cargo run -p trace-engine --example audio_probe`

use std::time::{Duration, Instant};

use trace_engine::audio::{self, AudioTrack, PcmChunk};
use crossbeam_channel::unbounded;

const CAPTURE_SECONDS: u64 = 2;

fn main() -> anyhow::Result<()> {
    println!("--- default devices ---");
    for track in [AudioTrack::System, AudioTrack::Microphone] {
        match audio::default_device_info(track) {
            Ok(info) => println!(
                "{:<10} {} | {} Hz, {} ch, {} bit, float={}",
                info.track, info.name, info.sample_rate, info.channels,
                info.bits_per_sample, info.is_float
            ),
            Err(error) => println!("{:<10} unavailable: {error}", track.label()),
        }
    }

    println!("--- available devices ---");
    for track in [AudioTrack::System, AudioTrack::Microphone] {
        match audio::list_devices(track) {
            Ok(devices) => {
                for device in devices {
                    println!(
                        "{:<10} {} | {} Hz, {} ch | {}",
                        device.track, device.name, device.sample_rate, device.channels, device.id
                    );
                }
            }
            Err(error) => println!("{:<10} cannot list: {error}", track.label()),
        }
    }

    println!("--- capturing {CAPTURE_SECONDS}s ---");
    let mut handles = Vec::new();
    let mut receivers = Vec::new();
    for track in [AudioTrack::System, AudioTrack::Microphone] {
        let (sender, receiver) = unbounded::<(AudioTrack, PcmChunk)>();
        match audio::start_capture(track, sender, Instant::now(), None) {
            Ok(handle) => {
                handles.push(handle);
                receivers.push((track, receiver));
            }
            Err(error) => println!("{:<10} cannot capture: {error}", track.label()),
        }
    }

    std::thread::sleep(Duration::from_secs(CAPTURE_SECONDS));

    for (track, receiver) in &receivers {
        let mut chunks = 0usize;
        let mut bytes = 0usize;
        let mut peak = 0u8;
        while let Ok((_, chunk)) = receiver.try_recv() {
            chunks += 1;
            bytes += chunk.data.len();
            for &byte in &chunk.data {
                peak = peak.max(byte);
            }
        }
        println!(
            "{:<10} chunks={chunks} bytes={bytes} peak_byte={peak} ({})",
            track.label(),
            if peak == 0 { "silent" } else { "has signal" }
        );
    }

    for handle in handles {
        handle.stop();
    }
    Ok(())
}
