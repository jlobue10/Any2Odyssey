//! Global shortcuts: Ctrl+Alt+F10 toggle, Ctrl+Alt+F9 controls, Ctrl+Alt+F8 restore.

use any2odyssey_core::ipc::{HOTKEY_RESTORE_ID, HOTKEY_SHOW_ID, HOTKEY_TOGGLE_ID};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};

/// `MOD_ALT | MOD_CONTROL | MOD_NOREPEAT`.
const MODS: HOT_KEY_MODIFIERS = HOT_KEY_MODIFIERS(0x0001 | 0x0002 | 0x4000);
const VK_F8: u32 = 0x77;
const VK_F9: u32 = 0x78;
const VK_F10: u32 = 0x79;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Registration {
    pub toggle: bool,
    pub show: bool,
    pub restore: bool,
}

impl Registration {
    pub fn all(&self) -> bool {
        self.toggle && self.show && self.restore
    }
}

/// Register the three shortcuts for `hwnd`. Failures are reported, not fatal.
pub fn register(hwnd: HWND) -> Registration {
    let reg = |id, vk| unsafe { RegisterHotKey(Some(hwnd), id, MODS, vk) }.is_ok();
    Registration {
        toggle: reg(HOTKEY_TOGGLE_ID, VK_F10),
        show: reg(HOTKEY_SHOW_ID, VK_F9),
        restore: reg(HOTKEY_RESTORE_ID, VK_F8),
    }
}

pub fn unregister(hwnd: HWND) {
    for id in [HOTKEY_TOGGLE_ID, HOTKEY_SHOW_ID, HOTKEY_RESTORE_ID] {
        let _ = unsafe { UnregisterHotKey(Some(hwnd), id) };
    }
}

/// Message id delivered for a registered hotkey.
pub const WM_HOTKEY: u32 = 0x0312;
