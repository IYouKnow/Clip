//! Ties capture to the encoder: a capture thread forwards frames to a worker
//! that owns the FFmpeg encoder and the replay buffer.
//!
//! All FFmpeg objects stay on the worker thread; the only things crossing
//! threads are frames (raw bytes, GPU frames, or downloaded NV12 frames),
//! commands, and resulting paths.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use crossbeam_channel::{bounded, select, Receiver, Sender};
use ffmpeg_next as ffmpeg;

use crate::audio::{self, AudioCaptureHandle, AudioTrack, PcmChunk};
use crate::audio_encode::AudioEncoder;
use crate::audio_mix::AudioMixer;
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
    /// Which audio tracks are being captured, once known.
    audio: Mutex<Option<String>>,
    audio_packets: AtomicU64,
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

    /// Which audio tracks are captured, e.g. "system + microphone", if any.
    pub fn audio(&self) -> Option<String> {
        self.audio.lock().ok().and_then(|value| value.clone())
    }

    /// Encoded audio packets currently held for a future clip.
    pub fn audio_packets(&self) -> u64 {
        self.audio_packets.load(Ordering::Relaxed)
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

    fn set_audio(&self, label: &str) {
        if let Ok(mut value) = self.audio.lock() {
            *value = Some(label.to_string());
        }
    }
}

/// Which audio sources to capture into the clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioConfig {
    /// Loopback capture of a playback device.
    pub system: bool,
    /// A recording device.
    pub microphone: bool,
    /// Bits per second for each AAC track.
    pub bitrate: u64,
    /// Endpoint id for the system track, or `None` for the default output.
    pub system_device: Option<String>,
    /// Endpoint id for the microphone, or `None` for the default input.
    pub microphone_device: Option<String>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            system: false,
            microphone: false,
            bitrate: 192_000,
            system_device: None,
            microphone_device: None,
        }
    }
}

impl AudioConfig {
    /// The tracks to capture, in a stable order.
    pub fn tracks(&self) -> Vec<AudioTrack> {
        let mut tracks = Vec::new();
        if self.system {
            tracks.push(AudioTrack::System);
        }
        if self.microphone {
            tracks.push(AudioTrack::Microphone);
        }
        tracks
    }

    /// The endpoint id requested for `track`, if any.
    pub fn device(&self, track: AudioTrack) -> Option<&str> {
        match track {
            AudioTrack::System => self.system_device.as_deref(),
            AudioTrack::Microphone => self.microphone_device.as_deref(),
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
    /// Which audio sources to capture alongside the video.
    pub audio: AudioConfig,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            encoder: None,
            fps: 60,
            bitrate: 20_000_000,
            buffer_seconds: 60.0,
            audio: AudioConfig::default(),
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

/// Per-track audio encoder and its slice of the replay buffer.
struct AudioState {
    track: AudioTrack,
    encoder: Option<AudioEncoder>,
    ring: Option<PacketRing>,
    /// Set when the encoder could not be opened, so it is not retried per chunk.
    failed: bool,
}

/// The optional mixed (system + microphone) audio stream.
struct MixState {
    /// Both sources are enabled, so a mix is wanted.
    enabled: bool,
    /// Set when the mixer could not be created, to avoid retrying per chunk.
    failed: bool,
    mixer: Option<AudioMixer>,
    ring: Option<PacketRing>,
}

impl MixState {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            failed: false,
            mixer: None,
            ring: None,
        }
    }
}

/// A running replay session.
pub struct ReplaySession {
    commands: Sender<Command>,
    stop: Arc<AtomicBool>,
    capture: Option<CaptureHandle>,
    audio: Vec<AudioCaptureHandle>,
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

        // One origin for both capture paths, so audio and video share a clock.
        let origin = Instant::now();

        // A single channel multiplexes every selected audio track's chunks. The
        // worker keeps the sender alive so `recv` blocks instead of disconnecting
        // when audio is off.
        let (audio_tx, audio_rx) = bounded::<(AudioTrack, PcmChunk)>(512);
        if let Some(label) = audio_label(&config.audio) {
            stats.set_audio(&label);
        }
        let mut audio = Vec::new();
        for track in config.audio.tracks() {
            let device = config.audio.device(track);
            match audio::start_capture(track, audio_tx.clone(), origin, device) {
                Ok(handle) => audio.push(handle),
                Err(error) => {
                    eprintln!("audio capture ({}) unavailable: {error:#}", track.label());
                }
            }
        }

        let worker_dir = clips_dir.clone();
        let worker_stats = stats.clone();
        let fps = config.fps;
        let bitrate = config.bitrate;
        let worker = std::thread::Builder::new()
            .name("trace-encoder".into())
            .spawn(move || {
                hw::set_current_thread_below_normal();
                if let Err(error) = run(
                    frame_rx,
                    command_rx,
                    audio_rx,
                    audio_tx,
                    config,
                    worker_dir,
                    worker_stats,
                ) {
                    eprintln!("encoder worker stopped: {error:#}");
                }
            })?;

        let capture = capture::start_monitor_capture(
            frame_tx,
            stop.clone(),
            capture::CaptureConfig { fps, bitrate },
            stats.clone(),
            origin,
        )?;

        Ok(Self {
            commands: command_tx,
            stop,
            capture: Some(capture),
            audio,
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
        for handle in self.audio.drain(..) {
            handle.stop();
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
        for handle in self.audio.drain(..) {
            handle.stop();
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

/// Encoder worker: consumes video frames and audio chunks, fills the rings, and
/// serves commands.
fn run(
    frames: Receiver<CaptureMessage>,
    commands: Receiver<Command>,
    audio: Receiver<(AudioTrack, PcmChunk)>,
    _audio_keepalive: Sender<(AudioTrack, PcmChunk)>,
    config: ReplayConfig,
    clips_dir: PathBuf,
    stats: Arc<SessionStats>,
) -> Result<()> {
    let mut encoder: Option<VideoEncoder> = None;
    let mut ring: Option<PacketRing> = None;
    let mut audio_states: Vec<AudioState> = config
        .audio
        .tracks()
        .into_iter()
        .map(|track| AudioState {
            track,
            encoder: None,
            ring: None,
            failed: false,
        })
        .collect();

    // A combined track is only useful when there is more than one source.
    let mut mix = MixState::new(config.audio.system && config.audio.microphone);

    // Once capture ends the frame channel closes; the worker must keep serving
    // commands (Stop/Finish) afterwards, or a `stop()` racing the close would
    // wait forever for a reply that never comes.
    let mut frames_open = true;
    loop {
        if !frames_open {
            match commands.recv() {
                Ok(command) => {
                    if handle_command(
                        command,
                        &mut encoder,
                        &mut ring,
                        &mut audio_states,
                        &mut mix,
                        &clips_dir,
                        &stats,
                    ) {
                        break;
                    }
                }
                Err(_) => break,
            }
            continue;
        }

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
                Err(_) => frames_open = false,
            },
            recv(audio) -> message => {
                if let Ok((track, chunk)) = message {
                    encode_audio(&mut audio_states, track, &chunk, &config, &stats, &mut mix);
                }
            },
            recv(commands) -> message => match message {
                Ok(command) => {
                    if handle_command(
                        command,
                        &mut encoder,
                        &mut ring,
                        &mut audio_states,
                        &mut mix,
                        &clips_dir,
                        &stats,
                    ) {
                        break;
                    }
                }
                Err(_) => break,
            },
        }
    }

    Ok(())
}

/// Handles one worker command; returns `true` when the worker should stop.
fn handle_command(
    command: Command,
    encoder: &mut Option<VideoEncoder>,
    ring: &mut Option<PacketRing>,
    audio: &mut [AudioState],
    mix: &mut MixState,
    clips_dir: &Path,
    stats: &SessionStats,
) -> bool {
    match command {
        Command::Save { seconds, reply } => {
            let _ = reply.send(save(encoder, ring, audio, mix, clips_dir, seconds));
            false
        }
        Command::Finish { seconds, reply } => {
            flush(encoder, ring, audio, mix, stats);
            let _ = reply.send(save(encoder, ring, audio, mix, clips_dir, seconds));
            true
        }
        Command::Stop { reply } => {
            flush(encoder, ring, audio, mix, stats);
            let _ = reply.send(Ok(()));
            true
        }
    }
}

/// Encodes one device chunk into its track's ring (and the mix), opening encoders
/// lazily once the device format is known.
fn encode_audio(
    states: &mut [AudioState],
    track: AudioTrack,
    chunk: &PcmChunk,
    config: &ReplayConfig,
    stats: &SessionStats,
    mix: &mut MixState,
) {
    if let Some(state) = states.iter_mut().find(|state| state.track == track) {
        if !state.failed {
            if state.encoder.is_none() {
                match AudioEncoder::new(chunk, config.audio.bitrate) {
                    Ok(encoder) => {
                        state.ring =
                            Some(PacketRing::new(encoder.time_base(), config.buffer_seconds));
                        state.encoder = Some(encoder);
                    }
                    Err(error) => {
                        eprintln!("audio encoding ({}) unavailable: {error:#}", track.label());
                        state.failed = true;
                    }
                }
            }

            if let (Some(encoder), Some(ring)) = (state.encoder.as_mut(), state.ring.as_mut()) {
                match encoder.encode(chunk) {
                    Ok(packets) => {
                        for packet in packets {
                            ring.push(packet);
                            stats.audio_packets.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(error) => eprintln!("audio encode error: {error:#}"),
                }
            }
        }
    }

    // The mix is independent of the per-track encoders.
    if mix.enabled && !mix.failed {
        if mix.mixer.is_none() {
            match AudioMixer::new(config.audio.bitrate) {
                Ok(mixer) => {
                    mix.ring = Some(PacketRing::new(mixer.time_base(), config.buffer_seconds));
                    mix.mixer = Some(mixer);
                }
                Err(error) => {
                    eprintln!("audio mix unavailable: {error:#}");
                    mix.failed = true;
                }
            }
        }
        if let Some(mixer) = mix.mixer.as_mut() {
            match mixer.push(track, chunk) {
                Ok(packets) => {
                    if let Some(ring) = mix.ring.as_mut() {
                        for packet in packets {
                            ring.push(packet);
                            stats.audio_packets.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(error) => eprintln!("audio mix error: {error:#}"),
            }
        }
    }
}

fn flush(
    encoder: &mut Option<VideoEncoder>,
    ring: &mut Option<PacketRing>,
    audio: &mut [AudioState],
    mix: &mut MixState,
    stats: &SessionStats,
) {
    if let (Some(encoder), Some(ring)) = (encoder.as_mut(), ring.as_mut()) {
        if let Ok(packets) = encoder.flush() {
            for packet in packets {
                ring.push(packet);
                stats.packets.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    for state in audio.iter_mut() {
        let (Some(encoder), Some(ring)) = (state.encoder.as_mut(), state.ring.as_mut()) else {
            continue;
        };
        if let Ok(packets) = encoder.flush() {
            for packet in packets {
                ring.push(packet);
                stats.audio_packets.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    if let Some(mixer) = mix.mixer.as_mut() {
        if let Ok(packets) = mixer.flush() {
            if let Some(ring) = mix.ring.as_mut() {
                for packet in packets {
                    ring.push(packet);
                    stats.audio_packets.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

fn save(
    encoder: &mut Option<VideoEncoder>,
    ring: &mut Option<PacketRing>,
    audio: &mut [AudioState],
    mix: &mut MixState,
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
    let video_packets = ring.slice_mut(range);

    // The clip's video window, so each audio track can be cut to match.
    let start_pts = video_packets
        .first()
        .and_then(|packet| packet.pts())
        .unwrap_or(0);
    let end_pts = video_packets
        .last()
        .and_then(|packet| packet.pts())
        .unwrap_or(i64::MAX);

    let mut audio_streams: Vec<(&ffmpeg::encoder::audio::Encoder, &mut [ffmpeg::Packet])> =
        Vec::new();

    // The mixed track goes first, so players default to the combined audio.
    if let (Some(mixer), Some(mix_ring)) = (mix.mixer.as_ref(), mix.ring.as_mut()) {
        let mix_range = mix_ring.range_for_window(start_pts, end_pts);
        if !mix_range.is_empty() {
            audio_streams.push((mixer.inner(), mix_ring.slice_mut(mix_range)));
        }
    }

    for state in audio.iter_mut() {
        let (Some(audio_encoder), Some(audio_ring)) =
            (state.encoder.as_ref(), state.ring.as_mut())
        else {
            continue;
        };
        let audio_range = audio_ring.range_for_window(start_pts, end_pts);
        if audio_range.is_empty() {
            continue;
        }
        audio_streams.push((audio_encoder.inner(), audio_ring.slice_mut(audio_range)));
    }

    mux::write_mp4(
        &path,
        (encoder.inner(), video_packets),
        &mut audio_streams,
        time_base,
    )?;
    Ok(path)
}

/// Human-readable description of the devices being captured, e.g.
/// `system: Speakers (Realtek) + microphone: Headset Microphone`, or `None` when
/// audio is off. Falls back to the requested id when a name can't be resolved.
fn audio_label(config: &AudioConfig) -> Option<String> {
    let tracks = config.tracks();
    if tracks.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for track in tracks {
        let name = match config.device(track) {
            Some(id) => audio::list_devices(track)
                .ok()
                .and_then(|devices| devices.into_iter().find(|device| device.id == id))
                .map(|device| device.name)
                .unwrap_or_else(|| id.to_string()),
            None => audio::default_device_info(track)
                .map(|device| device.name)
                .unwrap_or_else(|_| "default".to_string()),
        };
        parts.push(format!("{}: {name}", track.label()));
    }
    Some(parts.join(" + "))
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
