//! Tauri shell: exposes the replay engine and clip library to the UI.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use trace_engine::encode;
use trace_engine::session::{ReplayConfig, ReplaySession, SessionStats};
use trace_library::{self as library, ClipSummary};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tauri_plugin_opener::OpenerExt;

mod hotkeys;
use hotkeys::{HotkeyAction, Hotkeys};

/// Folder name under the user's Videos directory.
const CLIPS_FOLDER: &str = "Trace";

/// User-configurable options.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    /// Seconds of video kept in the replay buffer.
    buffer_seconds: u32,
    fps: u32,
    bitrate: u64,
    /// Preferred encoder, or `None` to auto-select.
    encoder: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            buffer_seconds: 60,
            fps: 60,
            bitrate: 20_000_000,
            encoder: None,
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
    buffer_seconds: u32,
    fps: u32,
    bitrate: u64,
    clips_dir: String,
    available_encoders: Vec<String>,
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
    /// Currently registered accelerators and the action each one fires.
    bindings: Mutex<Vec<(Shortcut, HotkeyAction)>>,
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
            };

            let started = ReplaySession::start(config, dir).map_err(|error| error.to_string())?;
            let stats = started.stats();
            if let Ok(mut current) = state.stats.lock() {
                *current = Some(stats);
            }
            *session = Some(started);
        }
    }

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

    Ok(state.status(app))
}

/// Saves the last `buffer_seconds` of captured video to a new clip.
fn save_clip_inner(state: &AppState) -> Result<ClipSummary, String> {
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

    library::summary(&path).map_err(|error| error.to_string())
}

/// Parses the configured accelerators into registrable shortcuts, rejecting
/// invalid strings and duplicates.
fn parse_bindings(hotkeys: &Hotkeys) -> Result<Vec<(Shortcut, HotkeyAction)>, String> {
    let mut bindings: Vec<(Shortcut, HotkeyAction)> = Vec::new();
    for (action, accelerator) in hotkeys.entries() {
        let Some(accelerator) = accelerator.as_deref().map(str::trim) else {
            continue;
        };
        if accelerator.is_empty() {
            continue;
        }
        let shortcut: Shortcut = accelerator
            .parse()
            .map_err(|_| format!("\"{accelerator}\" is not a valid shortcut"))?;
        if bindings
            .iter()
            .any(|(existing, _)| existing.matches(shortcut.mods, shortcut.key))
        {
            return Err(format!("\"{accelerator}\" is assigned to more than one action"));
        }
        bindings.push((shortcut, action));
    }
    Ok(bindings)
}

/// Unregisters everything and registers exactly `bindings`.
fn register_bindings(
    app: &AppHandle,
    bindings: &[(Shortcut, HotkeyAction)],
) -> Result<(), String> {
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    for (shortcut, _) in bindings {
        manager
            .register(*shortcut)
            .map_err(|error| format!("could not register {shortcut}: {error}"))?;
    }
    Ok(())
}

/// Validates, registers (rolling back on failure) and persists new bindings.
fn apply_hotkeys(app: &AppHandle, state: &AppState, hotkeys: Hotkeys) -> Result<(), String> {
    let next = parse_bindings(&hotkeys)?;
    let previous = state.bindings.lock().map(|b| b.clone()).unwrap_or_default();

    if let Err(error) = register_bindings(app, &next) {
        // Put the working set back so a bad binding can't leave us with none.
        let _ = register_bindings(app, &previous);
        return Err(error);
    }

    if let Ok(mut stored) = state.bindings.lock() {
        *stored = next;
    }
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
        HotkeyAction::SaveClip => save_clip_inner(state).map(|_| state.status(app)),
    };
    if let Err(error) = outcome {
        eprintln!("global hotkey action failed: {error}");
    }
}

/// Dispatches a global shortcut event to the matching action on a worker thread
/// so the mux/teardown work never blocks the event loop.
fn on_global_shortcut(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed {
        return;
    }
    let app = app.clone();
    let shortcut = *shortcut;
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let action = state.bindings.lock().ok().and_then(|bindings| {
            bindings
                .iter()
                .find(|(candidate, _)| candidate.matches(shortcut.mods, shortcut.key))
                .map(|(_, action)| *action)
        });
        if let Some(action) = action {
            run_hotkey_action(&app, &state, action);
        }
    });
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
fn set_settings(state: State<'_, AppState>, settings: Settings) {
    if let Ok(mut current) = state.settings.lock() {
        *current = settings;
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
async fn save_clip(state: State<'_, AppState>) -> Result<ClipSummary, String> {
    save_clip_inner(&state)
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

/// Clears (or restores) the registered shortcuts so the UI can capture a new
/// combination without the old binding firing mid-record.
#[tauri::command]
fn set_hotkeys_suspended(
    app: AppHandle,
    state: State<'_, AppState>,
    suspended: bool,
) -> Result<(), String> {
    if suspended {
        return register_bindings(&app, &[]);
    }
    let stored = state
        .hotkeys
        .lock()
        .map(|hotkeys| hotkeys.clone())
        .unwrap_or_default();
    let bindings = parse_bindings(&stored)?;
    register_bindings(&app, &bindings)
}

#[tauri::command]
async fn list_clips(app: AppHandle) -> Result<Vec<ClipSummary>, String> {
    let dir = clips_dir(&app)?;
    library::scan(&dir).map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_clip(path: String) -> Result<(), String> {
    library::delete(Path::new(&path)).map_err(|error| error.to_string())
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
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(on_global_shortcut)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            session: Mutex::new(None),
            stats: Mutex::new(None),
            settings: Mutex::new(Settings::default()),
            clips_dir: Mutex::new(None),
            encoders: Mutex::new(None),
            hotkeys: Mutex::new(Hotkeys::default()),
            bindings: Mutex::new(Vec::new()),
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            let stored = hotkeys::load(&handle);
            // Keep the raw bindings even if registration fails, so the page can
            // still show them and the user can fix the conflict.
            if let Ok(mut current) = state.hotkeys.lock() {
                *current = stored.clone();
            }
            if let Err(error) = apply_hotkeys(&handle, &state, stored) {
                eprintln!("failed to register stored hotkeys: {error}");
            }
            Ok(())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
