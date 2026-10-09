//! Headless check of the audio path: encodes synthetic video plus two AAC
//! tracks and muxes them into one MP4, so audio encoding, reframing and
//! interleaving can be verified without the UI.
//!
//! Usage: `cargo run -p trace-engine --example audio_mux -- out.mp4`

use std::path::PathBuf;

use trace_engine::audio::PcmChunk;
use trace_engine::audio_encode::AudioEncoder;
use trace_engine::encode::VideoEncoder;
use trace_engine::mux;
use trace_engine::ring::PacketRing;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FPS: u32 = 30;
const FRAMES: u32 = 90; // 3 seconds
const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
/// Samples fed per synthetic chunk, to exercise the encoder's reframing.
const CHUNK_SAMPLES: usize = 960;

fn main() -> anyhow::Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/audio_mux.mp4".to_string()),
    );

    // --- video ---
    let mut video_encoder = VideoEncoder::new(None, WIDTH, HEIGHT, FPS, 8_000_000)?;
    println!("video encoder: {}", video_encoder.name());
    let mut video_ring = PacketRing::new(video_encoder.time_base(), 60.0);
    let row = (WIDTH * 4) as usize;
    let mut frame = vec![0u8; row * HEIGHT as usize];
    for index in 0..FRAMES {
        fill_video(&mut frame, index);
        let pts = index as i64 * 1_000_000 / FPS as i64;
        for packet in video_encoder.encode_bgra(&frame, row, pts)? {
            video_ring.push(packet);
        }
    }
    for packet in video_encoder.flush()? {
        video_ring.push(packet);
    }

    // --- two audio tracks ---
    let (audio_encoder_a, mut audio_ring_a) = build_audio_track(440.0)?;
    let (audio_encoder_b, mut audio_ring_b) = build_audio_track(660.0)?;

    let time_base = video_ring.time_base();
    let video_range = video_ring.snapshot_range(60.0);
    let audio_range_a = audio_ring_a.range_for_window(i64::MIN, i64::MAX);
    let audio_range_b = audio_ring_b.range_for_window(i64::MIN, i64::MAX);

    println!(
        "video packets: {}, audio packets: {}/{}",
        video_ring.len(),
        audio_ring_a.len(),
        audio_ring_b.len()
    );

    let mut audio_streams = [
        (audio_encoder_a.inner(), audio_ring_a.slice_mut(audio_range_a)),
        (audio_encoder_b.inner(), audio_ring_b.slice_mut(audio_range_b)),
    ];
    mux::write_mp4(
        &out,
        (video_encoder.inner(), video_ring.slice_mut(video_range)),
        &mut audio_streams,
        time_base,
    )?;

    println!("wrote {}", out.display());
    Ok(())
}

/// Encodes a 3-second sine tone to AAC, returning the encoder and its buffer.
fn build_audio_track(tone_hz: f32) -> anyhow::Result<(AudioEncoder, PacketRing)> {
    let total_samples = RATE as usize * FRAMES as usize / FPS as usize;
    let mut encoder: Option<AudioEncoder> = None;
    let mut ring: Option<PacketRing> = None;
    let mut sample_index = 0usize;

    while sample_index < total_samples {
        let samples = CHUNK_SAMPLES.min(total_samples - sample_index);
        let mut data = vec![0u8; samples * CHANNELS as usize * 4];
        for i in 0..samples {
            let seconds = (sample_index + i) as f32 / RATE as f32;
            let value = (seconds * tone_hz * std::f32::consts::TAU).sin() * 0.25;
            for channel in 0..CHANNELS as usize {
                let offset = (i * CHANNELS as usize + channel) * 4;
                data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            }
        }

        let chunk = PcmChunk {
            data,
            sample_rate: RATE,
            channels: CHANNELS,
            bits_per_sample: 32,
            is_float: true,
            timestamp_micros: sample_index as i64 * 1_000_000 / RATE as i64,
        };

        if encoder.is_none() {
            let opened = AudioEncoder::new(&chunk, 192_000)?;
            ring = Some(PacketRing::new(opened.time_base(), 60.0));
            encoder = Some(opened);
        }

        let packets = encoder.as_mut().unwrap().encode(&chunk)?;
        for packet in packets {
            ring.as_mut().unwrap().push(packet);
        }
        sample_index += samples;
    }

    for packet in encoder.as_mut().unwrap().flush()? {
        ring.as_mut().unwrap().push(packet);
    }

    Ok((encoder.unwrap(), ring.unwrap()))
}

/// A moving bright band over a gradient so consecutive frames differ.
fn fill_video(data: &mut [u8], index: u32) {
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
