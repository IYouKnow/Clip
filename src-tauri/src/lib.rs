//! Tauri shell: exposes the replay engine and clip library to the UI.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossbeam_channel::unbounded;
use trace_engine::audio::{self, AudioCaptureHandle, AudioTrack, DeviceInfo, PcmChunk};
use trace_engine::encode;
use trace_engine::session::{AudioConfig, ReplayConfig, ReplaySession, SessionStats};
use trace_library::{self as library, ClipSummary};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

mod hook;
mod hotkeys;
use hotkeys::{HotkeyAction, Hotkeys};
mod settings;
mod tray;

/// Folder name under the user's Videos directory.
const CLIPS_FOLDER: &str = "Trace";

/// AAC bitrate used for each captured audio track.
const AUDIO_BITRATE: u64 = 192_000;

/// Which audio sources to capture into a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AudioSource {
    Off,
    System,
    Microphone,
    Both,
}

impl Default for AudioSource {
    fn default() -> Self {
        AudioSource::System
    }
}

impl AudioSource {
    /// Maps the user-facing choice onto the engine's capture flags, keeping only
    /// the endpoint ids for the sources that are actually enabled.
    fn config(
        self,
        system_device: Option<String>,
        microphone_device: Option<String>,
    ) -> AudioConfig {
        let (system, microphone) = match self {
            AudioSource::Off => (false, false),
            AudioSource::System => (true, false),
            AudioSource::Microphone => (false, true),
            AudioSource::Both => (true, true),
        };
        AudioConfig {
            system,
            microphone,
            bitrate: AUDIO_BITRATE,
            system_device: if system { system_device } else { None },
            microphone_device: if microphone { microphone_device } else { None },
        }
    }
}

/// The selectable audio endpoints for the settings UI.
#[derive(Debug, Clone, Serialize)]
struct AudioDevices {
    system: Vec<DeviceInfo>,
    microphone: Vec<DeviceInfo>,
}

/// User-configurable options.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    /// Seconds of video kept in the replay buffer.
    buffer_seconds: u32,
    fps: u32,
    bitrate: u64,
    /// Preferred encoder, or `None` to auto-select.
    encoder: Option<String>,
    /// Which audio sources to capture.
    #[serde(default)]
    audio_source: AudioSource,
    /// Playback endpoint id to capture, or `None` for the system default.
    #[serde(default)]
    system_device: Option<String>,
    /// Microphone endpoint id to capture, or `None` for the system default.
    #[serde(default)]
    microphone_device: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            buffer_seconds: 60,
            fps: 60,
            bitrate: 20_000_000,
            encoder: None,
            audio_source: AudioSource::default(),
            system_device: None,
            microphone_device: None,
        }
    }
}

/// Everything the UI needs to render the home screen.
#[derive(Debug, Clone, Serialize)]
struct Status {
    replaying: bool,
    encoder: Option<String>,
    /// Which capture→encode pipeline is active (e.g. "zero-copy gpu").
    pipeline: Option<String>,
    frames: u64,
    packets: u64,
    dropped: u64,
    /// Frames skipped because the screen had not changed.
    idle: u64,
    /// Active audio devices (e.g. "system: Speakers + microphone: Headset Mic").
    audio: Option<String>,
    buffer_seconds: u32,
    fps: u32,
    bitrate: u64,
    clips_dir: String,
    available_encoders: Vec<String>,
}

/// A running microphone test: a capture handle plus the level readouts.
struct MicTest {
    handle: AudioCaptureHandle,
    consumer: Option<std::thread::JoinHandle<()>>,
    /// Peak since the last status poll (f32 bits), so the meter is peak-hold.
    level: Arc<AtomicU32>,
    /// Peak over the whole test (f32 bits).
    peak: Arc<AtomicU32>,
    device: String,
}

/// Microphone-test level reported to the settings UI.
#[derive(Debug, Clone, Serialize)]
struct MicTestStatus {
    active: bool,
    /// Peak over the poll interval, 0.0..=1.0.
    level: f32,
    /// Peak over the whole test, 0.0..=1.0.
    peak: f32,
    device: Option<String>,
}

struct AppState {
    session: Mutex<Option<ReplaySession>>,
    stats: Mutex<Option<Arc<SessionStats>>>,
    settings: Mutex<Settings>,
    /// Cached so the 1 Hz status poll does no filesystem or codec work.
    clips_dir: Mutex<Option<PathBuf>>,
    encoders: Mutex<Option<Vec<String>>>,
    /// Configured bindings, persisted to `hotkeys.json`.
    hotkeys: Mutex<Hotkeys>,
    /// The running microphone test, if any.
    mic_test: Mutex<Option<MicTest>>,
    /// Tray icon and menu handles, populated in `setup`.
    tray: Mutex<Option<tray::TrayHandles>>,
    /// Set right before `app.exit` so the close handler stops hiding the window.
    quitting: AtomicBool,
}

fn clips_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .video_dir()
        .map_err(|error| error.to_string())?
        .join(CLIPS_FOLDER);
    library::ensure_dir(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

impl AppState {
    fn cached_clips_dir(&self, app: &AppHandle) -> String {
        if let Ok(mut cache) = self.clips_dir.lock() {
            if let Some(dir) = cache.as_ref() {
                return dir.to_string_lossy().to_string();
            }
            if let Ok(dir) = clips_dir(app) {
                let rendered = dir.to_string_lossy().to_string();
                *cache = Some(dir);
                return rendered;
            }
        }
        String::new()
    }

    fn cached_encoders(&self) -> Vec<String> {
        if let Ok(mut cache) = self.encoders.lock() {
            if let Some(list) = cache.as_ref() {
                return list.clone();
            }
            let list: Vec<String> = encode::h264_encoders()
                .into_iter()
                .filter(|info| info.available)
                .map(|info| info.name)
                .collect();
            *cache = Some(list.clone());
            return list;
        }
        Vec::new()
    }

    fn status(&self, app: &AppHandle) -> Status {
        let settings = self.settings.lock().map(|s| s.clone()).unwrap_or_default();
        let stats = self.stats.lock().ok().and_then(|s| s.clone());
        let stats = stats.as_deref();
        Status {
            replaying: stats.is_some(),
            encoder: stats.and_then(SessionStats::encoder),
            pipeline: stats.and_then(|s| s.pipeline().map(|kind| kind.label().to_string())),
            frames: stats.map(SessionStats::frames).unwrap_or(0),
            packets: stats.map(SessionStats::packets).unwrap_or(0),
            dropped: stats.map(SessionStats::dropped).unwrap_or(0),
            idle: stats.map(SessionStats::idle).unwrap_or(0),
            audio: stats.and_then(SessionStats::audio),
            buffer_seconds: settings.buffer_seconds,
            fps: settings.fps,
            bitrate: settings.bitrate,
            clips_dir: self.cached_clips_dir(app),
            available_encoders: self.cached_encoders(),
        }
    }
}

/// Starts capture if it is not already running and returns the fresh status.
fn start_replay_inner(app: &AppHandle, state: &AppState) -> Result<Status, String> {
    {
        let mut session = state.session.lock().map_err(|error| error.to_string())?;
        if session.is_none() {
            let settings = state.settings.lock().map(|s| s.clone()).unwrap_or_default();
            let dir = {
                let mut cache = state.clips_dir.lock().map_err(|e| e.to_string())?;
                match cache.as_ref() {
                    Some(dir) => dir.clone(),
                    None => {
                        let dir = clips_dir(app)?;
                        *cache = Some(dir.clone());
                        dir
                    }
                }
            };
            let config = ReplayConfig {
                encoder: settings.encoder.clone(),
                fps: settings.fps,
                bitrate: settings.bitrate,
                buffer_seconds: settings.buffer_seconds as f64,
                audio: settings.audio_source.config(
                    settings.system_device.clone(),
                    settings.microphone_device.clone(),
                ),
            };

            let started = ReplaySession::start(config, dir).map_err(|error| error.to_string())?;
            let stats = started.stats();
            if let Ok(mut current) = state.stats.lock() {
                *current = Some(stats);
            }
            *session = Some(started);
        }
    }

    tray::refresh(app);
    Ok(state.status(app))
}

/// Tears down capture, leaving the replaying state immediately.
fn stop_replay_inner(app: &AppHandle, state: &AppState) -> Result<Status, String> {
    let taken = state
        .session
        .lock()
        .map_err(|error| error.to_string())?
        .take();

    // Report "stopped" as soon as the session is gone. Tearing capture down
    // joins OS threads and can take a while (or stall on a wedged GPU), so the
    // UI must leave the replaying state before that finishes.
    if let Ok(mut current) = state.stats.lock() {
        *current = None;
    }

    if let Some(session) = taken {
        // Run the blocking teardown off the caller's critical path.
        std::thread::spawn(move || {
            if let Err(error) = session.stop() {
                eprintln!("failed to stop replay session: {error:#}");
            }
        });
    }

    tray::refresh(app);
    Ok(state.status(app))
}

/// Saves the last `buffer_seconds` of captured video to a new clip.
fn save_clip_inner(app: &AppHandle, state: &AppState) -> Result<ClipSummary, String> {
    // Take the path out from under the lock before the blocking mux.
    let path = {
        let seconds = state
            .settings
            .lock()
            .map(|settings| settings.buffer_seconds)
            .unwrap_or(60);
        let session = state.session.lock().map_err(|error| error.to_string())?;
        let session = session
            .as_ref()
            .ok_or_else(|| "replay is not running".to_string())?;
        session
            .save(seconds as f64)
            .map_err(|error| error.to_string())?
    };

    // A clip now exists on disk, so let any open view reload it — even if
    // summarizing the file below fails.
    let _ = app.emit("clips-changed", ());

    library::summary(&path).map_err(|error| error.to_string())
}

/// Validates and arms new bindings, then persists them.
fn apply_hotkeys(app: &AppHandle, state: &AppState, hotkeys: Hotkeys) -> Result<(), String> {
    let bindings = hotkeys::parse_bindings(&hotkeys)?;
    hook::set_bindings(bindings);
    if let Ok(mut stored) = state.hotkeys.lock() {
        *stored = hotkeys.clone();
    }
    hotkeys::persist(app, &hotkeys)
}

/// Runs the action bound to a global shortcut.
fn run_hotkey_action(app: &AppHandle, state: &AppState, action: HotkeyAction) {
    let outcome = match action {
        HotkeyAction::ToggleReplay => {
            let running = state.session.lock().map(|s| s.is_some()).unwrap_or(false);
            if running {
                stop_replay_inner(app, state)
            } else {
                start_replay_inner(app, state)
            }
        }
        HotkeyAction::SaveClip => save_clip_inner(app, state).map(|_| state.status(app)),
    };
    if let Err(error) = outcome {
        eprintln!("global hotkey action failed: {error}");
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state
        .settings
        .lock()
        .map(|settings| settings.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    settings::persist(&app, &settings)?;
    if let Ok(mut current) = state.settings.lock() {
        *current = settings;
    }
    Ok(())
}

/// Lists the selectable playback (system) and recording (microphone) endpoints.
#[tauri::command]
async fn list_audio_devices() -> AudioDevices {
    AudioDevices {
        system: audio::list_devices(AudioTrack::System).unwrap_or_default(),
        microphone: audio::list_devices(AudioTrack::Microphone).unwrap_or_default(),
    }
}

/// Friendly name of the microphone the test will open; falls back to the id.
fn mic_test_device_name(device_id: Option<&str>) -> String {
    match device_id {
        Some(id) => audio::list_devices(AudioTrack::Microphone)
            .ok()
            .and_then(|devices| devices.into_iter().find(|device| device.id == id))
            .map(|device| device.name)
            .unwrap_or_else(|| id.to_string()),
        None => audio::default_device_info(AudioTrack::Microphone)
            .map(|device| device.name)
            .unwrap_or_else(|_| "System default".to_string()),
    }
}

/// Stops the running microphone test, if any.
fn stop_mic_test_inner(state: &AppState) {
    let taken = state.mic_test.lock().ok().and_then(|mut current| current.take());
    if let Some(test) = taken {
        test.handle.stop();
        if let Some(consumer) = test.consumer {
            let _ = consumer.join();
        }
    }
}

/// Starts (or restarts) a microphone level test on the chosen endpoint.
#[tauri::command]
fn start_mic_test(
    state: State<'_, AppState>,
    device_id: Option<String>,
) -> Result<MicTestStatus, String> {
    stop_mic_test_inner(&state);

    let (sender, receiver) = unbounded::<(AudioTrack, PcmChunk)>();
    let handle = audio::start_capture(
        AudioTrack::Microphone,
        sender,
        Instant::now(),
        device_id.as_deref(),
    )
    .map_err(|error| error.to_string())?;

    let level = Arc::new(AtomicU32::new(0));
    let peak = Arc::new(AtomicU32::new(0));
    let consumer = {
        let level = level.clone();
        let peak = peak.clone();
        std::thread::Builder::new()
            .name("trace-mic-test".into())
            .spawn(move || {
                // Keep draining so the capture thread never blocks on send.
                while let Ok((_, chunk)) = receiver.recv() {
                    let bits = chunk.peak().to_bits();
                    level.fetch_max(bits, Ordering::Relaxed);
                    peak.fetch_max(bits, Ordering::Relaxed);
                }
            })
            .map_err(|error| error.to_string())?
    };

    let device = mic_test_device_name(device_id.as_deref());
    if let Ok(mut current) = state.mic_test.lock() {
        *current = Some(MicTest {
            handle,
            consumer: Some(consumer),
            level,
            peak,
            device: device.clone(),
        });
    }

    Ok(MicTestStatus {
        active: true,
        level: 0.0,
        peak: 0.0,
        device: Some(device),
    })
}

/// Stops the microphone test if one is running.
#[tauri::command]
fn stop_mic_test(state: State<'_, AppState>) {
    stop_mic_test_inner(&state);
}

/// Reports the running test's level, resetting the per-poll peak.
#[tauri::command]
fn mic_test_status(state: State<'_, AppState>) -> MicTestStatus {
    let guard = state.mic_test.lock().ok();
    match guard.as_ref().and_then(|current| current.as_ref()) {
        Some(test) => MicTestStatus {
            active: true,
            level: f32::from_bits(test.level.swap(0, Ordering::Relaxed)),
            peak: f32::from_bits(test.peak.load(Ordering::Relaxed)),
            device: Some(test.device.clone()),
        },
        None => MicTestStatus {
            active: false,
            level: 0.0,
            peak: 0.0,
            device: None,
        },
    }
}

#[tauri::command]
async fn get_status(state: State<'_, AppState>, app: AppHandle) -> Result<Status, String> {
    Ok(state.status(&app))
}

#[tauri::command]
async fn start_replay(state: State<'_, AppState>, app: AppHandle) -> Result<Status, String> {
    start_replay_inner(&app, &state)
}

#[tauri::command]
async fn stop_replay(state: State<'_, AppState>, app: AppHandle) -> Result<Status, String> {
    stop_replay_inner(&app, &state)
}

#[tauri::command]
async fn save_clip(app: AppHandle, state: State<'_, AppState>) -> Result<ClipSummary, String> {
    save_clip_inner(&app, &state)
}

#[tauri::command]
fn get_hotkeys(state: State<'_, AppState>) -> Hotkeys {
    state
        .hotkeys
        .lock()
        .map(|hotkeys| hotkeys.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn set_hotkeys(
    app: AppHandle,
    state: State<'_, AppState>,
    hotkeys: Hotkeys,
) -> Result<(), String> {
    apply_hotkeys(&app, &state, hotkeys)
}

/// Clears (or restores) the armed shortcuts so the UI can capture a new
/// combination without the old binding firing mid-record.
#[tauri::command]
fn set_hotkeys_suspended(state: State<'_, AppState>, suspended: bool) -> Result<(), String> {
    if suspended {
        hook::set_bindings(Vec::new());
        return Ok(());
    }
    let stored = state
        .hotkeys
        .lock()
        .map(|hotkeys| hotkeys.clone())
        .unwrap_or_default();
    hook::set_bindings(hotkeys::parse_bindings(&stored)?);
    Ok(())
}

#[tauri::command]
async fn list_clips(app: AppHandle) -> Result<Vec<ClipSummary>, String> {
    let dir = clips_dir(&app)?;
    library::scan(&dir).map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_clip(app: AppHandle, path: String) -> Result<(), String> {
    // Move to the OS Recycle Bin so a delete can be undone outside the app.
    trash::delete(Path::new(&path)).map_err(|error| error.to_string())?;
    let _ = app.emit("clips-changed", ());
    Ok(())
}

#[tauri::command]
fn open_clip(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn reveal_clip(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // tao registers raw keyboard input devices by default, which stops a
        // low-level keyboard hook from firing while one of our own WebView2
        // windows is focused. `Always` removes that registration (RIDEV_REMOVE),
        // keeping the hook alive when Trace itself has focus. See wry#1761.
        .device_event_filter(tauri::DeviceEventFilter::Always)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            session: Mutex::new(None),
            stats: Mutex::new(None),
            settings: Mutex::new(Settings::default()),
            clips_dir: Mutex::new(None),
            encoders: Mutex::new(None),
            hotkeys: Mutex::new(Hotkeys::default()),
            mic_test: Mutex::new(None),
            tray: Mutex::new(None),
            quitting: AtomicBool::new(false),
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            if let Ok(mut current) = state.settings.lock() {
                *current = settings::load(&handle);
            }

            // Arm global hotkeys. The hook forwards matches to a worker thread so
            // capture/mux work never runs inside the OS hook callback.
            let (actions_tx, actions_rx) = unbounded::<HotkeyAction>();
            match hook::install(actions_tx) {
                Ok(()) => {
                    let worker = handle.clone();
                    let _ = std::thread::Builder::new()
                        .name("trace-hotkey-worker".into())
                        .spawn(move || {
                            while let Ok(action) = actions_rx.recv() {
                                let state = worker.state::<AppState>();
                                run_hotkey_action(&worker, &state, action);
                            }
                        });
                }
                Err(error) => eprintln!("failed to install hotkey hook: {error}"),
            }

            let stored = hotkeys::load(&handle);
            // Keep the raw bindings even if a shortcut is invalid, so the page can
            // still show them and the user can fix them.
            if let Ok(mut current) = state.hotkeys.lock() {
                *current = stored.clone();
            }
            if let Err(error) = apply_hotkeys(&handle, &state, stored) {
                eprintln!("failed to apply stored hotkeys: {error}");
            }
            if let Err(error) = tray::build(&handle) {
                eprintln!("failed to create tray icon: {error}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                // A quit from the tray asked to close for real.
                if state.quitting.load(Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();
                let _ = window.hide();
                // The mic test is a diagnostic, not a recording: stop it silently.
                stop_mic_test_inner(&state);
                // Tell the user once that closing did not quit the app.
                let app = window.app_handle();
                let prefs = tray::load_prefs(app);
                if !prefs.notice_shown {
                    let _ = app
                        .notification()
                        .builder()
                        .title("Trace is still running")
                        .body(
                            "Trace keeps capturing in the background. Right-click the tray icon to quit.",
                        )
                        .show();
                    let _ = tray::persist_prefs(app, &tray::TrayPrefs { notice_shown: true });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            get_status,
            start_replay,
            stop_replay,
            save_clip,
            list_clips,
            delete_clip,
            open_clip,
            reveal_clip,
            get_hotkeys,
            set_hotkeys,
            set_hotkeys_suspended,
            list_audio_devices,
            start_mic_test,
            stop_mic_test,
            mic_test_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
