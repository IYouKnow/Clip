//! Desktop capture via the Windows Graphics Capture API (`windows-capture`).
//!
//! Frames leave the capture thread already on the GPU: the captured BGRA
//! texture is converted to NV12 by a D3D11 video processor and handed to the
//! encoder as a hardware frame, so nothing full-frame crosses the CPU. On
//! machines without a usable GPU path, the raw BGRA buffer is forwarded instead.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use ffmpeg_next::ffi;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::ImageFormat;
use windows_capture::frame::{Frame, FrameBuffer};
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::encode;
use crate::hw::{self, GpuDevice, HwFrame, HwFrames, VideoProcessor};
use crate::session::SessionStats;

/// How long to wait for a frame with actual pixels before giving up.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Which pipeline the capture thread will feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePath {
    /// GPU texture → GPU convert → hardware encoder (no CPU copy).
    ZeroCopy,
    /// GPU convert → NV12 download → software encoder.
    GpuDownload,
    /// CPU readback + swscale → encoder.
    CpuFallback,
}

/// One-time pipeline description sent before the first frame.
pub struct CaptureSetup {
    pub path: CapturePath,
    pub hw_frames: Option<Arc<HwFrames>>,
    pub width: u32,
    pub height: u32,
}

/// A CPU NV12 `AVFrame` handed to the encoder (owner transfers on send).
pub struct Nv12Frame(pub *mut ffi::AVFrame);

// The frame is exclusively owned and only moved between threads.
unsafe impl Send for Nv12Frame {}

impl Nv12Frame {
    /// Relinquishes ownership so the encoder can take the frame.
    pub fn into_raw(mut self) -> *mut ffi::AVFrame {
        let frame = self.0;
        self.0 = std::ptr::null_mut();
        std::mem::forget(self);
        frame
    }
}

impl Drop for Nv12Frame {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        unsafe {
            let mut frame = self.0;
            ffi::av_frame_free(&mut frame);
        }
    }
}

/// The pixel data for one captured frame.
pub enum CapturedData {
    /// Zero-copy: an NV12 D3D11 frame borrowed from the hardware pool.
    Hw(HwFrame),
    /// Downloaded NV12 frame for a software encoder.
    Nv12(Nv12Frame),
    /// Raw BGRA pixels (fallback path), row padding preserved.
    Bgra { data: Vec<u8>, pitch: usize },
}

/// A frame forwarded to the encoder thread.
pub struct CapturedFrame {
    pub data: CapturedData,
    pub width: u32,
    pub height: u32,
    pub timestamp_micros: i64,
}

/// Messages sent from the capture thread to the encoder worker.
pub enum CaptureMessage {
    Setup(CaptureSetup),
    Frame(CapturedFrame),
}

/// Capture-side settings needed to pick the pipeline.
#[derive(Clone, Copy)]
pub struct CaptureConfig {
    pub fps: u32,
    pub bitrate: u64,
}

/// Handle to a running capture session.
pub struct CaptureHandle {
    control: CaptureControl<FrameForwarder, BoxError>,
}

impl CaptureHandle {
    /// Stops the capture thread and waits for it to finish.
    pub fn stop(self) -> anyhow::Result<()> {
        self.control.stop().map_err(|error| anyhow::anyhow!("{error}"))
    }
}

/// Starts capturing the primary monitor, forwarding frames to `sink`.
///
/// Dirty-region reporting is requested so a static desktop costs nothing; the
/// OS-side update interval is only a hint, so real pacing happens in the
/// handler. Both are optional and degrade gracefully on older Windows builds.
pub fn start_monitor_capture(
    sink: Sender<CaptureMessage>,
    stop: Arc<AtomicBool>,
    config: CaptureConfig,
    stats: Arc<SessionStats>,
) -> anyhow::Result<CaptureHandle> {
    let interval = Duration::from_micros(1_000_000 / config.fps.max(1) as u64);

    // Prefer dirty regions + a tight interval; fall back to defaults when the
    // OS build lacks support (start would otherwise fail outright).
    let attempts = [
        (
            DirtyRegionSettings::ReportOnly,
            MinimumUpdateIntervalSettings::Custom(interval),
        ),
        (
            DirtyRegionSettings::Default,
            MinimumUpdateIntervalSettings::Default,
        ),
    ];

    let mut last_error = None;
    for (dirty, min_interval) in attempts {
        let monitor = Monitor::primary()?;
        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::WithCursor,
            DrawBorderSettings::WithoutBorder,
            SecondaryWindowSettings::Default,
            min_interval,
            dirty,
            ColorFormat::Bgra8,
            (sink.clone(), stop.clone(), config, stats.clone()),
        );

        match FrameForwarder::start_free_threaded(settings) {
            Ok(control) => return Ok(CaptureHandle { control }),
            Err(error) => last_error = Some(anyhow::anyhow!("{error}")),
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("capture could not be started")))
}

/// Forwards each changed frame to the encoder thread.
struct FrameForwarder {
    sink: Sender<CaptureMessage>,
    stop: Arc<AtomicBool>,
    stats: Arc<SessionStats>,
    config: CaptureConfig,
    start: Instant,
    interval: Duration,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// Built on the first changed frame.
    pipeline: Option<Pipeline>,
    last_emit: Option<Instant>,
}

struct Pipeline {
    path: CapturePath,
    gpu: Option<GpuDevice>,
    hw_frames: Option<Arc<HwFrames>>,
    processor: Option<VideoProcessor>,
    width: u32,
    height: u32,
}

impl GraphicsCaptureApiHandler for FrameForwarder {
    type Flags = (Sender<CaptureMessage>, Arc<AtomicBool>, CaptureConfig, Arc<SessionStats>);
    type Error = BoxError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        // Capture must never outrank the foreground; run below normal priority.
        hw::set_current_thread_below_normal();
        let (sink, stop, config, stats) = ctx.flags;
        Ok(Self {
            sink,
            stop,
            stats,
            config,
            start: Instant::now(),
            interval: Duration::from_micros(1_000_000 / config.fps.max(1) as u64),
            device: ctx.device,
            context: ctx.device_context,
            pipeline: None,
            last_emit: None,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame<'_>,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.stop.load(Ordering::Relaxed) {
            capture_control.stop();
            return Ok(());
        }

        // Skip frames the compositor reports as unchanged.
        let changed = match frame.dirty_regions() {
            Ok(regions) => !regions.is_empty(),
            Err(_) => true,
        };
        if !changed {
            self.stats.record_idle();
            return Ok(());
        }

        // Cap the output rate even when the display refreshes faster.
        let now = Instant::now();
        if let Some(last) = self.last_emit {
            if now.duration_since(last) < self.interval {
                self.stats.record_dropped();
                return Ok(());
            }
        }
        self.last_emit = Some(now);

        let timestamp_micros = self.start.elapsed().as_micros() as i64;

        if self.pipeline.is_none() {
            self.pipeline = Some(build_pipeline(&self.device, &self.context, self.config, frame)?);
            let pipeline = self.pipeline.as_ref().unwrap();
            if self
                .sink
                .send(CaptureMessage::Setup(CaptureSetup {
                    path: pipeline.path,
                    hw_frames: pipeline.hw_frames.clone(),
                    width: pipeline.width,
                    height: pipeline.height,
                }))
                .is_err()
            {
                capture_control.stop();
                return Ok(());
            }
        }

        let pipeline = self.pipeline.as_ref().unwrap();
        let data = match pipeline.path {
            CapturePath::ZeroCopy => {
                let hw_frames = pipeline.hw_frames.as_ref().unwrap();
                let processor = pipeline.processor.as_ref().unwrap();
                let mut hw_frame = hw_frames.get_frame()?;
                unsafe {
                    processor.convert(frame.as_raw_texture(), hw_frame.slice_index())?;
                }
                // Submit the blit before the encoder picks the texture up.
                if let Some(gpu) = pipeline.gpu.as_ref() {
                    let _ = unsafe { gpu.context.Flush() };
                }
                hw_frame.set_pts(timestamp_micros);
                CapturedData::Hw(hw_frame)
            }
            CapturePath::GpuDownload => {
                let hw_frames = pipeline.hw_frames.as_ref().unwrap();
                let processor = pipeline.processor.as_ref().unwrap();
                let hw_frame = hw_frames.get_frame()?;
                unsafe {
                    processor.convert(frame.as_raw_texture(), hw_frame.slice_index())?;
                }
                let cpu = hw_frame.download_nv12()?;
                unsafe {
                    (*cpu).pts = timestamp_micros;
                }
                CapturedData::Nv12(Nv12Frame(cpu))
            }
            CapturePath::CpuFallback => {
                let (data, pitch) = {
                    let mut buffer = frame.buffer()?;
                    (buffer.as_raw_buffer().to_vec(), buffer.row_pitch() as usize)
                };
                CapturedData::Bgra { data, pitch }
            }
        };

        // Never block the capture thread: if the encoder is behind, drop the
        // frame (it returns to the pool) and count it.
        match self.sink.try_send(CaptureMessage::Frame(CapturedFrame {
            data,
            width: pipeline.width,
            height: pipeline.height,
            timestamp_micros,
        })) {
            Ok(()) => {}
            Err(crossbeam_channel::TrySendError::Full(_)) => {
                self.stats.record_dropped();
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                capture_control.stop();
            }
        }

        Ok(())
    }
}

/// Chooses and builds the strongest pipeline this machine supports.
fn build_pipeline(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    config: CaptureConfig,
    frame: &Frame<'_>,
) -> Result<Pipeline, BoxError> {
    let width = frame.width();
    let height = frame.height();

    // The GPU path is all-or-nothing and can be slow/flaky to probe; if it
    // failed once in this process, go straight to CPU so we neither stall nor
    // spam on every session.
    static GPU_DISABLED: AtomicBool = AtomicBool::new(false);
    if GPU_DISABLED.load(Ordering::Relaxed) {
        return Ok(cpu_pipeline(width, height));
    }

    let gpu = GpuDevice::from_parts(device.clone(), context.clone());

    // Create and validate the GPU conversion *before* building any FFmpeg
    // context, so a fallback never has to tear one down.
    let (texture, slices) = match hw::create_nv12_texture(&gpu, width, height, hw::FRAME_POOL_SIZE) {
        Ok(value) => value,
        Err(error) => {
            GPU_DISABLED.store(true, Ordering::Relaxed);
            log_gpu_fallback(&error);
            return Ok(cpu_pipeline(width, height));
        }
    };
    let processor = match VideoProcessor::new(&gpu, &texture, slices, width, height) {
        Ok(processor) => processor,
        Err(error) => {
            GPU_DISABLED.store(true, Ordering::Relaxed);
            log_gpu_fallback(&error);
            return Ok(cpu_pipeline(width, height));
        }
    };

    let hw_frames = HwFrames::from_texture(&gpu, texture, width, height, slices)?;

    // Ask the encoder directly: a vendor encoder can be compiled in yet
    // unusable on this driver.
    let path = if encode::probe_zero_copy(&hw_frames, width, height, config.fps, config.bitrate) {
        CapturePath::ZeroCopy
    } else {
        CapturePath::GpuDownload
    };
    Ok(Pipeline {
        path,
        gpu: Some(gpu),
        hw_frames: Some(Arc::new(hw_frames)),
        processor: Some(processor),
        width,
        height,
    })
}

/// Reports a GPU-path failure once per process.
fn log_gpu_fallback(error: &anyhow::Error) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        eprintln!(
            "GPU zero-copy unavailable on this display/driver, using CPU capture \
             (frames are read back and converted on the CPU): {error:#}"
        );
    });
}

/// A pipeline for the pure-CPU fallback, which needs no GPU objects.
fn cpu_pipeline(width: u32, height: u32) -> Pipeline {
    Pipeline {
        path: CapturePath::CpuFallback,
        gpu: None,
        hw_frames: None,
        processor: None,
        width,
        height,
    }
}

/// Grabs the first non-empty frame of the primary monitor and writes it as a PNG.
///
/// Blocking: returns once a frame with content has been saved, or fails if the
/// desktop looks blank/locked within `DEFAULT_TIMEOUT`.
pub fn save_primary_monitor_png(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let monitor = Monitor::primary()?;
    let saved = Arc::new(AtomicBool::new(false));

    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        (path.as_ref().to_path_buf(), saved.clone()),
    );

    let control = OneShotPng::start_free_threaded(settings)?;

    let deadline = Instant::now() + DEFAULT_TIMEOUT;
    let finished = loop {
        if control.is_finished() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    if finished {
        control.wait()?;
    } else {
        control.stop()?;
    }

    if !saved.load(Ordering::Relaxed) {
        anyhow::bail!(
            "no non-empty frame within {DEFAULT_TIMEOUT:?}: \
             the desktop appears blank or the session is locked"
        );
    }
    Ok(())
}

/// Capture handler that saves the first frame containing actual pixels.
///
/// Diagnostic only.
struct OneShotPng {
    path: PathBuf,
    saved: Arc<AtomicBool>,
    seen: u32,
}

impl GraphicsCaptureApiHandler for OneShotPng {
    type Flags = (PathBuf, Arc<AtomicBool>);
    type Error = BoxError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (path, saved) = ctx.flags;
        Ok(Self { path, saved, seen: 0 })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame<'_>,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let mut buffer = frame.buffer()?;
        self.seen += 1;

        if self.seen == 1 {
            eprintln!(
                "first frame: {}x{} row_pitch={} format={:?}",
                buffer.width(),
                buffer.height(),
                buffer.row_pitch(),
                buffer.color_format(),
            );
        }

        if has_content(&mut buffer) {
            buffer.save_as_image(&self.path, ImageFormat::Png)?;
            self.saved.store(true, Ordering::Relaxed);
            capture_control.stop();
        }

        Ok(())
    }
}

/// Returns true if any sampled pixel has a non-zero colour channel.
///
/// Padding bytes between rows can be non-zero on a blank frame, so rows are
/// walked using `row_pitch` and only `width * 4` bytes of each are examined.
fn has_content(buffer: &mut FrameBuffer<'_>) -> bool {
    let width = buffer.width() as usize;
    let height = buffer.height() as usize;
    let row_pitch = buffer.row_pitch() as usize;
    let row_bytes = width * 4;
    let raw = buffer.as_raw_buffer();

    let step = 16;
    let mut y = 0;
    while y < height {
        let row = y * row_pitch;
        if row + row_bytes <= raw.len() {
            let mut x = 0;
            while x < width {
                let i = row + x * 4;
                if raw[i] | raw[i + 1] | raw[i + 2] != 0 {
                    return true;
                }
                x += step;
            }
        }
        y += step;
    }
    false
}
