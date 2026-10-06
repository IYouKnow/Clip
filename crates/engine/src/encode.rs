//! H.264 encoding of captured frames, plus encoder selection.

use std::collections::{HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;
use ffmpeg::util::frame::Video as VideoFrame;
use ffmpeg::Packet;

/// Timestamps are carried as microseconds; 1/1_000_000 time base.
pub const MICROS: i32 = 1_000_000;

/// H.264 encoders in preference order.
///
/// NVENC (NVIDIA) and AMF (AMD) are dedicated hardware; `h264_mf` uses Media
/// Foundation and covers Intel plus any machine without the vendor encoders.
/// `libx264` is deliberately last: the vendored FFmpeg is LGPL and does not
/// ship it, but a custom GPL build would be picked up automatically.
pub const H264_ENCODERS: &[&str] = &["h264_nvenc", "h264_amf", "h264_qsv", "h264_mf", "libx264"];

/// Availability of one candidate encoder.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EncoderInfo {
    pub name: String,
    pub available: bool,
}

/// Reports which known H.264 encoders are usable on this machine.
///
/// An encoder can be compiled into FFmpeg yet still fail to open (for example
/// NVENC without a driver), so once an attempt fails it is remembered.
pub fn h264_encoders() -> Vec<EncoderInfo> {
    H264_ENCODERS
        .iter()
        .map(|name| EncoderInfo {
            name: (*name).to_string(),
            available: ffmpeg_next::encoder::find_by_name(name).is_some() && !is_unusable(name),
        })
        .collect()
}

/// Encoders that failed to open on this machine.
///
/// Retrying them on every start spams driver errors and wastes time, so the
/// failure is remembered for the lifetime of the process.
fn unusable() -> &'static Mutex<HashSet<String>> {
    static UNUSABLE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    UNUSABLE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn is_unusable(name: &str) -> bool {
    unusable()
        .lock()
        .map(|set| set.contains(name))
        .unwrap_or(false)
}

fn mark_unusable(name: &str) {
    if let Ok(mut set) = unusable().lock() {
        set.insert(name.to_string());
    }
}

/// Picks the best available H.264 encoder for this machine.
pub fn pick_h264_encoder() -> Option<&'static str> {
    H264_ENCODERS
        .iter()
        .copied()
        .find(|name| ffmpeg_next::encoder::find_by_name(name).is_some())
}

/// Encodes BGRA frames to H.264 packets.
pub struct VideoEncoder {
    name: String,
    encoder: ffmpeg::encoder::video::Encoder,
    scaler: scaling::Context,
    /// Pixel format the scaler produces (what the encoder accepts).
    out_format: Pixel,
    /// Reused input frame in BGRA.
    scratch: VideoFrame,
    width: u32,
    height: u32,
    /// Microseconds between frames, used to advance timestamps.
    interval_micros: i64,
    /// Timestamps of submitted frames, consumed one per emitted packet.
    ///
    /// Encoders buffer internally, so a single `send_frame` may yield zero, one
    /// or several packets. Queueing keeps each packet on its own frame's time.
    pending: VecDeque<i64>,
    /// Timestamp to continue from when the queue is exhausted (during flush).
    next_pts: i64,
}

impl VideoEncoder {
    /// Opens `preferred` if given, otherwise the best available encoder,
    /// falling back through the list when one fails to open.
    pub fn new(
        preferred: Option<&str>,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u64,
    ) -> Result<Self> {
        ffmpeg::init()?;

        let mut candidates: Vec<&str> = Vec::new();
        if let Some(name) = preferred {
            candidates.push(name);
        }
        for name in H264_ENCODERS {
            if !candidates.contains(name) {
                candidates.push(name);
            }
        }

        let mut last_error = None;
        for name in candidates {
            if is_unusable(name) || ffmpeg::encoder::find_by_name(name).is_none() {
                continue;
            }
            match open(name, width, height, fps, bitrate) {
                Ok(encoder) => return Self::finish(encoder, width, height, fps, name),
                Err(error) => {
                    mark_unusable(name);
                    last_error = Some(error);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("no usable H.264 encoder available")))
    }

    /// Builds the scaler and input frame once the encoder is open.
    fn finish(
        encoder: ffmpeg::encoder::video::Encoder,
        width: u32,
        height: u32,
        fps: u32,
        name: &str,
    ) -> Result<Self> {
        let pixel_format = encoder.format();

        let scaler = scaling::Context::get(
            Pixel::BGRA,
            width,
            height,
            pixel_format,
            width,
            height,
            scaling::Flags::BILINEAR,
        )?;

        Ok(Self {
            name: name.to_string(),
            encoder,
            scaler,
            out_format: pixel_format,
            scratch: VideoFrame::new(Pixel::BGRA, width, height),
            width,
            height,
            interval_micros: 1_000_000 / fps.max(1) as i64,
            pending: VecDeque::new(),
            next_pts: 0,
        })
    }

    /// The time base every packet is stamped with: microseconds.
    ///
    /// Encoders are free to rewrite their own `time_base` (NVENC/AMF do), so
    /// timestamps are normalised to microseconds here instead of trusting it.
    pub fn time_base(&self) -> ffmpeg::Rational {
        ffmpeg::Rational(1, MICROS)
    }

    /// The name of the encoder actually in use.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Encodes one BGRA frame at `pts` microseconds, returning any packets the
    /// encoder produced.
    ///
    /// `bgra` keeps the device's row padding (`pitch` bytes per row), which is
    /// copied row by row into the scaler's input frame.
    pub fn encode_bgra(&mut self, bgra: &[u8], pitch: usize, pts: i64) -> Result<Vec<Packet>> {
        let row = (self.width * 4) as usize;
        anyhow::ensure!(pitch >= row, "row pitch {pitch} smaller than row {row}");
        let needed = (self.height as usize - 1)
            .checked_mul(pitch)
            .map(|offset| offset + row)
            .unwrap_or(0);
        anyhow::ensure!(
            bgra.len() >= needed,
            "frame buffer too small: {} < {needed}",
            bgra.len()
        );

        let stride = self.scratch.stride(0);
        {
            let dest = self.scratch.data_mut(0);
            for y in 0..self.height as usize {
                let src = &bgra[y * pitch..y * pitch + row];
                dest[y * stride..y * stride + row].copy_from_slice(src);
            }
        }
        self.scratch.set_pts(Some(pts));

        let mut converted = VideoFrame::new(self.out_format, self.width, self.height);
        self.scaler.run(&self.scratch, &mut converted)?;
        converted.set_pts(Some(pts));

        self.pending.push_back(pts);
        self.encoder.send_frame(&converted)?;

        Ok(self.drain())
    }

    /// Encodes `None`, then drains any remaining packets (call before saving).
    pub fn flush(&mut self) -> Result<Vec<Packet>> {
        self.encoder.send_eof()?;
        Ok(self.drain())
    }

    /// Collects every packet the encoder currently has ready.
    ///
    /// Each packet is stamped with the timestamp of the frame it came from;
    /// encoders otherwise invent frame-index based timestamps, which mux into a
    /// near-zero duration. Safe because B-frames are disabled, so packets leave
    /// in submission order.
    fn drain(&mut self) -> Vec<Packet> {
        let mut packets = Vec::new();
        loop {
            let mut packet = Packet::empty();
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    let pts = self.pending.pop_front().unwrap_or(self.next_pts);
                    self.next_pts = pts + self.interval_micros;
                    packet.set_pts(Some(pts));
                    packet.set_dts(Some(pts));
                    packets.push(packet);
                }
                Err(_) => break,
            }
        }
        packets
    }

    /// The opened encoder, needed to declare the output stream when muxing.
    pub fn inner(&self) -> &ffmpeg::encoder::video::Encoder {
        &self.encoder
    }
}

fn open(
    name: &str,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u64,
) -> Result<ffmpeg::encoder::video::Encoder> {
    // Hardware encoders want NV12 with low-latency tuning; some reject the
    // options, so fall back to plain YUV420P.
    if let Ok(encoder) = open_with(name, width, height, fps, bitrate, Pixel::NV12, latency_options(name))
    {
        return Ok(encoder);
    }
    open_with(
        name,
        width,
        height,
        fps,
        bitrate,
        Pixel::YUV420P,
        ffmpeg::Dictionary::new(),
    )
}

fn open_with(
    name: &str,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u64,
    pixel_format: Pixel,
    options: ffmpeg::Dictionary<'_>,
) -> Result<ffmpeg::encoder::video::Encoder> {
    Ok(build_video(name, width, height, fps, bitrate, pixel_format)?.open_with(options)?)
}

/// Low-latency tuning per encoder.
///
/// A replay buffer needs packets to be emitted as soon as possible; by default
/// hardware encoders hold frames back, which would leave the newest seconds of
/// the buffer unsaved.
fn latency_options(name: &str) -> ffmpeg::Dictionary<'_> {
    let mut options = ffmpeg::Dictionary::new();
    match name {
        "h264_nvenc" => {
            options.set("tune", "ll");
            options.set("delay", "0");
            options.set("rc", "cbr");
        }
        "h264_amf" => {
            options.set("usage", "ultralowlatency");
            options.set("quality", "speed");
        }
        "h264_qsv" => {
            options.set("low_delay_brc", "1");
            options.set("async_depth", "1");
        }
        "h264_mf" => {
            options.set("low_latency", "1");
        }
        _ => {}
    }
    options
}

/// Builds the encoder configuration; not yet opened.
fn build_video(
    name: &str,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: u64,
    pixel_format: Pixel,
) -> Result<ffmpeg::encoder::video::Video> {
    let codec = ffmpeg::encoder::find_by_name(name)
        .ok_or_else(|| anyhow!("encoder '{name}' not found"))?;

    let mut context = ffmpeg::codec::context::Context::new_with_codec(codec);
    context.set_time_base(ffmpeg::Rational(1, MICROS));
    context.set_frame_rate(Some(ffmpeg::Rational(fps as i32, 1)));

    let mut video = context.encoder().video()?;
    video.set_bit_rate(bitrate as usize);
    video.set_width(width);
    video.set_height(height);
    video.set_format(pixel_format);
    // Two-second GOP so the ring can always find a keyframe to start on.
    video.set_gop(fps * 2);
    video.set_max_b_frames(0);
    video.set_flags(ffmpeg::codec::flag::Flags::GLOBAL_HEADER);

    Ok(video)
}
