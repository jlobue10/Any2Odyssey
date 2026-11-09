//! Non-Windows build: the controls run, but 3D conversion needs Samsung's
//! Windows engine. Linux portable features arrive with the Linux milestone.

use any2odyssey_core::prefs::Preferences;
use any2odyssey_core::update::Comparison;
use std::path::Path;

pub struct ControlEvents {
    pub quit: NoEvent,
    pub show: NoEvent,
    pub toggle: NoEvent,
}
pub struct NoEvent;
impl NoEvent {
    pub fn is_signaled(&self) -> bool {
        false
    }
}
impl ControlEvents {
    pub fn create() -> Result<ControlEvents, String> {
        Ok(ControlEvents {
            quit: NoEvent,
            show: NoEvent,
            toggle: NoEvent,
        })
    }
}

pub fn run_worker(_: &str, _: u32, _: isize, _: Preferences) -> i32 {
    eprintln!("The conversion worker runs only on Windows.");
    1
}
pub fn inspect(_: u32) -> i32 {
    1
}
pub fn layout_probe(_: u32, _: u32) -> i32 {
    1
}
pub fn test_capture_exclusion() -> i32 {
    1
}
pub fn test_offscreen() -> i32 {
    1
}
pub fn signal(_: &str) -> bool {
    false
}
pub struct SingleInstance;
pub fn single_instance() -> Option<SingleInstance> {
    Some(SingleInstance)
}
pub fn validate() -> Result<(), String> {
    Err("3D conversion needs Samsung's Odyssey 3D Hub engine, which exists only on Windows.".into())
}
pub fn list_windows(_: Option<u32>) -> Vec<(isize, String)> {
    Vec::new()
}
pub struct Worker;
impl Worker {
    pub fn spawn(_: isize, _: Preferences) -> Result<Worker, String> {
        Err("3D conversion is only available on Windows.".into())
    }
    #[cfg(test)]
    pub fn fake() -> Worker {
        Worker
    }
    pub fn pid(&self) -> u32 {
        0
    }
    pub fn has_exited(&mut self) -> Option<i32> {
        Some(1)
    }
    pub fn restore(&self) {}
    pub fn stop(self) -> Option<String> {
        None
    }
}
pub fn show_window(_: isize) {}
pub fn hide_window(_: isize) {}
pub fn open_path(path: &Path) {
    let _ = open::that_detached(path);
}
pub fn install_jump_list() {}
pub fn check_updates() -> Result<Comparison, String> {
    Err("Checking for updates is available on Windows only.".into())
}
