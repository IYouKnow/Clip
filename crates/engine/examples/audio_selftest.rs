//! End-to-end audio self-test: renders a tone to the default output while
//! capturing system loopback, then reports what the capture actually received.
//!
//! This proves the loopback wiring independently of whatever happens to be
//! playing on the machine.
//!
//! Usage: `cargo run -p clipper23-engine --example audio_selftest`

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use clipper23_engine::audio::{self, AudioTrack, PcmChunk};
use crossbeam_channel::unbounded;
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode};

const TONE_HZ: f32 = 1000.0;
const TONE_SECONDS: u64 = 2;

fn main() -> anyhow::Result<()> {
    let (sender, receiver) = unbounded::<PcmChunk>();
    let capture = audio::start_capture(AudioTrack::System, sender)?;

    let renderer = std::thread::spawn(|| play_tone(Duration::from_secs(TONE_SECONDS)));

    std::thread::sleep(Duration::from_millis(TONE_SECONDS * 1000 + 500));
    capture.stop();

    match renderer.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("tone playback failed: {error:#}"),
        Err(_) => eprintln!("tone thread panicked"),
    }

    let mut chunks = 0usize;
    let mut bytes = 0usize;
    let mut peak = 0f32;
    let mut format = None;
    while let Ok(chunk) = receiver.try_recv() {
        chunks += 1;
        bytes += chunk.data.len();
        format = Some((chunk.is_float, chunk.bits_per_sample));
        peak = peak.max(peak_amplitude(&chunk));
    }

    println!("loopback: chunks={chunks} bytes={bytes} peak_amplitude={peak:.4}");
    match format {
        Some((is_float, bits)) => println!("format: float={is_float} bits={bits}"),
        None => println!("format: nothing captured"),
    }

    if peak > 0.01 {
        println!("RESULT: loopback capture works (signal detected)");
    } else {
        println!("RESULT: no signal captured");
    }

    Ok(())
}

/// Largest absolute sample value found in a chunk.
fn peak_amplitude(chunk: &PcmChunk) -> f32 {
    let bytes_per_sample = (chunk.bits_per_sample / 8) as usize;
    if bytes_per_sample == 0 {
        return 0.0;
    }

    let mut peak = 0.0f32;
    for sample in chunk.data.chunks_exact(bytes_per_sample) {
        let value = if chunk.is_float && bytes_per_sample == 4 {
            f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]])
        } else if !chunk.is_float && bytes_per_sample == 2 {
            i16::from_le_bytes([sample[0], sample[1]]) as f32 / i16::MAX as f32
        } else {
            0.0
        };
        peak = peak.max(value.abs());
    }
    peak
}

/// Plays a sine tone on the default output device.
fn play_tone(duration: Duration) -> anyhow::Result<()> {
    wasapi::initialize_mta().ok()?;

    let enumerator = DeviceEnumerator::new()?;
    let device = enumerator.get_default_device(&Direction::Render)?;
    let mut client = device.get_iaudioclient()?;
    let format = client.get_mixformat()?;

    let block_align = format.get_blockalign() as usize;
    let channels = format.get_nchannels() as usize;
    let sample_rate = format.get_samplespersec() as f32;
    let bits = format.get_bitspersample() as usize;
    let is_float = matches!(format.get_subformat(), Ok(SampleType::Float));

    let (_, min_period) = client.get_device_period()?;
    client.initialize_client(
        &format,
        &Direction::Render,
        &StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        },
    )?;

    let render = client.get_audiorenderclient()?;
    client.start_stream()?;

    let start = Instant::now();
    let mut phase = 0f32;

    while start.elapsed() < duration {
        let frames = client.get_available_space_in_frames()? as usize;
        if frames == 0 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        let mut data = vec![0u8; frames * block_align];
        for frame in 0..frames {
            let sample = (phase * std::f32::consts::TAU).sin() * 0.3;
            phase = (phase + TONE_HZ / sample_rate).fract();

            for channel in 0..channels {
                let offset = (frame * channels + channel) * (bits / 8);
                if is_float && bits == 32 {
                    data[offset..offset + 4].copy_from_slice(&sample.to_le_bytes());
                } else if !is_float && bits == 16 {
                    let value = (sample * i16::MAX as f32) as i16;
                    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
                }
            }
        }

        let mut queue: VecDeque<u8> = data.into();
        render.write_to_device_from_deque(frames, &mut queue, None)?;
        std::thread::sleep(Duration::from_millis(5));
    }

    client.stop_stream()?;
    Ok(())
}
