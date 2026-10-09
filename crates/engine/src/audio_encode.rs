//! AAC encoding of captured PCM for the clip audio tracks.
//!
//! Each audio source is encoded on its own instance: raw device PCM (whatever
//! the mix format is) is resampled to planar f32 at 48 kHz stereo, reframed to
//! the AAC frame size, and encoded. Packets are stamped with the same
//! microsecond time base as the video encoder, so the muxer can interleave them.

use std::collections::VecDeque;

use anyhow::{anyhow, bail, Context, Result};
use ffmpeg::format::sample::Type;
use ffmpeg::format::Sample;
use ffmpeg::{frame, ChannelLayout, Packet};
use ffmpeg_next as ffmpeg;

use crate::audio::PcmChunk;
use crate::encode::MICROS;

/// AAC-LC frame size, in samples per channel.
const FRAME_SAMPLES: usize = 1024;

/// Everything is resampled to this rate before encoding.
const OUTPUT_RATE: u32 = 48_000;

/// Stereo output.
const OUTPUT_CHANNELS: u16 = 2;

/// Encodes one audio source to AAC packets.
pub struct AudioEncoder {
    encoder: ffmpeg::encoder::audio::Encoder,
    resampler: ffmpeg::software::resampling::Context,
    src_format: Sample,
    src_layout: ChannelLayout,
    src_rate: u32,
    /// Planar f32 accumulation, one buffer per output channel.
    buffers: Vec<Vec<f32>>,
    /// Frames already handed to the encoder, for timestamping.
    emitted_samples: u64,
    /// Timestamps of submitted frames, consumed one per emitted packet (the
    /// encoder buffers internally, exactly like the video encoder).
    pending: VecDeque<i64>,
    next_pts: i64,
    base_pts: i64,
    started: bool,
}

impl AudioEncoder {
    /// Opens an AAC encoder matching the format of `first` (the first chunk
    /// from the device).
    pub fn new(first: &PcmChunk, bitrate: u64) -> Result<Self> {
        ffmpeg::init()?;

        let src_format = match (first.is_float, first.bits_per_sample) {
            (true, 32) => Sample::F32(Type::Packed),
            (false, 16) => Sample::I16(Type::Packed),
            (false, 32) => Sample::I32(Type::Packed),
            (is_float, bits) => bail!("unsupported audio format: float={is_float} bits={bits}"),
        };
        let src_layout = ChannelLayout::default(first.channels as i32);

        let dst_format = Sample::F32(Type::Planar);
        let dst_layout = ChannelLayout::default(OUTPUT_CHANNELS as i32);

        let codec = ffmpeg::encoder::find_by_name("aac")
            .ok_or_else(|| anyhow!("AAC encoder is not available in this FFmpeg build"))?;

        let mut context = ffmpeg::codec::context::Context::new_with_codec(codec);
        context.set_time_base(ffmpeg::Rational(1, MICROS));

        let mut audio = context.encoder().audio()?;
        audio.set_rate(OUTPUT_RATE as i32);
        audio.set_channel_layout(dst_layout);
        audio.set_format(dst_format);
        audio.set_bit_rate(bitrate as usize);
        audio.set_flags(ffmpeg::codec::flag::Flags::GLOBAL_HEADER);
        let encoder = audio.open_with(ffmpeg::Dictionary::new())?;

        let resampler = ffmpeg::software::resampling::Context::get(
            src_format,
            src_layout,
            first.sample_rate,
            dst_format,
            dst_layout,
            OUTPUT_RATE,
        )
        .context("creating the audio resampler")?;

        Ok(Self {
            encoder,
            resampler,
            src_format,
            src_layout,
            src_rate: first.sample_rate.max(1),
            buffers: vec![Vec::new(), Vec::new()],
            emitted_samples: 0,
            pending: VecDeque::new(),
            next_pts: 0,
            base_pts: first.timestamp_micros,
            started: false,
        })
    }

    /// Encodes one device chunk, returning whatever packets are ready.
    pub fn encode(&mut self, chunk: &PcmChunk) -> Result<Vec<Packet>> {
        if !self.started {
            self.base_pts = chunk.timestamp_micros;
            self.started = true;
        }

        let bytes_per_sample = (chunk.bits_per_sample / 8).max(1) as usize;
        let frame_samples =
            chunk.data.len() / (chunk.channels as usize * bytes_per_sample).max(1);
        if frame_samples == 0 {
            return Ok(Vec::new());
        }

        // Wrap the raw device PCM in an input frame.
        let mut input = frame::Audio::new(self.src_format, frame_samples, self.src_layout);
        input.set_rate(chunk.sample_rate);
        {
            let plane = input.data_mut(0);
            let len = plane.len().min(chunk.data.len());
            plane[..len].copy_from_slice(&chunk.data[..len]);
        }

        // Resample to planar f32 at the output rate; allow room for rate upscale.
        let capacity =
            (frame_samples as u64 * OUTPUT_RATE as u64 / self.src_rate as u64) as usize + 256;
        let mut output = frame::Audio::new(
            Sample::F32(Type::Planar),
            capacity,
            ChannelLayout::default(OUTPUT_CHANNELS as i32),
        );
        output.set_rate(OUTPUT_RATE);
        self.resampler.run(&input, &mut output)?;

        let produced = output.samples();
        for channel in 0..OUTPUT_CHANNELS as usize {
            let plane = output.data(channel);
            let buffer = &mut self.buffers[channel];
            buffer.extend(
                plane
                    .chunks_exact(4)
                    .take(produced)
                    .map(|bytes| f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            );
        }

        let mut packets = Vec::new();
        while self.buffers[0].len() >= FRAME_SAMPLES {
            packets.extend(self.encode_frame(FRAME_SAMPLES));
        }
        Ok(packets)
    }

    /// Encodes `None` (padding any partial frame with silence) and drains the
    /// remaining packets. Call before muxing the final clip.
    pub fn flush(&mut self) -> Result<Vec<Packet>> {
        let mut packets = Vec::new();
        if self.started && !self.buffers[0].is_empty() {
            packets.extend(self.encode_frame(FRAME_SAMPLES));
        }
        self.encoder.send_eof()?;
        packets.extend(self.drain());
        Ok(packets)
    }

    /// Pops one frame's worth of samples from the buffers and encodes it.
    fn encode_frame(&mut self, samples: usize) -> Vec<Packet> {
        let pts = self.pts_of(self.emitted_samples);

        let mut frame = frame::Audio::new(
            Sample::F32(Type::Planar),
            samples,
            ChannelLayout::default(OUTPUT_CHANNELS as i32),
        );
        frame.set_rate(OUTPUT_RATE);
        frame.set_pts(Some(pts));

        for channel in 0..OUTPUT_CHANNELS as usize {
            let buffer = &mut self.buffers[channel];
            // Pad only when there is not enough for a full frame (the trailing
            // frame); never truncate, or samples from a previous chunk are lost.
            if buffer.len() < samples {
                buffer.resize(samples, 0.0);
            }
            let source: Vec<f32> = buffer.drain(..samples).collect();
            let plane = frame.data_mut(channel);
            for (dst, sample) in plane.chunks_exact_mut(4).zip(source.iter()) {
                dst.copy_from_slice(&sample.to_ne_bytes());
            }
        }

        self.emitted_samples += samples as u64;
        self.pending.push_back(pts);
        if self.encoder.send_frame(&frame).is_err() {
            return Vec::new();
        }
        self.drain()
    }

    fn pts_of(&self, samples: u64) -> i64 {
        self.base_pts + samples as i64 * 1_000_000 / OUTPUT_RATE as i64
    }

    /// Collects every packet the encoder currently has ready, stamping each with
    /// the timestamp of the frame it came from.
    fn drain(&mut self) -> Vec<Packet> {
        let mut packets = Vec::new();
        loop {
            let mut packet = Packet::empty();
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    let pts = self.pending.pop_front().unwrap_or(self.next_pts);
                    self.next_pts = pts + 1_000_000 * FRAME_SAMPLES as i64 / OUTPUT_RATE as i64;
                    packet.set_pts(Some(pts));
                    packet.set_dts(Some(pts));
                    packets.push(packet);
                }
                Err(_) => break,
            }
        }
        packets
    }

    /// The time base every packet is stamped with: microseconds, matching video.
    pub fn time_base(&self) -> ffmpeg::Rational {
        ffmpeg::Rational(1, MICROS)
    }

    /// The opened encoder, needed to declare the output stream when muxing.
    pub fn inner(&self) -> &ffmpeg::encoder::audio::Encoder {
        &self.encoder
    }
}
