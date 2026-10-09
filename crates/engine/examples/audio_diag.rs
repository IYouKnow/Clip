//! Diagnostic: play a tone, capture system loopback, encode it to AAC and write
//! an audio-only MP4, reporting raw-capture statistics so the audio path can be
//! checked end to end.
//!
//! Usage: `cargo run -p trace-engine --example audio_diag -- out.m4a`

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossbeam_channel::unbounded;
use ffmpeg_next as ffmpeg;
use trace_engine::audio::{self, AudioTrack, PcmChunk};
use trace_engine::audio_encode::AudioEncoder;
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode};

const TONE_HZ: f32 = 1000.0;
const SECONDS: u64 = 3;

fn main() -> anyhow::Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/audio_diag.m4a".to_string()),
    );

    let (sender, receiver) = unbounded::<(AudioTrack, PcmChunk)>();
    let capture = audio::start_capture(AudioTrack::System, sender, Instant::now(), None)?;
    let renderer = std::thread::spawn(|| play_tone(Duration::from_secs(SECONDS)));

    std::thread::sleep(Duration::from_millis(SECONDS * 1000 + 500));
    capture.stop();
    let _ = renderer.join();

    let mut chunks: Vec<PcmChunk> = Vec::new();
    while let Ok((_, chunk)) = receiver.try_recv() {
        chunks.push(chunk);
    }

    println!("chunks: {}", chunks.len());
    if let Some(first) = chunks.first() {
        println!(
            "format: {} Hz, {} ch, {} bit, float={}",
            first.sample_rate, first.channels, first.bits_per_sample, first.is_float
        );
        let bytes: usize = chunks.iter().map(|c| c.data.len()).sum();
        let frames: usize =
            chunks.iter().map(|c| c.data.len() / (c.channels as usize * 4)).sum();
        println!(
            "bytes: {bytes}, frames: {frames} ({:.3}s), first_pts: {}, last_pts: {}",
            frames as f64 / first.sample_rate as f64,
            chunks.first().unwrap().timestamp_micros,
            chunks.last().unwrap().timestamp_micros,
        );
        println!(
            "raw peak(f32): {:.4}, rms(f32): {:.4}",
            peak_f32(&chunks),
            rms_f32(&chunks)
        );
    }

    // Encode and write an audio-only MP4.
    let first = chunks.first().cloned().unwrap();
    let mut encoder = AudioEncoder::new(&first, 192_000)?;
    let mut packets = Vec::new();
    for chunk in &chunks {
        packets.extend(encoder.encode(chunk)?);
    }
    packets.extend(encoder.flush()?);
    println!(
        "encoded packets: {}, first_pts: {:?}, last_pts: {:?}",
        packets.len(),
        packets.first().and_then(|p| p.pts()),
        packets.last().and_then(|p| p.pts()),
    );

    write_audio_mp4(&out, encoder.inner(), &mut packets)?;
    println!("wrote {}", out.display());
    Ok(())
}

/// Writes audio packets to an MP4 (audio-only), rescaling to the muxer time base.
fn write_audio_mp4(
    path: &std::path::Path,
    encoder: &ffmpeg::encoder::audio::Encoder,
    packets: &mut [ffmpeg::Packet],
) -> anyhow::Result<()> {
    let mut output = ffmpeg::format::output(path)?;
    let index = {
        let mut stream = output.add_stream(encoder.id())?;
        stream.set_parameters(encoder);
        stream.index()
    };
    output.write_header()?;

    let in_base = ffmpeg::Rational(1, 1_000_000);
    let out_base = output
        .stream(index)
        .map(|stream| stream.time_base())
        .unwrap_or(in_base);
    for packet in packets.iter_mut() {
        packet.set_stream(index);
        if out_base.0 != 0 && out_base.1 != 0 {
            packet.rescale_ts(in_base, out_base);
        }
        packet.write_interleaved(&mut output)?;
    }
    output.write_trailer()?;
    Ok(())
}

fn peak_f32(chunks: &[PcmChunk]) -> f32 {
    let mut peak = 0.0f32;
    for chunk in chunks {
        for sample in chunk.data.chunks_exact(4) {
            let value = f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]);
            peak = peak.max(value.abs());
        }
    }
    peak
}

fn rms_f32(chunks: &[PcmChunk]) -> f32 {
    let mut sum = 0.0f64;
    let mut count = 0u64;
    for chunk in chunks {
        for sample in chunk.data.chunks_exact(4) {
            let value = f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f64;
            sum += value * value;
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum / count as f64).sqrt() as f32
    }
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
