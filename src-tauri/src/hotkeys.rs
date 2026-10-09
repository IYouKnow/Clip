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

/// Which of the four modifier keys are held with a binding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

/// A parsed shortcut: its modifiers, the Windows virtual-key code it fires on, and
/// the action it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub mods: Mods,
    pub vk: u32,
    pub action: HotkeyAction,
}

/// Parses every configured accelerator into a registrable binding, rejecting
/// invalid strings and duplicate combinations.
pub fn parse_bindings(hotkeys: &Hotkeys) -> Result<Vec<Binding>, String> {
    let mut bindings: Vec<Binding> = Vec::new();
    for (action, accelerator) in hotkeys.entries() {
        let Some(accelerator) = accelerator.as_deref().map(str::trim) else {
            continue;
        };
        if accelerator.is_empty() {
            continue;
        }
        let binding = parse_accelerator(accelerator, action)?;
        if bindings
            .iter()
            .any(|existing| existing.mods == binding.mods && existing.vk == binding.vk)
        {
            return Err(format!("\"{accelerator}\" is assigned to more than one action"));
        }
        bindings.push(binding);
    }
    Ok(bindings)
}

/// Parses an accelerator such as `Ctrl+Shift+KeyR` or a bare `F9`.
///
/// Modifier tokens come first; the single remaining token is a DOM
/// `KeyboardEvent.code` (e.g. `KeyR`, `Digit1`, `F9`).
fn parse_accelerator(accelerator: &str, action: HotkeyAction) -> Result<Binding, String> {
    let invalid = || format!("\"{accelerator}\" is not a valid shortcut");
    let mut mods = Mods::default();
    let mut key: Option<u32> = None;

    for token in accelerator.split('+') {
        let token = token.trim();
        if token.is_empty() {
            return Err(invalid());
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "shift" => mods.shift = true,
            "alt" | "option" => mods.alt = true,
            "super" | "win" | "meta" | "cmd" | "command" => mods.win = true,
            _ => {
                if key.is_some() {
                    return Err(invalid());
                }
                key = Some(code_to_vk(token).ok_or_else(invalid)?);
            }
        }
    }

    Ok(Binding {
        mods,
        vk: key.ok_or_else(invalid)?,
        action,
    })
}

/// Maps a DOM `KeyboardEvent.code` onto a Windows virtual-key code.
fn code_to_vk(code: &str) -> Option<u32> {
    if let Some(letter) = code.strip_prefix("Key") {
        let mut chars = letter.chars();
        if let Some(c) = chars.next() {
            if chars.next().is_none() && c.is_ascii_uppercase() {
                return Some(c as u32);
            }
        }
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        if let Some(c) = digit.chars().next() {
            if c.is_ascii_digit() {
                return Some(c as u32);
            }
        }
    }
    if let Some(number) = code.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&number) {
            return Some(0x70 + number - 1);
        }
    }
    if let Some(digit) = code.strip_prefix("Numpad") {
        if let Some(c) = digit.chars().next() {
            if c.is_ascii_digit() {
                return Some(0x60 + (c as u32 - '0' as u32));
            }
        }
    }

    Some(match code {
        "Backspace" => 0x08,
        "Tab" => 0x09,
        "Enter" | "NumpadEnter" => 0x0D,
        "Pause" => 0x13,
        "CapsLock" => 0x14,
        "Escape" => 0x1B,
        "Space" => 0x20,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "End" => 0x23,
        "Home" => 0x24,
        "ArrowLeft" => 0x25,
        "ArrowUp" => 0x26,
        "ArrowRight" => 0x27,
        "ArrowDown" => 0x28,
        "PrintScreen" => 0x2C,
        "Insert" => 0x2D,
        "Delete" => 0x2E,
        "NumLock" => 0x90,
        "ScrollLock" => 0x91,
        "NumpadMultiply" => 0x6A,
        "NumpadAdd" => 0x6B,
        "NumpadSubtract" => 0x6D,
        "NumpadDecimal" => 0x6E,
        "NumpadDivide" => 0x6F,
        "Semicolon" => 0xBA,
        "Equal" => 0xBB,
        "Comma" => 0xBC,
        "Minus" => 0xBD,
        "Period" => 0xBE,
        "Slash" => 0xBF,
        "Backquote" => 0xC0,
        "BracketLeft" => 0xDB,
        "Backslash" => 0xDC,
        "BracketRight" => 0xDD,
        "Quote" => 0xDE,
        "AudioVolumeMute" => 0xAD,
        "AudioVolumeDown" => 0xAE,
        "AudioVolumeUp" => 0xAF,
        "MediaTrackNext" => 0xB0,
        "MediaTrackPrevious" => 0xB1,
        "MediaStop" => 0xB2,
        "MediaPlayPause" => 0xB3,
        "MediaPlay" => 0xFA,
        _ => return None,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_key() {
        let binding = parse_accelerator("Ctrl+Shift+KeyR", HotkeyAction::ToggleReplay).unwrap();
        assert_eq!(
            binding.mods,
            Mods { ctrl: true, shift: true, alt: false, win: false }
        );
        assert_eq!(binding.vk, 0x52);
    }

    #[test]
    fn parses_bare_key() {
        let binding = parse_accelerator("F9", HotkeyAction::SaveClip).unwrap();
        assert_eq!(binding.mods, Mods::default());
        assert_eq!(binding.vk, 0x78);
    }

    #[test]
    fn parses_win_modifier() {
        let binding = parse_accelerator("Super+Space", HotkeyAction::SaveClip).unwrap();
        assert!(binding.mods.win);
        assert_eq!(binding.vk, 0x20);
    }

    #[test]
    fn rejects_modifier_only_and_unknown_keys() {
        assert!(parse_accelerator("Ctrl+Shift", HotkeyAction::SaveClip).is_err());
        assert!(parse_accelerator("Nope", HotkeyAction::SaveClip).is_err());
    }

    #[test]
    fn rejects_duplicate_bindings() {
        let hotkeys = Hotkeys {
            toggle_replay: Some("F9".into()),
            save_clip: Some("F9".into()),
        };
        assert!(parse_bindings(&hotkeys).is_err());
    }
}
