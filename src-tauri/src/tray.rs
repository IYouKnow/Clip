//! System tray: keeps the app alive in the background when the window closes,
//! and exposes recording controls plus a real quit path.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::AppState;

/// Handles kept so the menu can track recording state.
pub struct TrayHandles {
    #[allow(dead_code)]
    pub icon: TrayIcon,
    /// Label toggles between "Start Replay" and "Stop Replay".
    pub toggle: MenuItem<Wry>,
    /// Enabled only while a replay session is running.
    pub save: MenuItem<Wry>,
}

/// Persisted one-time tray flags (`tray.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrayPrefs {
    #[serde(default)]
    pub notice_shown: bool,
}

/// `<app config dir>/tray.json`.
fn prefs_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|error| error.to_string())?;
    Ok(dir.join("tray.json"))
}

/// Reads stored tray flags, falling back to defaults when absent or unreadable.
pub fn load_prefs(app: &AppHandle) -> TrayPrefs {
    let Ok(path) = prefs_path(app) else {
        return TrayPrefs::default();
    };
    let Ok(contents) = fs::read_to_string(&path) else {
        return TrayPrefs::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

/// Writes tray flags to disk, creating the config directory if needed.
pub fn persist_prefs(app: &AppHandle, prefs: &TrayPrefs) -> Result<(), String> {
    let path = prefs_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(prefs).map_err(|error| error.to_string())?;
    fs::write(&path, json).map_err(|error| error.to_string())
}

/// Shows, unminimizes and focuses the main window.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Hides the main window if it is visible, otherwise restores it.
pub fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            show_window(app);
        }
    }
}

/// Builds the tray icon and menu, wires the handlers, and stores the handles.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Trace", true, None::<&str>)?;
    let separator_top = PredefinedMenuItem::separator(app)?;
    let toggle = MenuItem::with_id(app, "toggle", "Start Replay", true, None::<&str>)?;
    let save = MenuItem::with_id(app, "save", "Save Clip", false, None::<&str>)?;
    let separator_mid = PredefinedMenuItem::separator(app)?;
    let library = MenuItem::with_id(app, "library", "Open Library", true, None::<&str>)?;
    let separator_bottom = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Trace", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &show,
            &separator_top,
            &toggle,
            &save,
            &separator_mid,
            &library,
            &separator_bottom,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("trace-tray")
        .tooltip("Trace")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        })
        .on_menu_event(on_menu_event);
    if let Some(window_icon) = app.default_window_icon().cloned() {
        builder = builder.icon(window_icon);
    }
    let icon = builder.build(app)?;

    let state = app.state::<AppState>();
    if let Ok(mut slot) = state.tray.lock() {
        *slot = Some(TrayHandles { icon, toggle, save });
    }
    Ok(())
}

/// Handles a click on one of the tray menu entries.
fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "show" => show_window(app),
        "library" => {
            show_window(app);
            let _ = app.emit("navigate", "library");
        }
        "toggle" | "save" => {
            // Run off the event loop: start/stop/save can block.
            let app = app.clone();
            let id = event.id().as_ref().to_string();
            std::thread::spawn(move || {
                let state = app.state::<AppState>();
                let running = state.session.lock().map(|s| s.is_some()).unwrap_or(false);
                let outcome = if id == "save" {
                    if running {
                        crate::save_clip_inner(&app, &state).map(|_| ())
                    } else {
                        Ok(())
                    }
                } else if running {
                    crate::stop_replay_inner(&app, &state).map(|_| ())
                } else {
                    crate::start_replay_inner(&app, &state).map(|_| ())
                };
                if let Err(error) = outcome {
                    eprintln!("tray action failed: {error}");
                }
            });
        }
        "quit" => {
            let state = app.state::<AppState>();
            state.quitting.store(true, Ordering::SeqCst);
            // Returns quickly; teardown runs on a detached thread.
            let _ = crate::stop_replay_inner(app, &state);
            crate::stop_mic_test_inner(&state);
            app.exit(0);
        }
        _ => {}
    }
}

/// Keeps the menu in sync with the current recording state.
pub fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    let running = state.session.lock().map(|s| s.is_some()).unwrap_or(false);
    let tray = state.tray.lock();
    if let Ok(guard) = tray {
        if let Some(handles) = guard.as_ref() {
            let _ = handles
                .toggle
                .set_text(if running { "Stop Replay" } else { "Start Replay" });
            let _ = handles.save.set_enabled(running);
        }
    }
}
