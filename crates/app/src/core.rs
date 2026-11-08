//! Shared application state and the supervisor loop. The GUI thread renders
//! from it; a poller thread drives worker supervision, named events, tray
//! menu and hotkey events every 250 ms, so everything keeps working while the
//! controls window is hidden in the tray.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use any2odyssey_core::log::Logger;
use any2odyssey_core::paths;
use any2odyssey_core::prefs::{CaptureMode, Preferences, UiSettings};
use any2odyssey_core::status::WorkerStatus;
use any2odyssey_core::supervisor::{Action, Supervisor};

use crate::platform::{self, ControlEvents, Worker};

pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

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
    ToggleTheme,
    ViewLogs,
    MinimizeToTray,
    Quit,
}

pub struct Core {
    pub prefs: Preferences,
    pub ui: UiSettings,
    pub status: String,
    pub supervisor: Supervisor,
    pub worker: Option<Worker>,
    pub windows: Vec<WindowChoice>,
    pub selected_window: Option<isize>,
    pub hotkeys_ok: bool,
    pub start_requested: bool,
    pub exiting: bool,
    pub requests: Vec<UiRequest>,
    pub logger: Logger,
    events: Option<ControlEvents>,
    pub controls_hwnd: isize,
}

impl Core {
    pub fn new(logger: Logger, events: Option<ControlEvents>, start_requested: bool) -> Core {
        let prefs = load_preferences(&logger);
        let ui = UiSettings::load();
        let mut core = Core {
            supervisor: Supervisor::new(prefs.recover),
            prefs,
            ui,
            status: "Ready".into(),
            worker: None,
            windows: Vec::new(),
            selected_window: None,
            hotkeys_ok: false,
            start_requested,
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

    pub fn save_preferences(&mut self) {
        self.prefs.recover = self.supervisor.recover;
        if let Err(e) = self.prefs.save() {
            self.log(&format!("Could not save preferences: {e}"));
        }
    }

    pub fn save_ui(&self) {
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
        self.windows = platform::list_windows(worker_pid)
            .into_iter()
            .map(|(handle, title)| WindowChoice { handle, title })
            .collect();
        if let Some(selected) = self.selected_window {
            if !self.windows.iter().any(|w| w.handle == selected) {
                self.selected_window = self.windows.first().map(|w| w.handle);
            }
        } else {
            self.selected_window = self.windows.first().map(|w| w.handle);
        }
    }

    pub fn start_conversion(&mut self) {
        self.save_preferences();
        self.supervisor.start_requested();
        self.start_worker();
    }

    pub fn stop_conversion(&mut self) {
        self.supervisor.stop_requested();
        self.stop_worker();
        self.status = "3D is off. Your normal desktop is restored.".into();
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
        self.save_preferences();
        if self.wanted() {
            self.stop_worker();
            self.start_worker();
        } else {
            self.status = "Preferences saved.".into();
        }
    }

    pub fn restart(&mut self) {
        self.save_preferences();
        self.stop_worker();
        self.supervisor.start_requested();
        self.start_worker();
    }

    fn start_worker(&mut self) {
        if self.worker_running() {
            return;
        }
        if let Err(e) = platform::validate() {
            self.status = e;
            self.supervisor.start_failed();
            return;
        }
        self.save_preferences();
        let source = self.selected_window.unwrap_or(0);
        if self.prefs.mode == CaptureMode::Window && source == 0 {
            self.status = "Choose a source window first.".into();
            self.supervisor.start_failed();
            return;
        }
        match Worker::spawn(source) {
            Ok(worker) => {
                self.log(&format!(
                    "Started renderer PID {} mode {:?}",
                    worker.pid(),
                    self.prefs.mode
                ));
                self.status = "Starting 3D — allow a few seconds for the AI model.".into();
                self.worker = Some(worker);
            }
            Err(e) => {
                self.supervisor.start_failed();
                self.status = format!("Could not start conversion: {e}");
                self.log(&format!("Could not start conversion: {e}"));
            }
        }
    }

    fn stop_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            if let Some(message) = worker.stop() {
                self.log(&message);
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
                self.save_preferences();
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
                self.save_preferences();
                if self.wanted() {
                    self.stop_worker();
                    self.start_worker();
                }
            }
            MenuCommand::Restart => self.restart(),
            MenuCommand::Help => self.requests.push(UiRequest::ShowHelp),
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
        self.requests.push(UiRequest::Quit);
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

/// Load preferences, importing Any2Odd's file once when ours does not exist.
fn load_preferences(logger: &Logger) -> Preferences {
    let own = paths::settings_file();
    if !own.exists() {
        if let Some(legacy) = paths::legacy_any2odd_settings_file().filter(|p| p.is_file()) {
            let mut imported = Preferences::load_from(&legacy);
            if imported.save_to(&own).is_ok() {
                logger.log(&format!("Imported preferences from {}", legacy.display()));
            }
            return imported;
        }
    }
    Preferences::load()
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
            {
                let Ok(mut core) = core.lock() else { break };
                crate::hotkeys::drain(&mut core);
                #[cfg(windows)]
                crate::tray::drain(&mut core);
                core.poll(Instant::now());
                if core.requests.contains(&UiRequest::ShowControls)
                    || core.requests.contains(&UiRequest::ShowHelp)
                {
                    show = true;
                }
                if core.requests.contains(&UiRequest::Quit) {
                    quit = true;
                }
                if core.exiting && core.requests.is_empty() && core.worker.is_none() {
                    // Quit handled by the GUI; stop polling once it is gone.
                }
                if show || quit {
                    platform::show_window(core.controls_hwnd);
                }
            }
            ctx.request_repaint();
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
