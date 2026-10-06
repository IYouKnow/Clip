//! Ties capture to the encoder: a capture thread forwards frames to a worker
//! that owns the FFmpeg encoder and the replay buffer.
//!
//! All FFmpeg objects stay on the worker thread; the only things crossing
//! threads are frames (raw bytes, GPU frames, or downloaded NV12 frames),
//! commands, and resulting paths.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use crossbeam_channel::{bounded, select, Receiver, Sender};

use crate::capture::{
    self, CaptureHandle, CaptureMessage, CapturePath, CapturedData,
};
use crate::encode::{PipelineKind, VideoEncoder};
use crate::hw;
use crate::mux;
use crate::ring::PacketRing;

/// Live counters shared between the worker, the capture thread and the UI.
#[derive(Default)]
pub struct SessionStats {
    encoder: Mutex<Option<String>>,
    pipeline: Mutex<Option<PipelineKind>>,
    frames: AtomicU64,
    packets: AtomicU64,
    dropped: AtomicU64,
    /// Frames the compositor reported as unchanged (and so cost nothing).
    idle: AtomicU64,
}

impl SessionStats {
    /// Name of the encoder the worker actually opened, once known.
    pub fn encoder(&self) -> Option<String> {
        self.encoder.lock().ok().and_then(|value| value.clone())
    }

    /// Which pipeline is in use, once the encoder is open.
    pub fn pipeline(&self) -> Option<PipelineKind> {
        self.pipeline.lock().ok().and_then(|value| *value)
    }

    /// Frames received from capture.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Encoded packets currently held for a future clip.
    pub fn packets(&self) -> u64 {
        self.packets.load(Ordering::Relaxed)
    }

    /// Frames discarded because the encoder was behind.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Frames skipped because the screen had not changed.
    pub fn idle(&self) -> u64 {
        self.idle.load(Ordering::Relaxed)
    }

    pub(crate) fn record_dropped(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_idle(&self) {
        self.idle.fetch_add(1, Ordering::Relaxed);
    }

    fn set_encoder(&self, name: &str, kind: PipelineKind) {
        if let Ok(mut value) = self.encoder.lock() {
            *value = Some(name.to_string());
        }
        if let Ok(mut value) = self.pipeline.lock() {
            *value = Some(kind);
        }
    }
}

/// How the replay buffer should be encoded.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// Encoder name, or `None` to pick the best available.
    pub encoder: Option<String>,
    pub fps: u32,
    pub bitrate: u64,
    /// How many seconds of encoded video to retain.
    pub buffer_seconds: f64,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            encoder: None,
            fps: 60,
            bitrate: 20_000_000,
            buffer_seconds: 60.0,
        }
    }
}

/// Commands accepted by the encoder worker.
enum Command {
    Save {
        seconds: f64,
        reply: Sender<Result<PathBuf>>,
    },
    /// Flush the encoder, write a final clip, then shut down. Used when the
    /// session is ending (and by headless runs).
    Finish {
        seconds: f64,
        reply: Sender<Result<PathBuf>>,
    },
    Stop {
        reply: Sender<Result<()>>,
    },
}

/// A running replay session.
pub struct ReplaySession {
    commands: Sender<Command>,
    stop: Arc<AtomicBool>,
    capture: Option<CaptureHandle>,
    worker: Option<std::thread::JoinHandle<()>>,
    stats: Arc<SessionStats>,
}

impl ReplaySession {
    /// Starts capturing the primary monitor and filling the replay buffer.
    pub fn start(config: ReplayConfig, clips_dir: PathBuf) -> Result<Self> {
        // Bounded: if the encoder ever falls behind, frames are dropped rather
        // than piling up in memory.
        let (frame_tx, frame_rx) = bounded::<CaptureMessage>(4);
        let (command_tx, command_rx) = bounded::<Command>(4);
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(SessionStats::default());

        let worker_dir = clips_dir.clone();
        let worker_stats = stats.clone();
        let fps = config.fps;
        let bitrate = config.bitrate;
        let worker = std::thread::Builder::new()
            .name("clipper23-encoder".into())
            .spawn(move || {
                hw::set_current_thread_below_normal();
                if let Err(error) = run(frame_rx, command_rx, config, worker_dir, worker_stats) {
                    eprintln!("encoder worker stopped: {error:#}");
                }
            })?;

        let capture = capture::start_monitor_capture(
            frame_tx,
            stop.clone(),
            capture::CaptureConfig { fps, bitrate },
            stats.clone(),
        )?;

        Ok(Self {
            commands: command_tx,
            stop,
            capture: Some(capture),
            worker: Some(worker),
            stats,
        })
    }

    /// Live counters for status reporting.
    pub fn stats(&self) -> Arc<SessionStats> {
        self.stats.clone()
    }

    /// Writes the last `seconds` of buffered video to a new clip and returns its path.
    ///
    /// Non-destructive: recording continues afterwards. Because the encoder is
    /// not flushed, packets it is still holding are not included.
    pub fn save(&self, seconds: f64) -> Result<PathBuf> {
        let (reply, result) = bounded(1);
        self.commands
            .send(Command::Save { seconds, reply })
            .map_err(|_| anyhow!("replay session is not running"))?;
        result.recv()?
    }

    /// Ends the session: flushes the encoder, writes a final clip, and returns it.
    pub fn finish(mut self, seconds: f64) -> Result<PathBuf> {
        let (reply, result) = bounded(1);
        self.commands
            .send(Command::Finish { seconds, reply })
            .map_err(|_| anyhow!("replay session is not running"))?;
        let clip = result.recv()?;

        self.stop.store(true, Ordering::Relaxed);
        if let Some(capture) = self.capture.take() {
            let _ = capture.stop();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        clip
    }

    /// Stops capture and shuts the encoder down.
    pub fn stop(mut self) -> Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(capture) = self.capture.take() {
            let _ = capture.stop();
        }

        let (reply, result) = bounded(1);
        if self.commands.send(Command::Stop { reply }).is_ok() {
            let _ = result.recv();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        Ok(())
    }
}

impl Drop for ReplaySession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(capture) = self.capture.take() {
            let _ = capture.stop();
        }
        let (reply, _result) = bounded(1);
        let _ = self.commands.send(Command::Stop { reply });
    }
}

/// Encoder worker: consumes frames, fills the ring, and serves commands.
fn run(
    frames: Receiver<CaptureMessage>,
    commands: Receiver<Command>,
    config: ReplayConfig,
    clips_dir: PathBuf,
    stats: Arc<SessionStats>,
) -> Result<()> {
    let mut encoder: Option<VideoEncoder> = None;
    let mut ring: Option<PacketRing> = None;

    loop {
        select! {
            recv(frames) -> message => match message {
                Ok(CaptureMessage::Setup(setup)) => {
                    let (hw_frames, nv12_input) = match setup.path {
                        CapturePath::ZeroCopy => (setup.hw_frames.clone(), true),
                        CapturePath::GpuDownload => (None, true),
                        CapturePath::CpuFallback => (None, false),
                    };
                    let opened = VideoEncoder::new_with_hw(
                        config.encoder.as_deref(),
                        setup.width,
                        setup.height,
                        config.fps,
                        config.bitrate,
                        hw_frames,
                        nv12_input,
                    )?;
                    stats.set_encoder(opened.name(), opened.kind());
                    ring = Some(PacketRing::new(opened.time_base(), config.buffer_seconds));
                    encoder = Some(opened);
                }
                Ok(CaptureMessage::Frame(frame)) => {
                    let Some(encoder) = encoder.as_mut() else { continue };
                    stats.frames.fetch_add(1, Ordering::Relaxed);

                    let packets = match frame.data {
                        CapturedData::Hw(hw_frame) => {
                            encoder.encode_av_frame(hw_frame.into_raw(), frame.timestamp_micros)?
                        }
                        CapturedData::Nv12(nv12) => {
                            encoder.encode_av_frame(nv12.into_raw(), frame.timestamp_micros)?
                        }
                        CapturedData::Bgra { data, pitch } => {
                            encoder.encode_bgra(&data, pitch, frame.timestamp_micros)?
                        }
                    };

                    for packet in packets {
                        ring.as_mut().unwrap().push(packet);
                        stats.packets.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(_) => break,
            },
            recv(commands) -> message => match message {
                Ok(Command::Save { seconds, reply }) => {
                    let _ = reply.send(save(&mut encoder, &mut ring, &clips_dir, seconds));
                }
                Ok(Command::Finish { seconds, reply }) => {
                    flush(&mut encoder, &mut ring, &stats);
                    let _ = reply.send(save(&mut encoder, &mut ring, &clips_dir, seconds));
                    break;
                }
                Ok(Command::Stop { reply }) => {
                    flush(&mut encoder, &mut ring, &stats);
                    let _ = reply.send(Ok(()));
                    break;
                }
                Err(_) => break,
            },
        }
    }

    Ok(())
}

fn flush(encoder: &mut Option<VideoEncoder>, ring: &mut Option<PacketRing>, stats: &SessionStats) {
    if let (Some(encoder), Some(ring)) = (encoder.as_mut(), ring.as_mut()) {
        if let Ok(packets) = encoder.flush() {
            for packet in packets {
                ring.push(packet);
                stats.packets.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

fn save(
    encoder: &mut Option<VideoEncoder>,
    ring: &mut Option<PacketRing>,
    clips_dir: &Path,
    seconds: f64,
) -> Result<PathBuf> {
    let (Some(encoder), Some(ring)) = (encoder.as_mut(), ring.as_mut()) else {
        bail!("no frames captured yet");
    };

    // Note: the encoder is deliberately not flushed here — sending EOF would
    // end the stream and stop further recording. A few in-flight packets may
    // not be in the buffer yet, which is fine for a replay buffer.
    let range = ring.snapshot_range(seconds);
    if range.is_empty() {
        bail!("replay buffer is empty");
    }

    std::fs::create_dir_all(clips_dir)?;
    let path = clips_dir.join(format!("clip-{}.mp4", timestamp()));
    let time_base = ring.time_base();
    let packets = ring.slice_mut(range);
    mux::write_mp4(&path, encoder.inner(), time_base, packets)?;
    Ok(path)
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
