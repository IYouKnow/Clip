//! Tauri shell: exposes the replay engine and clip library to the UI.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clipper23_engine::encode;
use clipper23_engine::session::{ReplayConfig, ReplaySession, SessionStats};
use clipper23_library::{self as library, ClipSummary};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

/// Folder name under the user's Videos directory.
const CLIPS_FOLDER: &str = "Clipper23";

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
    {
        let mut session = state.session.lock().map_err(|error| error.to_string())?;
        if session.is_none() {
            let settings = state.settings.lock().map(|s| s.clone()).unwrap_or_default();
            let dir = {
                let mut cache = state.clips_dir.lock().map_err(|e| e.to_string())?;
                match cache.as_ref() {
                    Some(dir) => dir.clone(),
                    None => {
                        let dir = clips_dir(&app)?;
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

    Ok(state.status(&app))
}

#[tauri::command]
async fn stop_replay(state: State<'_, AppState>, app: AppHandle) -> Result<Status, String> {
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
        // Run the blocking teardown off the async runtime and off the command's
        // critical path, so a slow stop can never freeze status polling or the
        // stop button's loading state.
        std::thread::spawn(move || {
            if let Err(error) = session.stop() {
                eprintln!("failed to stop replay session: {error:#}");
            }
        });
    }

    Ok(state.status(&app))
}

#[tauri::command]
async fn save_clip(state: State<'_, AppState>) -> Result<ClipSummary, String> {
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            session: Mutex::new(None),
            stats: Mutex::new(None),
            settings: Mutex::new(Settings::default()),
            clips_dir: Mutex::new(None),
            encoders: Mutex::new(None),
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
