//! Persisted user settings (`settings.json` in the app config directory).

use std::fs;
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::Settings;

/// `<app config dir>/settings.json`.
fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|error| error.to_string())?;
    Ok(dir.join("settings.json"))
}

/// Reads stored settings, falling back to defaults when absent or unreadable.
pub fn load(app: &AppHandle) -> Settings {
    let Ok(path) = config_path(app) else {
        return Settings::default();
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return Settings::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

/// Writes settings to disk, creating the config directory if needed.
pub fn persist(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(&path, json).map_err(|error| error.to_string())
}
