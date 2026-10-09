//! Global hotkey bindings: the actions they trigger and on-disk persistence.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// An action a global hotkey can trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    ToggleReplay,
    SaveClip,
}

/// User-configured accelerators, stored as strings like `Ctrl+Shift+R`.
/// `None` means the action has no binding.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hotkeys {
    #[serde(default)]
    pub toggle_replay: Option<String>,
    #[serde(default)]
    pub save_clip: Option<String>,
}

impl Hotkeys {
    /// Every action paired with its configured accelerator.
    pub fn entries(&self) -> [(HotkeyAction, &Option<String>); 2] {
        [
            (HotkeyAction::ToggleReplay, &self.toggle_replay),
            (HotkeyAction::SaveClip, &self.save_clip),
        ]
    }
}

/// `<app config dir>/hotkeys.json`.
fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|error| error.to_string())?;
    Ok(dir.join("hotkeys.json"))
}

/// Reads stored bindings, falling back to defaults when absent or unreadable.
pub fn load(app: &AppHandle) -> Hotkeys {
    let Ok(path) = config_path(app) else {
        return Hotkeys::default();
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return Hotkeys::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

/// Writes bindings to disk, creating the config directory if needed.
pub fn persist(app: &AppHandle, hotkeys: &Hotkeys) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(hotkeys).map_err(|error| error.to_string())?;
    fs::write(&path, json).map_err(|error| error.to_string())
}
