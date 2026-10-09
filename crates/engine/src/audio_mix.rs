//! Live mixing of two audio sources (system + microphone) into one AAC stream.
//!
//! Each source is converted to planar f32 at 48 kHz stereo and accumulated on a
//! shared timeline (aligned by the first chunk's timestamp). Complete frames are
//! summed, scaled for headroom, and handed to an [`AudioEncoder`], so a clip can
//! carry a ready-to-play mix *alongside* the individual tracks.

use anyhow::Result;
use ffmpeg_next as ffmpeg;
use ffmpeg::Packet;

use crate::audio::{AudioTrack, PcmChunk};
use crate::audio_encode::{AudioEncoder, PcmConverter, FRAME_SAMPLES, OUTPUT_RATE};

/// Gain applied to each source before summing, leaving headroom against clipping.
const SOURCE_GAIN: f32 = 0.7;

/// One source's converted samples, aligned to the shared mix timeline.
struct Source {
    converter: PcmConverter,
    /// Planar f32 at 48 kHz stereo samples awaiting mixing.
    buffer: [Vec<f32>; 2],
    /// Set once the first chunk has fixed this source's timeline offset.
    aligned: bool,
}

impl Source {
    fn new(first: &PcmChunk) -> Result<Self> {
        Ok(Self {
            converter: PcmConverter::new(first)?,
            buffer: [Vec::new(), Vec::new()],
            aligned: false,
        })
    }
}

/// Sums the system and microphone sources into one AAC stream.
pub struct AudioMixer {
    encoder: AudioEncoder,
    system: Option<Source>,
    microphone: Option<Source>,
    /// Absolute sample index (at 48 kHz) that the mix timeline starts on.
    base_index: u64,
    base_known: bool,
    /// Samples already emitted to the encoder.
    emitted: u64,
}

impl AudioMixer {
    /// Creates the mix encoder. The sources are opened lazily as chunks arrive.
    pub fn new(bitrate: u64) -> Result<Self> {
        // The mix is always planar f32, 48 kHz stereo; a synthetic empty chunk is
        // enough to configure the encoder.
        let format = PcmChunk {
            data: Vec::new(),
            sample_rate: OUTPUT_RATE,
            channels: 2,
            bits_per_sample: 32,
            is_float: true,
            timestamp_micros: 0,
        };
        let encoder = AudioEncoder::new(&format, bitrate)?;
        Ok(Self {
            encoder,
            system: None,
            microphone: None,
            base_index: 0,
            base_known: false,
            emitted: 0,
        })
    }

    /// Feeds one device chunk from `track`, returning any mixed packets ready.
    pub fn push(&mut self, track: AudioTrack, chunk: &PcmChunk) -> Result<Vec<Packet>> {
        {
            let source = match track {
                AudioTrack::System => &mut self.system,
                AudioTrack::Microphone => &mut self.microphone,
            };
            if source.is_none() {
                *source = Some(Source::new(chunk)?);
            }
            let source = source.as_mut().unwrap();

            if !source.aligned {
                // Align both sources to a common start so the mix is not offset.
                let index = microseconds_to_samples(chunk.timestamp_micros);
                if !self.base_known {
                    self.base_index = index;
                    self.base_known = true;
                }
                let offset = index.saturating_sub(self.base_index) as usize;
                if offset > 0 {
                    for buffer in &mut source.buffer {
                        buffer.resize(offset, 0.0);
                    }
                }
                source.aligned = true;
            }

            source.converter.convert(chunk, &mut source.buffer)?;
        }

        self.emit_ready()
    }

    /// Mixes whatever remains, padding the tail, then flushes the encoder.
    pub fn flush(&mut self) -> Result<Vec<Packet>> {
        if let (Some(system), Some(microphone)) = (self.system.as_mut(), self.microphone.as_mut()) {
            let longest = system.buffer[0].len().max(microphone.buffer[0].len());
            let padded = (longest + FRAME_SAMPLES - 1) / FRAME_SAMPLES * FRAME_SAMPLES;
            for buffer in &mut system.buffer {
                buffer.resize(padded, 0.0);
            }
            for buffer in &mut microphone.buffer {
                buffer.resize(padded, 0.0);
            }
        }
        let mut packets = self.emit_ready()?;
        packets.extend(self.encoder.flush()?);
        Ok(packets)
    }

    /// Mixes and encodes every frame where both sources have enough samples.
    fn emit_ready(&mut self) -> Result<Vec<Packet>> {
        let (Some(system), Some(microphone)) = (self.system.as_mut(), self.microphone.as_mut())
        else {
            return Ok(Vec::new());
        };

        let mut packets = Vec::new();
        loop {
            let available = system.buffer[0].len().min(microphone.buffer[0].len());
            if available < FRAME_SAMPLES {
                break;
            }

            let mut data = vec![0u8; FRAME_SAMPLES * 2 * 4];
            for index in 0..FRAME_SAMPLES {
                for channel in 0..2 {
                    let mixed = (system.buffer[channel][index] + microphone.buffer[channel][index])
                        * SOURCE_GAIN;
                    let offset = (index * 2 + channel) * 4;
                    data[offset..offset + 4].copy_from_slice(&mixed.clamp(-1.0, 1.0).to_ne_bytes());
                }
            }
            for channel in 0..2 {
                system.buffer[channel].drain(..FRAME_SAMPLES);
                microphone.buffer[channel].drain(..FRAME_SAMPLES);
            }

            let timestamp_micros = samples_to_microseconds(self.base_index + self.emitted);
            self.emitted += FRAME_SAMPLES as u64;
            let chunk = PcmChunk {
                data,
                sample_rate: OUTPUT_RATE,
                channels: 2,
                bits_per_sample: 32,
                is_float: true,
                timestamp_micros,
            };
            packets.extend(self.encoder.encode(&chunk)?);
        }
        Ok(packets)
    }

    /// The time base every packet is stamped with (microseconds).
    pub fn time_base(&self) -> ffmpeg::Rational {
        self.encoder.time_base()
    }

    /// The opened mix encoder, needed to declare the output stream when muxing.
    pub fn inner(&self) -> &ffmpeg::encoder::audio::Encoder {
        self.encoder.inner()
    }
}

fn microseconds_to_samples(micros: i64) -> u64 {
    (micros.max(0) as u64) * OUTPUT_RATE as u64 / 1_000_000
}

fn samples_to_microseconds(samples: u64) -> i64 {
    (samples * 1_000_000 / OUTPUT_RATE as u64) as i64
}
