//! Names shared between the controls process and the worker. All of them are
//! new for Any2Odyssey so an installed Any2Odd is never signalled by mistake.

/// Single-instance guard for the controls window.
pub const UI_MUTEX: &str = r"Local\Any2OdysseyUI";
/// Single-instance guard for the renderer worker.
pub const RENDERER_MUTEX: &str = r"Local\Any2OdysseyRenderer";
/// Auto-reset events another instance or the installer can signal.
pub const QUIT_EVENT: &str = r"Local\Any2OdysseyQuit";
pub const TOGGLE_EVENT: &str = r"Local\Any2OdysseyToggle";
pub const SHOW_EVENT: &str = r"Local\Any2OdysseyShow";
/// Per-worker stop event: this prefix plus a random hex id. The restore event
/// is the same name with [`RESTORE_SUFFIX`] appended.
pub const STOP_EVENT_PREFIX: &str = r"Local\Any2OdysseyStop-";
pub const RESTORE_SUFFIX: &str = "-restore";

/// Title given to Samsung's renderer window once the worker owns it.
pub const OUTPUT_WINDOW_TITLE: &str = "Any2Odyssey - 3D output";
/// Title Samsung's engine gives its GLFW output window before we rename it.
pub const ENGINE_OUTPUT_TITLE: &str = "Odyssey 3D Hub - 3D Player";
/// Window class of the engine's output window.
pub const ENGINE_OUTPUT_CLASS: &str = "GLFW30";
/// Title of the offscreen capture surface the worker owns.
pub const CAPTURE_SURFACE_TITLE: &str = "Any2Odyssey capture surface";

/// Global hotkeys: Ctrl+Alt+F10 toggles, Ctrl+Alt+F9 shows controls,
/// Ctrl+Alt+F8 restores the overlay.
pub const HOTKEY_TOGGLE_ID: i32 = 1;
pub const HOTKEY_SHOW_ID: i32 = 2;
pub const HOTKEY_RESTORE_ID: i32 = 3;

pub fn is_output_title(title: &str) -> bool {
    title == OUTPUT_WINDOW_TITLE || title == ENGINE_OUTPUT_TITLE
}

/// Build the stop/restore event names for one worker session.
pub fn worker_event_names(session_id: &str) -> (String, String) {
    let stop = format!("{STOP_EVENT_PREFIX}{session_id}");
    let restore = format!("{stop}{RESTORE_SUFFIX}");
    (stop, restore)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_never_collide_with_any2odd() {
        for name in [
            UI_MUTEX,
            RENDERER_MUTEX,
            QUIT_EVENT,
            TOGGLE_EVENT,
            SHOW_EVENT,
            STOP_EVENT_PREFIX,
        ] {
            assert!(
                !name.contains("Odyssey3DCompanion") && !name.contains("OdysseyCompanion"),
                "{name}"
            );
            assert!(name.starts_with(r"Local\"));
        }
        let (stop, restore) = worker_event_names("abc");
        assert_eq!(stop, r"Local\Any2OdysseyStop-abc");
        assert_eq!(restore, r"Local\Any2OdysseyStop-abc-restore");
        assert!(
            is_output_title(ENGINE_OUTPUT_TITLE)
                && is_output_title(OUTPUT_WINDOW_TITLE)
                && !is_output_title("Notepad")
        );
    }
}
