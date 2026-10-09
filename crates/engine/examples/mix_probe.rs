//! Synthetic check of the audio mixer: sums two tones (440 + 660 Hz) through the
//! mixer and writes the mixed AAC so the result can be inspected/decoded.
//!
//! Usage: `cargo run -p trace-engine --example mix_probe -- out.m4a`

use std::path::PathBuf;

use ffmpeg_next as ffmpeg;
use trace_engine::audio::{AudioTrack, PcmChunk};
use trace_engine::audio_encode::OUTPUT_RATE;
use trace_engine::audio_mix::AudioMixer;

const SECONDS: usize = 3;
const CHUNK: usize = 960;

fn main() -> anyhow::Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/mix_probe.m4a".to_string()),
    );

    let mut mixer = AudioMixer::new(192_000)?;
    let total = OUTPUT_RATE as usize * SECONDS;
    let mut packets = Vec::new();
    let mut index = 0usize;
    while index < total {
        let samples = CHUNK.min(total - index);
        let timestamp = index as i64 * 1_000_000 / OUTPUT_RATE as i64;
        // Interleave both sources so the mixer sees them in lockstep.
        packets.extend(mixer.push(AudioTrack::System, &tone(440.0, index, samples, timestamp))?);
        packets.extend(mixer.push(
            AudioTrack::Microphone,
            &tone(660.0, index, samples, timestamp),
        )?);
        index += samples;
    }
    packets.extend(mixer.flush()?);
    println!("packets: {}", packets.len());

    write_audio_mp4(&out, mixer.inner(), &mut packets)?;
    println!("wrote {}", out.display());
    Ok(())
}

/// One stereo f32 chunk of a sine tone.
fn tone(hz: f32, start: usize, samples: usize, timestamp_micros: i64) -> PcmChunk {
    let mut data = vec![0u8; samples * 2 * 4];
    for i in 0..samples {
        let seconds = (start + i) as f32 / OUTPUT_RATE as f32;
        let value = (seconds * hz * std::f32::consts::TAU).sin() * 0.25;
        for channel in 0..2 {
            let offset = (i * 2 + channel) * 4;
            data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    PcmChunk {
        data,
        sample_rate: OUTPUT_RATE,
        channels: 2,
        bits_per_sample: 32,
        is_float: true,
        timestamp_micros,
    }
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
