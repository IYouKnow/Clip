//! Global hotkeys via a Windows low-level keyboard hook.
//!
//! `tauri-plugin-global-shortcut` uses `RegisterHotKey`, which Windows stops
//! delivering while a full-screen/exclusive game or an elevated app has focus.
//! A `WH_KEYBOARD_LL` hook observes presses system-wide regardless of focus, so
//! it keeps working in exactly the situations a clipping tool needs.
//!
//! The hook runs on its own thread with its own message pump. Matches are handed
//! to the app over a channel, so the callback never blocks on capture work
//! (Windows silently drops hooks that take too long).

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use crossbeam_channel::Sender;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::hotkeys::{Binding, HotkeyAction, Mods};

/// State shared with the hook callback, which is a plain `extern "system"` fn and
/// therefore cannot capture anything.
struct Shared {
    /// The shortcuts currently armed to fire.
    bindings: Mutex<Vec<Binding>>,
    /// Virtual-key codes currently held down, so auto-repeat fires only once.
    pressed: Mutex<HashSet<u32>>,
    /// Where fired actions are delivered.
    actions: Sender<HotkeyAction>,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

/// Installs the hook and starts its message loop on a dedicated thread.
pub fn install(actions: Sender<HotkeyAction>) -> Result<(), String> {
    let shared = Arc::new(Shared {
        bindings: Mutex::new(Vec::new()),
        pressed: Mutex::new(HashSet::new()),
        actions,
    });
    SHARED
        .set(shared)
        .map_err(|_| "hotkey hook already installed".to_string())?;

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("trace-hotkey-hook".into())
        .spawn(move || run(ready_tx))
        .map_err(|error| error.to_string())?;

    ready_rx.recv().map_err(|error| error.to_string())?
}

/// Replaces the set of shortcuts the hook fires on.
pub fn set_bindings(bindings: Vec<Binding>) {
    if let Some(shared) = SHARED.get() {
        if let Ok(mut live) = shared.bindings.lock() {
            *live = bindings;
        }
    }
}

/// Installs the hook, then pumps messages until the thread is asked to quit.
fn run(ready: std::sync::mpsc::Sender<Result<(), String>>) {
    unsafe {
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
            Ok(hook) => hook,
            Err(error) => {
                let _ = ready.send(Err(error.to_string()));
                return;
            }
        };
        let _ = ready.send(Ok(()));

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        let _ = UnhookWindowsHookEx(hook);
    }
}

/// The low-level hook callback. Keys always pass through to the focused app.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        if let Some(shared) = SHARED.get() {
            match wparam.0 as u32 {
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    let first_press = shared
                        .pressed
                        .lock()
                        .map(|mut down| down.insert(event.vkCode))
                        .unwrap_or(false);
                    if first_press {
                        if let Some(action) = shared.match_binding(event.vkCode) {
                            let _ = shared.actions.send(action);
                        }
                    }
                }
                WM_KEYUP | WM_SYSKEYUP => {
                    if let Ok(mut down) = shared.pressed.lock() {
                        down.remove(&event.vkCode);
                    }
                }
                _ => {}
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

impl Shared {
    /// Finds the action bound to `vk` with exactly the modifiers now held.
    fn match_binding(&self, vk: u32) -> Option<HotkeyAction> {
        let mods = current_mods();
        self.bindings
            .lock()
            .ok()?
            .iter()
            .find(|binding| binding.mods == mods && binding.vk == vk)
            .map(|binding| binding.action)
    }
}

/// Reads the current state of the four modifier keys.
fn current_mods() -> Mods {
    let down = |key: i32| unsafe { GetAsyncKeyState(key) < 0 };
    Mods {
        ctrl: down(VK_CONTROL.0 as i32),
        shift: down(VK_SHIFT.0 as i32),
        alt: down(VK_MENU.0 as i32),
        win: down(VK_LWIN.0 as i32) || down(VK_RWIN.0 as i32),
    }
}
