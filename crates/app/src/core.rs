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
use any2odyssey_core::update::{self, Comparison, UpdateState};

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
    CheckUpdates,
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
    update: UpdateState,
}

impl UiState {
    fn capture(core: &Core) -> Self {
        Self {
            status: core.status.clone(),
            prefs: core.prefs.clone(),
            wanted: core.wanted(),
            exiting: core.exiting,
            update: core.update.clone(),
        }
    }

    fn needs_repaint(&self, core: &Core) -> bool {
        !core.requests.is_empty()
            || self.status != core.status
            || self.prefs != core.prefs
            || self.wanted != core.wanted()
            || self.exiting != core.exiting
            || self.update != core.update
    }
}

pub struct Core {
    pub prefs: Preferences,
    pub ui: UiSettings,
    pub status: String,
    /// The status line answers a deliberate user action ("Automatic recovery
    /// cancelled") and must survive the settling of an operation that was
    /// already in flight. Cleared by the next user-initiated start, stop or
    /// apply, which write their own status.
    status_sticky: bool,
    settings_load_error: Option<String>,
    pub supervisor: Supervisor,
    pub worker: Option<Worker>,
    operation: Option<mpsc::Receiver<WorkerResult>>,
    generation: u64,
    /// Result of the background update check, see `check_updates`.
    pub update: UpdateState,
    update_task: Option<mpsc::Receiver<Result<Comparison, String>>>,
    pub windows: Vec<WindowChoice>,
    pub selected_window: Option<isize>,
    pub hotkeys_ok: bool,
    pub start_requested: bool,
    /// Isolated preview (screenshots, UI checks): never starts a worker or writes preferences.
    pub preview: bool,
    /// `--screenshot FILE [--dark] [--help-page]`: render, save, exit.
    pub screenshot: Option<ScreenshotRequest>,
    /// How the screenshot ended, so the process can exit non-zero when the
    /// image was never delivered or could not be saved.
    pub screenshot_outcome: Option<Result<(), String>>,
    pub exiting: bool,
    pub requests: Vec<UiRequest>,
    pub logger: Logger,
    events: Option<ControlEvents>,
    pub controls_hwnd: isize,
}

impl Core {
    pub fn new(logger: Logger, events: Option<ControlEvents>, start_requested: bool) -> Core {
        Self::initialize(logger, events, start_requested, false)
    }

    pub fn new_preview(logger: Logger) -> Core {
        Self::initialize(logger, None, false, true)
    }

    fn initialize(
        logger: Logger,
        events: Option<ControlEvents>,
        start_requested: bool,
        preview: bool,
    ) -> Core {
        // Preview isolation must precede preference loading: normal startup
        // can import and save legacy settings before the UI is constructed.
        let loaded = if preview {
            Ok(Preferences::default())
        } else {
            load_preferences(&logger)
        };
        let (prefs, settings_load_error) = match loaded {
            Ok(prefs) => (prefs, None),
            Err(error) => {
                logger.log(&error);
                (
                    Preferences {
                        auto_convert: false,
                        check_updates: false,
                        ..Default::default()
                    },
                    Some(error),
                )
            }
        };
        let ui = if preview {
            UiSettings::default()
        } else {
            let (ui, problem) = UiSettings::load();
            if let Some(problem) = problem {
                // Presentation settings are not worth a recovery prompt, but
                // the theme and the update-check timestamp silently reset, so
                // leave a trace before the next save overwrites the file.
                logger.log(&format!(
                    "ui-settings.json could not be read and was reset to defaults: {problem}"
                ));
            }
            let swept = any2odyssey_core::fsutil::remove_stale_temporaries(&paths::data_dir());
            if swept > 0 {
                logger.log(&format!(
                    "Removed {swept} stale temporary file(s) left by an interrupted write."
                ));
            }
            ui
        };
        let mut core = Core {
            supervisor: Supervisor::new(prefs.recover),
            prefs,
            ui,
            status: settings_load_error
                .as_deref()
                .map(settings_recovery_message)
                .unwrap_or_else(|| "Ready".into()),
            status_sticky: false,
            settings_load_error,
            worker: None,
            operation: None,
            generation: 0,
            update: UpdateState::Unchecked,
            update_task: None,
            windows: Vec::new(),
            selected_window: None,
            hotkeys_ok: false,
            start_requested,
            preview,
            screenshot: None,
            screenshot_outcome: None,
            exiting: false,
            requests: Vec::new(),
            logger,
            events,
            controls_hwnd: 0,
        };
        if !preview {
            core.refresh_windows();
        }
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
        self.status_sticky = false;
        if !self.save_preferences() {
            return;
        }
        self.supervisor.start_requested();
        self.start_worker();
    }

    pub fn stop_conversion(&mut self) {
        self.status_sticky = false;
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
        self.status_sticky = false;
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
        self.status_sticky = false;
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
            self.status = "Choose a source window and click Apply first.".into();
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
            // A relaunch the user has just cancelled reports in and is stopped
            // here; its progress is not what the status line is about then.
            if !self.status_sticky {
                self.status = "Stopping 3D…".into();
            }
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
                Err(_) => {
                    // A cancelled attempt must not override newer intent. But
                    // when the user turned 3D off while this start was still
                    // validating, "Stopping 3D…" is now stale: nothing else
                    // will report that the desktop is back to normal.
                    if !self.wanted() && self.worker.is_none() && !self.exiting {
                        self.report_off();
                    }
                }
            },
            WorkerResult::Stopped(message) => {
                if let Some(message) = message {
                    self.log(&message);
                }
                if !self.wanted() {
                    self.report_off();
                }
            }
        }
    }

    /// The settled state after a stop, unless the status line still carries
    /// the answer to the user action that caused it (recovery cancelled while
    /// its relaunch was in flight): that message already says 3D is off and
    /// how to turn it back on, and must not be replaced a moment later.
    fn report_off(&mut self) {
        if !self.status_sticky {
            self.status = "3D is off. Your normal desktop is restored.".into();
        }
    }

    /// Start one background check; the answer arrives through `poll`. The
    /// network call never runs under the shared-state lock or in a preview.
    pub fn check_updates(&mut self) {
        if self.preview || self.update_task.is_some() {
            return;
        }
        self.update = UpdateState::Checking;
        let (tx, rx) = mpsc::channel();
        self.update_task = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(platform::check_updates());
        });
    }

    fn poll_update(&mut self) {
        let result = match self.update_task.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => result,
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                Err("the update check stopped unexpectedly".to_owned())
            }
            _ => return,
        };
        self.update_task = None;
        self.update = UpdateState::from_result(result);
        self.log(&format!("Update check: {}", self.update.summary()));
        if !matches!(self.update, UpdateState::Failed(_)) {
            self.ui.last_update_check = Some(update::now_unix());
            self.save_ui();
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
            // The cancelled relaunch may still be validating; when its result
            // arrives, the settling messages must not replace this answer.
            self.status = message;
            self.status_sticky = true;
        }
        self.save_preferences();
    }

    /// Queue a command for the GUI thread. While a deferred quit runs, the
    /// window must stay where it is: a show request (tray double-click,
    /// Ctrl+Alt+F9, the menus) would un-hide and activate the controls for
    /// the seconds the worker takes to stop, stealing the focus from a game
    /// just as `--show` did before 0.8.5. Such requests are dropped; the
    /// others (theme, tray, quit) still apply.
    pub fn request(&mut self, request: UiRequest) {
        if self.exiting
            && matches!(
                request,
                UiRequest::ShowControls | UiRequest::ShowHelp | UiRequest::ShowAbout
            )
        {
            return;
        }
        self.requests.push(request);
    }

    pub fn menu(&mut self, command: MenuCommand) {
        match command {
            MenuCommand::ShowControls => self.request(UiRequest::ShowControls),
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
                if !self.save_preferences() {
                    return;
                }
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
            MenuCommand::Help => self.request(UiRequest::ShowHelp),
            MenuCommand::About => self.request(UiRequest::ShowAbout),
            MenuCommand::CheckUpdates => {
                self.check_updates();
                self.request(UiRequest::ShowAbout);
            }
            MenuCommand::ToggleTheme => self.request(UiRequest::ToggleTheme),
            MenuCommand::ViewLogs => platform::open_path(&paths::data_dir()),
            MenuCommand::MinimizeToTray => self.request(UiRequest::MinimizeToTray),
            MenuCommand::Quit => self.quit(),
        }
    }

    pub fn quit(&mut self) {
        if self.exiting {
            return;
        }
        self.exiting = true;
        // The poller may drain Show and Quit in the same batch. Remove
        // previously accepted shows as well as refusing new ones in request.
        self.requests.retain(|request| {
            !matches!(
                request,
                UiRequest::ShowControls | UiRequest::ShowHelp | UiRequest::ShowAbout
            )
        });
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
            let (show, toggle) = (events.show.is_signaled(), events.toggle.is_signaled());
            // During a deferred quit `request` drops the show, so the window
            // does not come back for a frame; the `--show` launch that set
            // the event has already exited 0 and is not told.
            if show {
                self.request(UiRequest::ShowControls);
            }
            if toggle {
                self.toggle();
            }
        }
        self.poll_update();
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
                // The worker's error paths write their reason to
                // worker-status.json before exiting. Read it while the pid
                // still matches, so the status line carries it instead of
                // only a number; the log keeps the line as well.
                let reason = self.worker.as_ref().and_then(|w| read_status(w.pid()));
                self.worker = None;
                self.status = self.supervisor.worker_exited(code, reason.as_deref(), now);
                self.log(&format!(
                    "Renderer exited with code {code}: {}",
                    self.status
                ));
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

/// Open a page from an update result in the browser. Only GitHub's own hosts
/// pass; anything else is ignored. The check runs here, synchronously; the
/// launch itself runs on its own thread because `open` can wait up to 250 ms
/// for the launcher and the caller holds the shared state inside a GUI frame.
pub fn open_release_url(url: &str) {
    if !update::allowed_url(url) {
        return;
    }
    let url = url.to_owned();
    std::thread::spawn(move || {
        let _ = open::that_detached(url);
    });
}

fn settings_recovery_message(error: &str) -> String {
    format!("{error}. Automatic conversion is disabled, and so is the daily update check until you turn it back on from the About page. Fix the settings file and reopen, or choose settings and click Apply to save them with a backup of the original.")
}

/// Load preferences, importing Any2Odd's file when ours does not exist.
fn load_preferences(logger: &Logger) -> Result<Preferences, String> {
    load_preferences_from(
        logger,
        &paths::settings_file(),
        paths::legacy_any2odd_settings_file().as_deref(),
    )
}

/// Only damage to our own file is an error (and gates automatic start and
/// the saves until Apply, see `Core::initialize`). Any2Odd's file is a
/// courtesy import: failures are logged without blocking the controls, but
/// automatic conversion and update checks stay off until the user opts in.
fn load_preferences_from(
    logger: &Logger,
    own: &std::path::Path,
    legacy: Option<&std::path::Path>,
) -> Result<Preferences, String> {
    if !own
        .try_exists()
        .map_err(|e| format!("Could not inspect saved settings: {e}"))?
    {
        if let Some(legacy) = legacy {
            let imported = legacy.try_exists().and_then(|exists| {
                if exists {
                    Preferences::load_from(legacy).map(Some)
                } else {
                    Ok(None)
                }
            });
            match imported {
                Ok(Some(mut imported)) => {
                    imported
                        .save_to(own)
                        .map_err(|e| format!("Could not import legacy settings: {e}"))?;
                    logger.log(&format!("Imported preferences from {}", legacy.display()));
                    return Ok(imported);
                }
                Ok(None) => {}
                Err(e) => {
                    logger.log(&format!(
                        "Could not import Any2Odd's settings from {}: {e}. Starting with automatic conversion and update checks disabled; Any2Odd's file is left as it is.",
                        legacy.display()
                    ));
                    return Ok(Preferences {
                        auto_convert: false,
                        check_updates: false,
                        ..Default::default()
                    });
                }
            }
        }
    }
    Preferences::load_from(own).map_err(|e| format!("Could not load saved settings: {e}"))
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
            // the GUI thread. Never hold Core while making them. Only a show
            // request un-hides the window: a quit from the tray or `--quit`
            // while hidden must not flash the controls and steal the focus
            // from a game for one frame. The repaint wakes eframe's loop so
            // the frame turns the Quit request into a window close; a hidden
            // window still gets that frame because `hide_window` minimizes
            // instead of hiding (see platform).
            if show {
                platform::show_window(hwnd);
            }
            if repaint {
                ctx.request_repaint();
            }
            if quit {
                // The frame normally closes the window within milliseconds.
                // Should it never come (no paint message reaches the window),
                // the worker is already stopped and the preferences saved, so
                // ending the process is the right outcome, not a zombie that
                // only Task Manager can remove.
                ctx.request_repaint();
                std::thread::sleep(QUIT_GRACE);
                if let Ok(core) = core.lock() {
                    core.log("The window did not close after the quit request; exiting directly.");
                }
                std::process::exit(0);
            }
        })
        .expect("poller thread");
}

/// How long the poller gives the GUI to close the window after a quit
/// request before it ends the process itself.
const QUIT_GRACE: Duration = Duration::from_secs(3);

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
    fn preview_initialization_ignores_saved_settings_and_never_saves_them() {
        let _guard = SETTINGS_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _restore = RestoreDataDir(std::env::var_os(paths::DATA_DIR_ENV));
        std::env::set_var(paths::DATA_DIR_ENV, dir.path());
        let settings = dir.path().join("settings.json");
        let ui = dir.path().join("ui-settings.json");
        let saved_ui = br#"{"DarkMode":true,"LastUpdateCheck":1800000000}"#;
        std::fs::write(&ui, saved_ui).unwrap();
        for original in [
            br#"{"Mode":"Window","AutoConvert":false,"Depth":1.5}"#.as_slice(),
            b"damaged preferences".as_slice(),
        ] {
            std::fs::write(&settings, original).unwrap();
            let mut core = Core::new_preview(Logger::new(dir.path().join("test.log")));
            assert!(core.preview);
            assert_eq!(core.prefs, Preferences::default());
            assert_eq!(core.ui, UiSettings::default());
            assert!(core.settings_load_error.is_none());
            assert!(core.windows.is_empty());
            core.prefs.depth = 1.25;
            core.apply();
            core.save_ui();
            core.check_updates();
            core.quit();
            assert!(core.operation.is_none());
            assert!(core.update_task.is_none());
            assert_eq!(std::fs::read(&settings).unwrap(), original);
            assert_eq!(std::fs::read(&ui).unwrap(), saved_ui);
        }
        std::fs::remove_file(&settings).unwrap();
        let mut core = Core::new_preview(Logger::new(dir.path().join("test.log")));
        core.apply();
        assert!(
            !settings.exists(),
            "previews do not create or import preferences"
        );
    }

    #[test]
    fn damaged_settings_block_launch_and_autosave_until_explicit_apply() {
        let _guard = SETTINGS_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _restore = RestoreDataDir(std::env::var_os(paths::DATA_DIR_ENV));
        std::env::set_var(paths::DATA_DIR_ENV, dir.path());
        let path = dir.path().join("settings.json");
        let original = br#"{"Mode":"Window","AutoConvert":false,"CheckUpdates":false,"Depth":"#;
        std::fs::write(&path, original).unwrap();
        let make_core = || Core::new(Logger::new(dir.path().join("test.log")), None, true);
        let mut core = make_core();
        assert!(!core.prefs.auto_convert);
        assert!(
            !core.prefs.check_updates,
            "unreadable preferences must not enable automatic network checks"
        );
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
        assert!(!saved.check_updates);
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
    fn a_damaged_any2odd_file_is_logged_and_skipped_not_treated_as_our_damage() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("settings.json");
        let legacy = dir.path().join("Odyssey3DCompanion").join("settings.json");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let log = dir.path().join("test.log");
        let logger = Logger::new(log.clone());
        // Readable legacy file: imported and written as our own.
        std::fs::write(&legacy, br#"{"Mode":"Active","AutoConvert":false}"#).unwrap();
        let prefs = load_preferences_from(&logger, &own, Some(&legacy)).unwrap();
        assert_eq!(prefs.mode, CaptureMode::Active);
        assert!(!prefs.auto_convert);
        assert!(own.exists());
        // Damaged legacy file: no error or write, but no automatic work.
        std::fs::remove_file(&own).unwrap();
        std::fs::write(&legacy, b"not json").unwrap();
        let prefs = load_preferences_from(&logger, &own, Some(&legacy)).unwrap();
        assert_eq!(
            prefs,
            Preferences {
                auto_convert: false,
                check_updates: false,
                ..Default::default()
            }
        );
        assert!(!own.exists(), "nothing is imported from a damaged file");
        assert_eq!(std::fs::read(&legacy).unwrap(), b"not json");
        assert!(std::fs::read_to_string(&log)
            .unwrap()
            .contains("Could not import Any2Odd's settings"));
        // A legacy path that does not exist imports nothing and writes nothing.
        std::fs::remove_file(&legacy).unwrap();
        let prefs = load_preferences_from(&logger, &own, Some(&legacy)).unwrap();
        assert_eq!(prefs, Preferences::default());
        assert!(!own.exists());
        // Our own file still gates: a damaged one is the error it always was.
        std::fs::write(&own, b"damaged").unwrap();
        assert!(load_preferences_from(&logger, &own, Some(&legacy)).is_err());
    }

    #[test]
    fn failed_legacy_import_never_enables_automatic_work() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("settings.json");
        let legacy = dir.path().join("legacy.json");
        let logger = Logger::new(dir.path().join("test.log"));
        // A truncated file can still contain the old opt-outs; failing to
        // parse it must not turn desktop capture or network checks back on.
        std::fs::write(
            &legacy,
            br#"{"AutoConvert":false,"CheckUpdates":false,"Mode":"Window","#,
        )
        .unwrap();
        let check = |path: &std::path::Path| {
            let prefs = load_preferences_from(&logger, &own, Some(path)).unwrap();
            assert!(
                !prefs.auto_convert,
                "failed import enabled desktop conversion"
            );
            assert!(!prefs.check_updates, "failed import enabled update checks");
            assert!(!own.exists());
        };
        check(&legacy);
        // Some OSes report a path below a regular file as an inspection
        // error; others report it as absent. Exercise the error when present.
        let blocked = legacy.join("settings.json");
        if blocked.try_exists().is_err() {
            check(&blocked);
        }
        // An existing directory cannot be read as a settings file.
        std::fs::remove_file(&legacy).unwrap();
        std::fs::create_dir(&legacy).unwrap();
        check(&legacy);
        // A valid current file remains authoritative, including opt-ins.
        let mut saved = Preferences::default();
        saved.save_to(&own).unwrap();
        assert_eq!(
            load_preferences_from(&logger, &own, Some(&legacy)).unwrap(),
            saved
        );
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
        // Restored by the guard even when an assertion below fails, so a
        // failure cannot point later tests at a deleted directory.
        let _restore = RestoreDataDir(std::env::var_os(paths::DATA_DIR_ENV));
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
    }

    #[test]
    fn failed_menu_save_keeps_the_existing_worker_running() {
        let _guard = SETTINGS_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _restore = RestoreDataDir(std::env::var_os(paths::DATA_DIR_ENV));
        std::env::set_var(paths::DATA_DIR_ENV, dir.path());
        // Refuse every settings write without depending on OS permissions.
        std::fs::create_dir(dir.path().join("settings.json")).unwrap();
        for command in [
            MenuCommand::KeepOverlayOnTop(false),
            MenuCommand::SideBySide,
            MenuCommand::AiMode,
            MenuCommand::AlwaysConvertDesktop,
        ] {
            let mut core = isolated(dir.path());
            core.preview = false;
            core.prefs.mode = CaptureMode::Window;
            core.prefs.side_by_side = true;
            core.supervisor.start_requested();
            core.worker = Some(Worker::fake());
            let pid = core.worker.as_ref().unwrap().pid();

            core.menu(command);

            assert!(
                core.wanted(),
                "{command:?}: conversion intent was cancelled"
            );
            assert_eq!(core.generation, 0, "{command:?}: stop was requested");
            assert!(
                core.operation.is_none(),
                "{command:?}: worker control task started"
            );
            assert_eq!(core.worker.as_ref().map(Worker::pid), Some(pid));
            assert!(core.status.starts_with("Could not save preferences:"));
            core.take_shutdown()();
        }
    }

    fn isolated(dir: &std::path::Path) -> Core {
        Core {
            prefs: Preferences::default(),
            ui: UiSettings::default(),
            status: "Ready".into(),
            status_sticky: false,
            settings_load_error: None,
            supervisor: Supervisor::new(true),
            worker: None,
            operation: None,
            generation: 0,
            update: UpdateState::Unchecked,
            update_task: None,
            windows: Vec::new(),
            selected_window: None,
            hotkeys_ok: false,
            start_requested: false,
            preview: true,
            screenshot: None,
            screenshot_outcome: None,
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
    fn turning_off_during_a_failing_start_reports_the_desktop_restored() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = isolated(dir.path());
        core.supervisor.start_requested();
        let generation = core.generation;
        let (tx, rx) = mpsc::channel();
        core.operation = Some(rx);
        core.status = "Starting 3D — checking prerequisites and loading the AI model.".into();
        core.stop_conversion(); // Turn off while the start is still validating.
        assert_eq!(core.status, "Stopping 3D…");
        tx.send(WorkerResult::Started {
            generation,
            result: Err("missing engine".into()),
        })
        .unwrap_or_else(|_| panic!("receiver closed"));
        core.poll_operation();
        assert!(!core.wanted());
        assert!(core.operation.is_none());
        assert_eq!(core.status, "3D is off. Your normal desktop is restored.");
    }

    #[test]
    fn cancelling_recovery_keeps_its_message_when_the_relaunch_settles() {
        use any2odyssey_core::supervisor::RECOVERY_DELAY;
        const CANCELLED: &str = "Automatic recovery cancelled. Turn on 3D to restart manually.";
        let dir = tempfile::tempdir().unwrap();
        for relaunch_succeeds in [true, false] {
            let mut core = isolated(dir.path());
            core.supervisor.start_requested();
            let now = Instant::now();
            // The worker dies, recovery queues a retry, the retry fires and
            // its launch is validating when the user unchecks recovery.
            core.status = core.supervisor.worker_exited(1, None, now);
            assert!(core.supervisor.retry_pending());
            assert_eq!(
                core.supervisor.tick_without_worker(now + RECOVERY_DELAY),
                Action::StartWorker
            );
            let generation = core.generation;
            let (tx, rx) = mpsc::channel();
            core.operation = Some(rx);
            core.status = "Starting 3D — checking prerequisites and loading the AI model.".into();
            core.recover_changed(false);
            assert!(!core.wanted());
            assert_eq!(core.status, CANCELLED);
            let result = if relaunch_succeeds {
                Ok(Worker::fake())
            } else {
                Err("missing engine".to_owned())
            };
            tx.send(WorkerResult::Started { generation, result })
                .unwrap_or_else(|_| panic!("receiver closed"));
            // A launched worker is stopped on arrival; wait for that stop to
            // report in so both settling messages have had their chance.
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                core.poll(Instant::now());
                if core.operation.is_none() || Instant::now() > deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(core.operation.is_none(), "the stop never reported in");
            assert!(core.worker.is_none());
            assert!(!core.wanted());
            assert_eq!(core.status, CANCELLED, "relaunch ok: {relaunch_succeeds}");
            // The next deliberate action speaks for itself again.
            core.start_conversion();
            assert!(!core.status_sticky);
            assert_ne!(core.status, CANCELLED);
        }
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
    fn show_requests_are_dropped_during_a_deferred_quit() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = isolated(dir.path());
        core.menu(MenuCommand::ShowControls);
        assert_eq!(core.requests, vec![UiRequest::ShowControls]);
        // These requests arrived before Quit but have not reached a frame.
        core.request(UiRequest::ShowHelp);
        core.request(UiRequest::ShowAbout);
        core.request(UiRequest::ToggleTheme);
        core.quit();
        for command in [
            MenuCommand::ShowControls,
            MenuCommand::Help,
            MenuCommand::About,
            MenuCommand::CheckUpdates,
        ] {
            core.menu(command);
        }
        core.request(UiRequest::ShowControls); // the hotkey and tray path
        assert!(
            !core.requests.iter().any(|r| matches!(
                r,
                UiRequest::ShowControls | UiRequest::ShowHelp | UiRequest::ShowAbout
            )),
            "{:?}",
            core.requests
        );
        core.request(UiRequest::ToggleTheme);
        assert!(core.requests.contains(&UiRequest::ToggleTheme));
        core.poll(Instant::now());
        assert!(core.requests.contains(&UiRequest::Quit));
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

    #[test]
    fn update_results_arrive_through_poll_and_record_the_check_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = isolated(dir.path());
        core.check_updates();
        assert_eq!(core.update, UpdateState::Unchecked, "previews never check");
        let release = update::parse_latest_release(r#"{"tag_name":"v99.0.0"}"#).unwrap();
        let (tx, rx) = mpsc::channel();
        core.update_task = Some(rx);
        core.update = UpdateState::Checking;
        tx.send(Ok(Comparison::Available(release.clone()))).unwrap();
        let previous = UiState::capture(&core);
        core.poll(Instant::now());
        assert_eq!(core.update, UpdateState::Available(release));
        assert!(core.ui.last_update_check.is_some());
        assert!(core.update_task.is_none());
        assert!(previous.needs_repaint(&core), "a new result wakes the UI");

        let (tx, rx) = mpsc::channel::<Result<Comparison, String>>();
        core.update_task = Some(rx);
        core.ui.last_update_check = None;
        drop(tx);
        core.poll(Instant::now());
        assert!(matches!(core.update, UpdateState::Failed(_)));
        assert_eq!(core.ui.last_update_check, None, "failures do not count");
    }
}
