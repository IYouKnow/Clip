//! H.264 encoding of captured frames, plus encoder selection.
//!
//! Two paths share this type:
//! * **Zero-copy**: NVENC/AMF are opened with an attached D3D11 `hw_frames_ctx`
//!   and are fed NV12 hardware frames straight off the GPU ([`encode_av_frame`]).
//! * **CPU fallback**: a BGRA frame is colour-converted with swscale and encoded
//!   with whatever encoder opened, used when no GPU path is available.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg::format::Pixel;
use ffmpeg::software::scaling;
use ffmpeg::util::frame::Video as VideoFrame;
use ffmpeg::Packet;

use crate::hw::HwFrames;

/// Timestamps are carried as microseconds; 1/1_000_000 time base.
pub const MICROS: i32 = 1_000_000;

/// H.264 encoders in preference order.
///
/// NVENC (NVIDIA) and AMF (AMD) are dedicated hardware blocks and are the only
/// encoders that accept D3D11 frames for a true zero-copy path. `h264_mf`
/// (Media Foundation, usually Intel) and `libopenh264` are software/CPU
/// fallbacks. `h264_qsv` and `libx264` are absent from the vendored LGPL build.
pub const H264_ENCODERS: &[&str] = &["h264_nvenc", "h264_amf", "h264_mf", "libopenh264"];

/// Encoders that can consume D3D11 hardware frames directly.
pub fn is_zero_copy_encoder(name: &str) -> bool {
    matches!(name, "h264_nvenc" | "h264_amf")
}

/// Which pipeline actually ended up in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineKind {
    /// GPU texture → GPU convert → hardware encoder. No CPU copy.
    ZeroCopyGpu,
    /// GPU texture → GPU convert → NV12 download → software encoder.
    GpuConvertCpuEncode,
    /// CPU readback → swscale → encoder (last-resort fallback).
    CpuFallback,
}

impl PipelineKind {
    pub fn label(self) -> &'static str {
        match self {
            PipelineKind::ZeroCopyGpu => "zero-copy gpu",
            PipelineKind::GpuConvertCpuEncode => "gpu convert, cpu encode",
            PipelineKind::CpuFallback => "cpu fallback",
        }
    }
}

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

/// Authoritatively checks whether a vendor hardware encoder can actually open
/// with a D3D11 frame pool on this machine.
///
/// `has_zero_copy_encoder` only inspects the build; an encoder can be compiled
/// in yet fail on the installed driver (for example NVENC without nvcuda.dll).
/// The opened encoder is dropped immediately; only success is reported.
pub fn probe_zero_copy(hw: &HwFrames, width: u32, height: u32, fps: u32, bitrate: u64) -> bool {
    let _ = ffmpeg::init();
    let _quiet = QuietLog::new();
    for name in H264_ENCODERS.iter().copied().filter(|n| is_zero_copy_encoder(n)) {
        if is_unusable(name) || ffmpeg_next::encoder::find_by_name(name).is_none() {
            continue;
        }
        if open(name, width, height, fps, bitrate, Some(hw), false).is_ok() {
            return true;
        }
    }
    false
}

/// Silences FFmpeg's logging while encoders are being probed/tried.
///
/// Encoder selection deliberately tries candidates that may fail on this
/// machine (e.g. NVENC without a driver); those failures are expected and only
/// noisy. The previous level is restored on drop.
struct QuietLog(ffmpeg::util::log::level::Level);

impl QuietLog {
    fn new() -> Self {
        let previous = ffmpeg::util::log::get_level()
            .unwrap_or(ffmpeg::util::log::level::Level::Info);
        ffmpeg::util::log::set_level(ffmpeg::util::log::level::Level::Quiet);
        Self(previous)
    }
}

impl Drop for QuietLog {
    fn drop(&mut self) {
        ffmpeg::util::log::set_level(self.0);
    }
}

/// Encodes frames to H.264 packets.
pub struct VideoEncoder {
    name: String,
    kind: PipelineKind,
    encoder: ffmpeg::encoder::video::Encoder,
    /// CPU colour converter, only for the BGRA fallback path.
    scaler: Option<scaling::Context>,
    /// Reused BGRA input frame for the fallback path.
    scratch: Option<VideoFrame>,
    /// Pixel format the scaler produces on the fallback path.
    out_format: Pixel,
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
    /// Keeps the hardware frame pool alive while the encoder references it.
    _hw_frames: Option<Arc<HwFrames>>,
}

impl VideoEncoder {
    /// Opens `preferred` (or the best available) encoder for CPU-supplied BGRA frames.
    pub fn new(
        preferred: Option<&str>,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u64,
    ) -> Result<Self> {
        Self::new_with_hw(preferred, width, height, fps, bitrate, None, false)
    }

    /// Opens the best encoder.
    ///
    /// `hw` supplies a D3D11 frame pool for the zero-copy path. `nv12_input`
    /// says the caller will feed NV12 frames (hardware or downloaded) instead of
    /// CPU BGRA, so no swscale stage is built.
    pub fn new_with_hw(
        preferred: Option<&str>,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u64,
        hw: Option<Arc<HwFrames>>,
        nv12_input: bool,
    ) -> Result<Self> {
        ffmpeg::init()?;

        // Candidate encoders that are compiled in may still fail on this driver;
        // keep those expected failures out of the console.
        let _quiet = QuietLog::new();

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

            // Only the vendor encoders can take D3D11 frames; everyone else
            // gets CPU frames even when a GPU pool exists.
            let use_hw = if is_zero_copy_encoder(name) {
                hw.as_deref()
            } else {
                None
            };

            match open(name, width, height, fps, bitrate, use_hw, !nv12_input) {
                Ok(encoder) => {
                    let kind = if use_hw.is_some() {
                        PipelineKind::ZeroCopyGpu
                    } else if nv12_input {
                        PipelineKind::GpuConvertCpuEncode
                    } else {
                        PipelineKind::CpuFallback
                    };
                    return Self::finish(encoder, width, height, fps, name, kind, hw);
                }
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
        kind: PipelineKind,
        hw: Option<Arc<HwFrames>>,
    ) -> Result<Self> {
        let (scaler, scratch, out_format) = if kind == PipelineKind::CpuFallback {
            let format = encoder.format();
            let scratch = Some(VideoFrame::new(Pixel::BGRA, width, height));
            if format == Pixel::BGRA || format == Pixel::RGBA {
                // This encoder (e.g. AMF) takes BGRA directly, so the CPU
                // fallback can skip swscale altogether.
                (None, scratch, format)
            } else {
                let scaler = scaling::Context::get(
                    Pixel::BGRA,
                    width,
                    height,
                    format,
                    width,
                    height,
                    scaling::Flags::BILINEAR,
                )?;
                (Some(scaler), scratch, format)
            }
        } else {
            (None, None, encoder.format())
        };

        if kind == PipelineKind::CpuFallback && scaler.is_none() {
            eprintln!("{name} accepts BGRA directly: skipping CPU colour conversion");
        }
        Ok(Self {
            name: name.to_string(),
            kind,
            encoder,
            scaler,
            scratch,
            out_format,
            width,
            height,
            interval_micros: 1_000_000 / fps.max(1) as i64,
            pending: VecDeque::new(),
            next_pts: 0,
            _hw_frames: hw,
        })
    }

    /// The time base every packet is stamped with: microseconds.
    pub fn time_base(&self) -> ffmpeg::Rational {
        ffmpeg::Rational(1, MICROS)
    }

    /// The name of the encoder actually in use.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Which pipeline is in use.
    pub fn kind(&self) -> PipelineKind {
        self.kind
    }

    /// Encodes an already-prepared frame (D3D11 hardware frame, or a CPU NV12
    /// frame on the download path). Takes ownership of `frame`.
    pub fn encode_av_frame(&mut self, frame: *mut ffmpeg_next::ffi::AVFrame, pts: i64) -> Result<Vec<Packet>> {
        let mut wrapped = unsafe { ffmpeg::util::frame::Video::wrap(frame) };
        wrapped.set_pts(Some(pts));

        self.pending.push_back(pts);
        self.encoder.send_frame(&wrapped)?;

        Ok(self.drain())
    }

    /// Encodes one BGRA frame at `pts` microseconds (CPU fallback path).
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

        let scratch = self
            .scratch
            .as_mut()
            .ok_or_else(|| anyhow!("encoder does not accept CPU frames"))?;

        let stride = scratch.stride(0);
        if pitch == row && stride == row {
            // No padding on either side: one copy is enough.
            let len = row * self.height as usize;
            scratch.data_mut(0)[..len].copy_from_slice(&bgra[..len]);
        } else {
            let dest = scratch.data_mut(0);
            for y in 0..self.height as usize {
                let src = &bgra[y * pitch..y * pitch + row];
                dest[y * stride..y * stride + row].copy_from_slice(src);
            }
        }
        scratch.set_pts(Some(pts));

        self.pending.push_back(pts);
        match self.scaler.as_mut() {
            Some(scaler) => {
                let mut converted = VideoFrame::new(self.out_format, self.width, self.height);
                scaler.run(scratch, &mut converted)?;
                converted.set_pts(Some(pts));
                self.encoder.send_frame(&converted)?;
            }
            None => {
                // Encoder takes BGRA natively; send the frame as-is.
                self.encoder.send_frame(scratch)?;
            }
        }

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
    hw: Option<&HwFrames>,
    prefer_bgra: bool,
) -> Result<ffmpeg::encoder::video::Encoder> {
    // Hardware frames: the encoder is fed D3D11 textures directly.
    if let Some(frames) = hw {
        let mut video = build_video(name, width, height, fps, bitrate, Pixel::D3D11)?;
        unsafe {
            let context = video.as_mut_ptr();
            (*context).hw_frames_ctx = ffmpeg_next::ffi::av_buffer_ref(frames.frames_ref());
        }
        return Ok(video.open_with(latency_options(name))?);
    }

    // On the CPU fallback a native BGRA encoder (AMF) skips swscale entirely,
    // so try BGRA first there; otherwise go straight to the YUV formats the
    // encoder is expected to take.
    let mut formats = vec![(Pixel::NV12, latency_options(name))];
    if prefer_bgra {
        formats.insert(0, (Pixel::BGRA, latency_options(name)));
    }
    formats.push((Pixel::YUV420P, ffmpeg::Dictionary::new()));

    for (format, options) in formats {
        if let Ok(encoder) = open_with(name, width, height, fps, bitrate, format, options) {
            return Ok(encoder);
        }
    }
    Err(anyhow!("encoder '{name}' rejected every supported pixel format"))
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
