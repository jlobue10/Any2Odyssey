//! Global shortcuts through the `global-hotkey` crate (RegisterHotKey on
//! Windows). Ctrl+Alt+F10 toggles, Ctrl+Alt+F9 shows the controls,
//! Ctrl+Alt+F8 restores the overlay.

use std::sync::OnceLock;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::core::{Core, UiRequest};

#[derive(Clone, Copy)]
struct Registered {
    toggle: u32,
    show: u32,
    restore: u32,
}

static REGISTERED: OnceLock<Option<Registered>> = OnceLock::new();

/// Register all three shortcuts. The returned manager owns the hidden
/// message window and must be kept alive by the GUI thread. `None` when any
/// shortcut is already taken by another application (nothing stays registered).
pub fn register() -> Option<GlobalHotKeyManager> {
    let manager = GlobalHotKeyManager::new().ok()?;
    let mods = Some(Modifiers::CONTROL | Modifiers::ALT);
    let toggle = HotKey::new(mods, Code::F10);
    let show = HotKey::new(mods, Code::F9);
    let restore = HotKey::new(mods, Code::F8);
    let all = [toggle, show, restore];
    let ok = manager.register_all(&all).is_ok();
    if !ok {
        let _ = manager.unregister_all(&all);
        let _ = REGISTERED.set(None);
        return None;
    }
    let _ = REGISTERED.set(Some(Registered {
        toggle: toggle.id(),
        show: show.id(),
        restore: restore.id(),
    }));
    Some(manager)
}

/// Deliver pending hotkey presses to the core (any thread).
pub fn drain(core: &mut Core) {
    let Some(Some(reg)) = REGISTERED.get() else {
        return;
    };
    while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
        if event.state != HotKeyState::Pressed {
            continue;
        }
        if event.id == reg.toggle {
            core.toggle();
        } else if event.id == reg.show {
            core.request(UiRequest::ShowControls);
        } else if event.id == reg.restore {
            core.restore_overlay();
        }
    }
}
