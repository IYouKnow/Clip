//! Audio capture: system output (WASAPI loopback) and microphone.
//!
//! Each source is captured on its own thread and forwarded as raw PCM chunks.
//! Samples are kept in whatever format the device produces; conversion and
//! encoding happen later, when a clip is written.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

/// How often the capture loop polls the device.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Which audio source to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioTrack {
    /// Everything the system is playing: loopback capture of the default
    /// playback (render) device.
    System,
    /// The default recording device (microphone).
    Microphone,
}

impl AudioTrack {
    pub fn label(self) -> &'static str {
        match self {
            AudioTrack::System => "system",
            AudioTrack::Microphone => "microphone",
        }
    }

    /// Which default endpoint to open.
    ///
    /// System audio comes from the render endpoint; capturing a render endpoint
    /// is what puts WASAPI into loopback mode.
    fn endpoint(self) -> Direction {
        match self {
            AudioTrack::System => Direction::Render,
            AudioTrack::Microphone => Direction::Capture,
        }
    }
}

/// Raw audio produced by a device, in the device's own format.
#[derive(Debug, Clone)]
pub struct PcmChunk {
    pub data: Vec<u8>,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub is_float: bool,
    /// Microseconds since capture started.
    pub timestamp_micros: i64,
}

/// Handle to a running capture thread.
pub struct AudioCaptureHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioCaptureHandle {
    /// Stops capturing and waits for the thread to finish.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for AudioCaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A device and the format it exposes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceInfo {
    pub track: String,
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub is_float: bool,
}

/// Starts capturing `track`, forwarding PCM chunks to `sink`.
pub fn start_capture(track: AudioTrack, sink: Sender<PcmChunk>) -> Result<AudioCaptureHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();

    let thread = std::thread::Builder::new()
        .name(format!("clipper23-audio-{}", track.label()))
        .spawn(move || {
            if let Err(error) = capture_loop(track, sink, thread_stop) {
                eprintln!("audio capture ({}): {error:#}", track.label());
            }
        })?;

    Ok(AudioCaptureHandle {
        stop,
        thread: Some(thread),
    })
}

/// Reports the default device and format for `track`.
pub fn default_device_info(track: AudioTrack) -> Result<DeviceInfo> {
    wasapi::initialize_mta().ok()?;
    let enumerator = DeviceEnumerator::new()?;
    let device = enumerator.get_default_device(&track.endpoint())?;
    let name = device.get_friendlyname().map_err(|error| anyhow!("{error}"))?;
    let format = device.get_iaudioclient()?.get_mixformat()?;

    Ok(DeviceInfo {
        track: track.label().to_string(),
        name,
        sample_rate: format.get_samplespersec(),
        channels: format.get_nchannels(),
        bits_per_sample: format.get_bitspersample(),
        is_float: matches!(format.get_subformat(), Ok(SampleType::Float)),
    })
}

fn capture_loop(track: AudioTrack, sink: Sender<PcmChunk>, stop: Arc<AtomicBool>) -> Result<()> {
    // COM must be initialised on this thread.
    wasapi::initialize_mta().ok()?;

    let enumerator = DeviceEnumerator::new()?;
    let device = enumerator.get_default_device(&track.endpoint())?;
    let mut client = device.get_iaudioclient()?;
    let format: WaveFormat = client.get_mixformat()?;

    let sample_rate = format.get_samplespersec();
    let channels = format.get_nchannels();
    let bits_per_sample = format.get_bitspersample();
    let is_float = matches!(format.get_subformat(), Ok(SampleType::Float));

    let (_, min_period) = client.get_device_period()?;
    client.initialize_client(
        &format,
        // Both tracks are captured; the render endpoint being opened is what
        // selects loopback for system audio.
        &Direction::Capture,
        &StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        },
    )?;

    let capture = client.get_audiocaptureclient()?;
    let mut queue: VecDeque<u8> = VecDeque::new();

    client.start_stream()?;

    let mut frames_delivered: u64 = 0;
    let mut closed = false;

    while !stop.load(Ordering::Relaxed) {
        capture.read_from_device_to_deque(&mut queue)?;

        if !queue.is_empty() {
            let data: Vec<u8> = queue.drain(..).collect();
            let frames = data.len() as u64 / (channels as u64 * (bits_per_sample as u64 / 8)).max(1);
            frames_delivered += frames;

            let chunk = PcmChunk {
                data,
                sample_rate,
                channels,
                bits_per_sample,
                is_float,
                timestamp_micros: frames_delivered as i64 * 1_000_000 / sample_rate.max(1) as i64,
            };

            // A closed channel means the session shut down.
            if sink.send(chunk).is_err() {
                closed = true;
                break;
            }
        }

        std::thread::sleep(POLL_INTERVAL);
    }

    client.stop_stream()?;
    let _ = closed;
    Ok(())
}
