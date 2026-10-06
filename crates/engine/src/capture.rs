//! Desktop capture via the Windows Graphics Capture API (`windows-capture`).
//!
//! Two entry points: a one-shot single-frame grab (used to prove capture works)
//! and a continuous session that forwards frames to another thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::ImageFormat;
use windows_capture::frame::{Frame, FrameBuffer};
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

/// How long to wait for a frame with actual pixels before giving up.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A captured frame, in the device's BGRA layout with row padding preserved.
#[derive(Debug)]
pub struct CapturedFrame {
    /// Raw pixels, `pitch * height` bytes.
    pub data: Vec<u8>,
    /// Bytes per row, including any padding.
    pub pitch: usize,
    pub width: u32,
    pub height: u32,
    pub timestamp_micros: i64,
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
/// `fps` throttles delivery at the OS level so the encoder is not handed a
/// frame for every compositor update (which on a high-refresh display is far
/// more work than is needed for a 60 fps clip).
pub fn start_monitor_capture(
    sink: Sender<CapturedFrame>,
    stop: Arc<AtomicBool>,
    fps: u32,
) -> anyhow::Result<CaptureHandle> {
    let monitor = Monitor::primary()?;
    let interval = Duration::from_micros(1_000_000 / fps.max(1) as u64);
    let settings = Settings::new(
        monitor,
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Custom(interval),
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        (sink, stop),
    );

    let control = FrameForwarder::start_free_threaded(settings)?;
    Ok(CaptureHandle { control })
}

/// Forwards each changed frame to the encoder thread.
struct FrameForwarder {
    sink: Sender<CapturedFrame>,
    stop: Arc<AtomicBool>,
    start: Instant,
}

impl GraphicsCaptureApiHandler for FrameForwarder {
    type Flags = (Sender<CapturedFrame>, Arc<AtomicBool>);
    type Error = BoxError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (sink, stop) = ctx.flags;
        Ok(Self {
            sink,
            stop,
            start: Instant::now(),
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

        // A single copy of the raw buffer: row padding is kept and handled by
        // the encoder, which avoids repacking the frame first.
        let (data, pitch, width, height) = {
            let mut buffer = frame.buffer()?;
            let pitch = buffer.row_pitch() as usize;
            let width = buffer.width();
            let height = buffer.height();
            (buffer.as_raw_buffer().to_vec(), pitch, width, height)
        };
        let timestamp_micros = self.start.elapsed().as_micros() as i64;

        // If the consumer is gone, stop capturing.
        if self
            .sink
            .send(CapturedFrame {
                data,
                pitch,
                width,
                height,
                timestamp_micros,
            })
            .is_err()
        {
            capture_control.stop();
        }

        Ok(())
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
/// Diagnostic only: reading the frame buffer copies pixels back from the GPU,
/// which the real encode pipeline avoids by using the D3D11 texture directly.
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
        Ok(Self {
            path,
            saved,
            seen: 0,
        })
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
