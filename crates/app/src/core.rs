//! Shared application state and the supervisor loop. The GUI thread renders
//! from it; a poller thread drives worker supervision, named events, tray
//! menu and hotkey events every 250 ms, so everything keeps working while the
//! controls window is hidden in the tray.

use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use any2odyssey_core::log::Logger;
use any2odyssey_core::paths;
use any2odyssey_core::prefs::{CaptureMode, Preferences, UiSettings};
use any2odyssey_core::status::WorkerStatus;
use any2odyssey_core::supervisor::{Action, Supervisor};

use crate::platform::{self, ControlEvents, Worker};

pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenshotRequest {
    pub path: std::path::PathBuf,
    pub dark: bool,
    pub help_page: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowChoice {
    pub handle: isize,
    pub title: String,
}

/// Commands the poller or menus want the GUI thread to execute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiRequest {
    ShowControls,
    ShowHelp,
    ShowAbout,
    ToggleTheme,
    MinimizeToTray,
    Quit,
}

/// Menu commands shared by the tray menu and the right-click menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    ShowControls,
    Toggle,
    RestoreOverlay,
    KeepOverlayOnTop(bool),
    AlwaysConvertDesktop,
    SideBySide,
    AiMode,
    Restart,
    Help,
    About,
    ToggleTheme,
    ViewLogs,
    MinimizeToTray,
    Quit,
}

enum WorkerResult {
    Started {
        generation: u64,
        result: Result<Worker, String>,
    },
    Stopped(Option<String>),
}

/// State changed by the poller that the GUI actually displays. Keeping one
/// small snapshot avoids scheduling a full redraw for an unchanged poll.
struct UiState {
    status: String,
    prefs: Preferences,
    wanted: bool,
    exiting: bool,
}

impl UiState {
    fn capture(core: &Core) -> Self {
        Self {
            status: core.status.clone(),
            prefs: core.prefs.clone(),
            wanted: core.wanted(),
            exiting: core.exiting,
        }
    }

    fn needs_repaint(&self, core: &Core) -> bool {
        !core.requests.is_empty()
            || self.status != core.status
            || self.prefs != core.prefs
            || self.wanted != core.wanted()
            || self.exiting != core.exiting
    }
}

pub struct Core {
    pub prefs: Preferences,
    pub ui: UiSettings,
    pub status: String,
    settings_load_error: Option<String>,
    pub supervisor: Supervisor,
    pub worker: Option<Worker>,
    operation: Option<mpsc::Receiver<WorkerResult>>,
    generation: u64,
    pub windows: Vec<WindowChoice>,
    pub selected_window: Option<isize>,
    pub hotkeys_ok: bool,
    pub start_requested: bool,
    /// Isolated preview (screenshots, UI checks): never starts a worker or writes preferences.
    pub preview: bool,
    /// `--screenshot FILE [--dark] [--help-page]`: render, save, exit.
    pub screenshot: Option<ScreenshotRequest>,
    pub exiting: bool,
    pub requests: Vec<UiRequest>,
    pub logger: Logger,
    events: Option<ControlEvents>,
    pub controls_hwnd: isize,
}

impl Core {
    pub fn new(logger: Logger, events: Option<ControlEvents>, start_requested: bool) -> Core {
        let (prefs, settings_load_error) = match load_preferences(&logger) {
            Ok(prefs) => (prefs, None),
            Err(error) => {
                logger.log(&error);
                (
                    Preferences {
                        auto_convert: false,
                        ..Default::default()
                    },
                    Some(error),
                )
            }
        };
        let ui = UiSettings::load();
        let mut core = Core {
            supervisor: Supervisor::new(prefs.recover),
            prefs,
            ui,
            status: settings_load_error
                .as_deref()
                .map(settings_recovery_message)
                .unwrap_or_else(|| "Ready".into()),
            settings_load_error,
            worker: None,
            operation: None,
            generation: 0,
            windows: Vec::new(),
            selected_window: None,
            hotkeys_ok: false,
            start_requested,
            preview: false,
            screenshot: None,
            exiting: false,
            requests: Vec::new(),
            logger,
            events,
            controls_hwnd: 0,
        };
        core.refresh_windows();
        core
    }

    pub fn log(&self, message: &str) {
        self.logger.log(message);
    }

    pub fn save_preferences(&mut self) -> bool {
        self.prefs.recover = self.supervisor.recover;
        if self.preview {
            return true;
        }
        if let Some(error) = &self.settings_load_error {
            self.status = settings_recovery_message(error);
            return false;
        }
        if let Err(e) = self.prefs.save() {
            self.status = format!("Could not save preferences: {e}");
            self.log(&self.status);
            return false;
        }
        true
    }

    pub fn save_ui(&self) {
        if self.preview {
            return;
        }
        if let Err(e) = self.ui.save() {
            self.log(&format!("Could not save desktop theme: {e}"));
        }
    }

    pub fn wanted(&self) -> bool {
        self.supervisor.wanted
    }

    pub fn worker_running(&mut self) -> bool {
        match self.worker.as_mut() {
            Some(w) => w.has_exited().is_none(),
            None => false,
        }
    }

    pub fn refresh_windows(&mut self) {
        let worker_pid = self.worker.as_ref().map(Worker::pid);
        let windows = platform::list_windows(worker_pid)
            .into_iter()
            .map(|(handle, title)| WindowChoice { handle, title })
            .collect();
        self.set_windows(windows);
    }

    fn set_windows(&mut self, windows: Vec<WindowChoice>) {
        self.windows = windows;
        // A refreshed list is not consent to capture its first entry. Require
        // a choice at launch, and clear a choice whose window has disappeared.
        if self
            .selected_window
            .is_some_and(|selected| !self.windows.iter().any(|w| w.handle == selected))
        {
            self.selected_window = None;
        }
    }

    pub fn start_conversion(&mut self) {
        if !self.save_preferences() {
            return;
        }
        self.supervisor.start_requested();
        self.start_worker();
    }

    pub fn stop_conversion(&mut self) {
        self.supervisor.stop_requested();
        self.stop_worker();
        self.status = if self.operation.is_some() {
            "Stopping 3D…"
        } else {
            "3D is off. Your normal desktop is restored."
        }
        .into();
    }

    pub fn toggle(&mut self) {
        if self.wanted() {
            self.stop_conversion();
        } else {
            self.start_conversion();
        }
    }

    /// Apply settings / restart conversion.
    pub fn apply(&mut self) {
        if self.settings_load_error.is_some() && !self.preview {
            // Apply is the explicit choice to replace unreadable settings.
            // Startup, hide, quit and ordinary saves leave the original alone.
            self.prefs.recover = self.supervisor.recover;
            match self.prefs.save_recovering_to(&paths::settings_file()) {
                Ok(backup) => {
                    if let Some(backup) = backup {
                        self.log(&format!(
                            "Previous settings preserved at {}",
                            backup.display()
                        ));
                    }
                    self.settings_load_error = None;
                }
                Err(error) => {
                    self.status =
                        format!("Could not recover preferences; original left unchanged: {error}");
                    self.log(&self.status);
                    return;
                }
            }
        } else if !self.save_preferences() {
            return;
        }
        if self.wanted() {
            self.stop_worker();
            self.start_worker();
        } else {
            self.status = "Preferences saved.".into();
        }
    }

    pub fn restart(&mut self) {
        if !self.save_preferences() {
            return;
        }
        self.stop_worker();
        self.supervisor.start_requested();
        self.start_worker();
    }

    fn start_worker(&mut self) {
        if self.preview {
            self.status = "Preview mode: conversion is not started.".into();
            return;
        }
        if self.exiting || !self.wanted() || self.operation.is_some() || self.worker_running() {
            return;
        }
        if !self.save_preferences() {
            self.supervisor.start_failed();
            return;
        }
        let preferences = self.prefs.clone();
        let source = self.selected_window.unwrap_or(0);
        if self.prefs.mode == CaptureMode::Window && source == 0 {
            self.status = "Choose a source window first.".into();
            self.supervisor.start_failed();
            return;
        }
        let generation = self.generation;
        let (tx, rx) = mpsc::channel();
        self.operation = Some(rx);
        self.status = "Starting 3D — checking prerequisites and loading the AI model.".into();
        std::thread::spawn(move || {
            // Hashing, hardware discovery and process launch must not hold the
            // GUI's shared state lock. Cancellation is checked on completion.
            let result = platform::validate().and_then(|()| Worker::spawn(source, preferences));
            if let Err(mpsc::SendError(WorkerResult::Started {
                result: Ok(worker), ..
            })) = tx.send(WorkerResult::Started { generation, result })
            {
                // A closed receiver must never leave a newly spawned worker orphaned.
                worker.stop();
            }
        });
    }

    fn stop_worker(&mut self) {
        // Invalidate a start already in flight, including an Apply that changes
        // settings while prerequisite checks are still running.
        self.generation = self.generation.wrapping_add(1);
        if let Some(worker) = self.worker.take() {
            let (tx, rx) = mpsc::channel();
            self.operation = Some(rx);
            self.status = "Stopping 3D…".into();
            std::thread::spawn(move || {
                let _ = tx.send(WorkerResult::Stopped(worker.stop()));
            });
        }
    }

    fn poll_operation(&mut self) {
        let result = match self.operation.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.operation = None;
                self.supervisor.start_failed();
                self.status = "Worker control task stopped unexpectedly. See logs.".into();
                self.log(&self.status);
                return;
            }
            _ => return,
        };
        self.operation = None;
        match result {
            WorkerResult::Started { generation, result } => match result {
                Ok(worker) => {
                    self.log(&format!("Started renderer PID {}", worker.pid()));
                    self.worker = Some(worker);
                    if generation != self.generation || !self.wanted() || self.exiting {
                        self.stop_worker();
                    }
                }
                Err(e) if generation == self.generation && self.wanted() && !self.exiting => {
                    self.supervisor.start_failed();
                    self.status = format!("Could not start conversion: {e}");
                    self.log(&self.status);
                }
                Err(_) => {} // A cancelled attempt must not override newer intent.
            },
            WorkerResult::Stopped(message) => {
                if let Some(message) = message {
                    self.log(&message);
                }
                if !self.wanted() {
                    self.status = "3D is off. Your normal desktop is restored.".into();
                }
            }
        }
    }

    pub fn ready_to_exit(&self) -> bool {
        self.exiting && self.operation.is_none() && self.worker.is_none()
    }

    /// Emergency cleanup when the native window closes without the normal
    /// deferred Quit flow. The caller performs these waits outside Core's lock.
    pub fn take_shutdown(&mut self) -> impl FnOnce() {
        self.exiting = true;
        self.supervisor.stop_requested();
        let worker = self.worker.take();
        let operation = self.operation.take();
        move || {
            if let Some(worker) = worker {
                worker.stop();
            }
            if let Some(rx) = operation {
                if let Ok(WorkerResult::Started {
                    result: Ok(worker), ..
                }) = rx.recv()
                {
                    worker.stop();
                }
            }
        }
    }

    pub fn restore_overlay(&mut self) {
        let running = self.worker_running();
        match self.worker.as_ref() {
            Some(w) if running => {
                w.restore();
                self.log("Overlay restore requested.");
            }
            _ => self.status = "Turn on 3D before restoring its overlay.".into(),
        }
    }

    pub fn recover_changed(&mut self, enabled: bool) {
        let running = self.worker_running();
        if let Some(message) = self.supervisor.recover_changed(enabled, running) {
            self.status = message;
        }
        self.save_preferences();
    }

    pub fn menu(&mut self, command: MenuCommand) {
        match command {
            MenuCommand::ShowControls => self.requests.push(UiRequest::ShowControls),
            MenuCommand::Toggle => self.toggle(),
            MenuCommand::RestoreOverlay => self.restore_overlay(),
            MenuCommand::KeepOverlayOnTop(value) => {
                self.prefs.keep_overlay_on_top = value;
                if !self.save_preferences() {
                    return;
                }
                if self.wanted() {
                    self.stop_worker();
                    self.start_worker();
                }
            }
            MenuCommand::AlwaysConvertDesktop => {
                self.prefs.mode = CaptureMode::Desktop;
                self.prefs.side_by_side = false;
                self.save_preferences();
                self.stop_conversion();
                self.start_conversion();
            }
            MenuCommand::SideBySide | MenuCommand::AiMode => {
                self.prefs.side_by_side = command == MenuCommand::SideBySide;
                if !self.save_preferences() {
                    return;
                }
                if self.wanted() {
                    self.stop_worker();
                    self.start_worker();
                }
            }
            MenuCommand::Restart => self.restart(),
            MenuCommand::Help => self.requests.push(UiRequest::ShowHelp),
            MenuCommand::About => self.requests.push(UiRequest::ShowAbout),
            MenuCommand::ToggleTheme => self.requests.push(UiRequest::ToggleTheme),
            MenuCommand::ViewLogs => platform::open_path(&paths::data_dir()),
            MenuCommand::MinimizeToTray => self.requests.push(UiRequest::MinimizeToTray),
            MenuCommand::Quit => self.quit(),
        }
    }

    pub fn quit(&mut self) {
        if self.exiting {
            return;
        }
        self.exiting = true;
        self.save_preferences();
        self.stop_conversion();
        // Keep the window and poller responsive until background cleanup finishes.
    }

    /// One supervision tick: external requests, worker state, recovery.
    pub fn poll(&mut self, now: Instant) {
        if let Some(events) = &self.events {
            if events.quit.is_signaled() {
                self.quit();
                return;
            }
            if events.show.is_signaled() {
                self.requests.push(UiRequest::ShowControls);
            }
            if events.toggle.is_signaled() {
                self.toggle();
            }
        }
        self.poll_operation();
        if self.operation.is_some() {
            return;
        }
        if self.exiting {
            if self.ready_to_exit() && !self.requests.contains(&UiRequest::Quit) {
                self.requests.push(UiRequest::Quit);
            }
            return;
        }
        let exited = self.worker.as_mut().and_then(|w| w.has_exited());
        match (self.worker.is_some(), exited) {
            (false, _) => {
                if self.supervisor.tick_without_worker(now) == Action::StartWorker {
                    self.start_worker();
                }
            }
            (true, Some(code)) => {
                self.worker = None;
                self.status = self.supervisor.worker_exited(code, now);
            }
            (true, None) => {
                if let Some(worker) = &self.worker {
                    if let Some(status) = read_status(worker.pid()) {
                        self.status = status;
                    }
                }
            }
        }
    }
}

fn read_status(worker_pid: u32) -> Option<String> {
    let text = std::fs::read_to_string(paths::worker_status_file()).ok()?;
    let status: WorkerStatus = serde_json::from_str(&text).ok()?;
    (status.pid == worker_pid).then_some(status.state)
}

fn settings_recovery_message(error: &str) -> String {
    format!("{error}. Automatic conversion is disabled. Fix the settings file and reopen, or choose settings and click Apply to save them with a backup of the original.")
}

/// Load preferences, importing Any2Odd's file once when ours does not exist.
fn load_preferences(logger: &Logger) -> Result<Preferences, String> {
    let own = paths::settings_file();
    if !own
        .try_exists()
        .map_err(|e| format!("Could not inspect saved settings: {e}"))?
    {
        if let Some(legacy) = paths::legacy_any2odd_settings_file() {
            if legacy
                .try_exists()
                .map_err(|e| format!("Could not inspect legacy settings: {e}"))?
            {
                let mut imported = Preferences::load_from(&legacy)
                    .map_err(|e| format!("Could not load legacy settings: {e}"))?;
                imported
                    .save_to(&own)
                    .map_err(|e| format!("Could not import legacy settings: {e}"))?;
                logger.log(&format!("Imported preferences from {}", legacy.display()));
                return Ok(imported);
            }
        }
    }
    Preferences::load().map_err(|e| format!("Could not load saved settings: {e}"))
}

/// Start the poller thread. It wakes the GUI (and un-hides the window when
/// another instance or a hotkey asks for the controls).
pub fn spawn_poller(core: Arc<Mutex<Core>>, ctx: egui::Context) {
    std::thread::Builder::new()
        .name("any2odyssey-poller".into())
        .spawn(move || loop {
            std::thread::sleep(POLL_INTERVAL);
            let mut show = false;
            let mut quit = false;
            let repaint;
            let hwnd;
            {
                let Ok(mut core) = core.lock() else { break };
                let previous = UiState::capture(&core);
                crate::hotkeys::drain(&mut core);
                #[cfg(windows)]
                crate::tray::drain(&mut core);
                core.poll(Instant::now());
                if core.requests.contains(&UiRequest::ShowControls)
                    || core.requests.contains(&UiRequest::ShowHelp)
                    || core.requests.contains(&UiRequest::ShowAbout)
                {
                    show = true;
                }
                if core.requests.contains(&UiRequest::Quit) {
                    quit = true;
                }
                hwnd = core.controls_hwnd;
                repaint = previous.needs_repaint(&core);
            }
            // Cross-thread Win32 window calls can synchronously dispatch on
            // the GUI thread. Never hold Core while making them.
            if show || quit {
                platform::show_window(hwnd);
            }
            if repaint {
                ctx.request_repaint();
            }
            if quit {
                break;
            }
        })
        .expect("poller thread");
}

pub fn version_line() -> String {
    format!(
        "Version {}  ·  {}",
        any2odyssey_core::VERSION,
        any2odyssey_core::TARGET_DISPLAY
    )
}

pub fn shortcuts_line() -> &'static str {
    "Ctrl+Alt+F10  On / off    ·    Ctrl+Alt+F9  Controls    ·    Ctrl+Alt+F8  Restore overlay    ·    F1  Help"
}

#[cfg(test)]
mod tests {
    use super::*;

    static SETTINGS_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct RestoreDataDir(Option<std::ffi::OsString>);

    impl Drop for RestoreDataDir {
        fn drop(&mut self) {
            if let Some(old) = self.0.take() {
                std::env::set_var(paths::DATA_DIR_ENV, old);
            } else {
                std::env::remove_var(paths::DATA_DIR_ENV);
            }
        }
    }

    #[test]
    fn damaged_settings_block_launch_and_autosave_until_explicit_apply() {
        let _guard = SETTINGS_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _restore = RestoreDataDir(std::env::var_os(paths::DATA_DIR_ENV));
        std::env::set_var(paths::DATA_DIR_ENV, dir.path());
        let path = dir.path().join("settings.json");
        let original = br#"{"Mode":"Window","AutoConvert":false,"Depth":"#;
        std::fs::write(&path, original).unwrap();
        let make_core = || Core::new(Logger::new(dir.path().join("test.log")), None, true);
        let mut core = make_core();
        assert!(!core.prefs.auto_convert);
        assert!(core.settings_load_error.is_some());
        assert!(core.status.contains("Automatic conversion is disabled"));
        core.start_conversion();
        assert!(!core.wanted());
        assert!(core.operation.is_none());
        assert!(!core.save_preferences());
        core.quit();
        assert_eq!(std::fs::read(&path).unwrap(), original);

        let mut core = make_core();
        core.prefs.mode = CaptureMode::Window;
        core.prefs.depth = 1.25;
        core.apply();
        assert!(core.settings_load_error.is_none());
        assert!(!core.wanted());
        let saved = Preferences::load_from(&path).unwrap();
        assert_eq!(saved.mode, CaptureMode::Window);
        assert_eq!(saved.depth, 1.25);
        assert!(!saved.auto_convert);
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("settings-recovery-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), original);
        assert!(make_core().settings_load_error.is_none());
    }

    #[test]
    fn unreadable_settings_cannot_be_replaced_without_a_backup() {
        let _guard = SETTINGS_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _restore = RestoreDataDir(std::env::var_os(paths::DATA_DIR_ENV));
        std::env::set_var(paths::DATA_DIR_ENV, dir.path());
        let path = dir.path().join("settings.json");
        std::fs::create_dir(&path).unwrap();
        let mut core = Core::new(Logger::new(dir.path().join("test.log")), None, false);
        core.apply();
        assert!(core.settings_load_error.is_some());
        assert!(!core.prefs.auto_convert);
        assert!(!core.wanted());
        assert!(path.is_dir());
    }

    #[test]
    fn failed_save_blocks_start_apply_and_restart() {
        let _guard = SETTINGS_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // A directory at settings.json reliably refuses replacement on both OSes.
        let settings = dir.path().join("settings.json");
        std::fs::create_dir(&settings).unwrap();
        let old = std::env::var_os(paths::DATA_DIR_ENV);
        std::env::set_var(paths::DATA_DIR_ENV, dir.path());
        let mut core = isolated(dir.path());
        core.preview = false;
        core.prefs.mode = CaptureMode::Window;
        core.selected_window = Some(42);
        for action in [Core::start_conversion, Core::apply, Core::restart] {
            action(&mut core);
            assert!(!core.wanted());
            assert!(core.operation.is_none());
            assert_eq!(core.generation, 0);
            assert!(core.status.starts_with("Could not save preferences:"));
        }
        if let Some(old) = old {
            std::env::set_var(paths::DATA_DIR_ENV, old);
        } else {
            std::env::remove_var(paths::DATA_DIR_ENV);
        }
    }

    fn isolated(dir: &std::path::Path) -> Core {
        Core {
            prefs: Preferences::default(),
            ui: UiSettings::default(),
            status: "Ready".into(),
            settings_load_error: None,
            supervisor: Supervisor::new(true),
            worker: None,
            operation: None,
            generation: 0,
            windows: Vec::new(),
            selected_window: None,
            hotkeys_ok: false,
            start_requested: false,
            preview: true,
            screenshot: None,
            exiting: false,
            requests: Vec::new(),
            logger: Logger::new(dir.join("test.log")),
            events: None,
            controls_hwnd: 0,
        }
    }

    #[test]
    fn pending_operation_does_not_block_poll_and_quit_waits_for_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = isolated(dir.path());
        let (tx, rx) = mpsc::channel();
        core.operation = Some(rx);
        core.poll(Instant::now());
        assert!(core.operation.is_some());
        core.quit();
        core.poll(Instant::now());
        assert!(!core.ready_to_exit());
        assert!(!core.requests.contains(&UiRequest::Quit));
        tx.send(WorkerResult::Stopped(None))
            .unwrap_or_else(|_| panic!("receiver closed"));
        core.poll(Instant::now());
        assert!(core.ready_to_exit());
        assert!(core.requests.contains(&UiRequest::Quit));
    }

    #[test]
    fn cancelled_start_failure_does_not_erase_newer_start_intent() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = isolated(dir.path());
        core.supervisor.start_requested();
        let generation = core.generation;
        let (tx, rx) = mpsc::channel();
        core.operation = Some(rx);
        core.stop_worker(); // Invalidate the old launch, as Apply does.
        tx.send(WorkerResult::Started {
            generation,
            result: Err("cancelled attempt".into()),
        })
        .unwrap_or_else(|_| panic!("receiver closed"));
        core.poll_operation();
        assert!(core.wanted());
        assert_eq!(core.status, "Ready");
        assert!(core.operation.is_none());
    }

    #[test]
    fn current_start_failure_clears_intent_and_reports_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = isolated(dir.path());
        core.supervisor.start_requested();
        let (tx, rx) = mpsc::channel();
        core.operation = Some(rx);
        tx.send(WorkerResult::Started {
            generation: core.generation,
            result: Err("missing engine".into()),
        })
        .unwrap_or_else(|_| panic!("receiver closed"));
        core.poll_operation();
        assert!(!core.wanted());
        assert!(core.status.contains("missing engine"));
    }
    #[test]
    fn unchanged_polls_do_not_redraw_but_status_commands_and_quit_do() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = isolated(dir.path());
        let previous = UiState::capture(&core);
        for _ in 0..10 {
            core.poll(Instant::now());
            assert!(!previous.needs_repaint(&core));
        }
        core.status = "3D active".into();
        assert!(previous.needs_repaint(&core));
        let previous = UiState::capture(&core);
        core.prefs.side_by_side = true;
        assert!(previous.needs_repaint(&core));
        let previous = UiState::capture(&core);
        core.requests.push(UiRequest::ShowControls);
        assert!(previous.needs_repaint(&core));
        core.requests.clear();
        core.quit();
        assert!(previous.needs_repaint(&core));
    }

    #[test]
    fn refreshing_sources_never_selects_an_unrequested_window() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = isolated(dir.path());
        let choices = |handles: &[isize]| {
            handles
                .iter()
                .map(|&handle| WindowChoice {
                    handle,
                    title: format!("Window {handle}"),
                })
                .collect()
        };
        core.set_windows(choices(&[10, 20]));
        assert_eq!(core.selected_window, None, "startup requires a choice");
        core.selected_window = Some(20);
        core.set_windows(choices(&[30, 20, 10]));
        assert_eq!(core.selected_window, Some(20), "retain a valid choice");
        core.set_windows(choices(&[30, 10]));
        assert_eq!(core.selected_window, None, "do not substitute another app");
        core.set_windows(choices(&[20, 30, 10]));
        assert_eq!(core.selected_window, None, "do not revive a cleared choice");
    }
}
