//! The renderer worker process: `Any2OdysseyWorker.exe --worker <event> <parentPid> <sourceHwnd> <preferencesJson>`.
//! It validates prerequisites, loads Samsung's engine, owns the capture
//! surface and the overlay controller, and reports status through
//! `worker-status.json` until the parent signals its stop event, exits, or
//! the engine closes.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use any2odyssey_core::engine::{ENGINE_CORE_HINT, ENGINE_READY_TIMEOUT_SECS};
use any2odyssey_core::frame_wake::DispatchDeadline;
use any2odyssey_core::ipc;
use any2odyssey_core::log::Logger;
use any2odyssey_core::paths;
use any2odyssey_core::prefs::{CaptureMode, Preferences};
use any2odyssey_core::status::{exit_code, WorkerStatus};
use windows::Win32::Foundation::{HWND, WAIT_EVENT, WAIT_OBJECT_0};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::{
    GetCurrentProcess, TerminateProcess, WaitForMultipleObjects, INFINITE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_TIMER,
};

use crate::engine::{self, EngineLibrary};
use crate::ipc::{Event, InstanceGuard};
use crate::mirror::{self, Mirror, TIMER_OVERLAY, TIMER_POLL, TIMER_UPDATE, WM_FRAME_ARRIVED};
use crate::overlay::OverlayController;
use crate::processes::{current_pid, ParentWatch};
use crate::win;

/// How long the controls wait for a signalled worker before killing it
/// (`Worker::stop` in the app crate). The in-process watchdog uses the same
/// grace: past it the controls would have killed us if they were still there,
/// so a worker alive after it was either orphaned or is stuck.
const WATCHDOG_GRACE: Duration = Duration::from_secs(7);

/// WM_QUIT ended the loop without any of its own stop paths having run.
const UNREQUESTED_QUIT_LOG: &str =
    "The message loop ended without a stop request; stopping the engine.";
const UNREQUESTED_QUIT_STATUS: &str =
    "Conversion stopped: the message loop ended without a stop request.";

/// The engine invoked its `closed` callback without being asked: the case
/// recovery exists for, so it exits with the renderer error code rather than
/// as a clean stop.
const ENGINE_CLOSED_LOG: &str = "The engine closed itself; stopping. See the conversion log.";
const ENGINE_CLOSED_STATUS: &str =
    "Conversion stopped: the engine closed itself. See conversion log.";

fn report(state: &str) {
    let _ = any2odyssey_core::fsutil::write_json_atomic(
        &paths::worker_status_file(),
        &WorkerStatus::now(state, current_pid()),
    );
}

/// Backstop for a message loop that cannot reach its own stop checks: an
/// engine `Stop` that never returns, a wndproc blocked inside the engine, or
/// a loop starved in some way the deadlines do not cover. The loop normally
/// notices the stop event or a dead parent within a poll interval and exits
/// on its own; this thread only acts when `finished` is still clear a full
/// grace period after either handle was signalled, so a slow but progressing
/// shutdown under the grace is never cut short. Without it, a click-through
/// topmost overlay would stay on screen with nothing left to remove it.
fn spawn_watchdog(
    parent: ParentWatch,
    stop: Event,
    finished: Arc<AtomicBool>,
    log: Arc<dyn Fn(&str) + Send + Sync>,
) {
    let thread_log = log.clone();
    let spawned = std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || {
            let handles = [parent.handle(), stop.handle()];
            // SAFETY: the handles are owned by this closure for its whole life
            // and carry SYNCHRONIZE access. The stop event is manual-reset, so
            // satisfying this wait does not consume the signal the message
            // loop polls for.
            let woken = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
            if woken != WAIT_OBJECT_0 && woken != WAIT_EVENT(WAIT_OBJECT_0.0 + 1) {
                // WAIT_FAILED: nothing to wait on; the loop's own polls stay
                // in charge, but say so, or a dead backstop leaves no trace.
                thread_log("Watchdog: wait failed; the message loop's own checks remain the only stop path.");
                return;
            }
            // The wait reports the lowest signalled index, so controls that
            // asked for a stop and then quit would read as "went away". Ask
            // the event itself: a requested stop is the more specific cause.
            let reason = if stop.is_signaled() {
                "the stop was requested"
            } else {
                "the controls went away"
            };
            std::thread::sleep(WATCHDOG_GRACE);
            if finished.load(Ordering::Acquire) {
                return;
            }
            thread_log(&format!(
                "Watchdog: the message loop did not stop within {} s after {reason}; exiting.",
                WATCHDOG_GRACE.as_secs()
            ));
            // Not `std::process::exit`: ExitProcess runs every DLL's detach
            // under the loader lock, and the thread we are rescuing may be
            // blocked inside the engine holding exactly what its DllMain
            // needs. TerminateProcess skips all of that; the kernel destroys
            // the overlay window with the process either way.
            // SAFETY: terminating our own process with a plain exit code.
            let _ = unsafe { TerminateProcess(GetCurrentProcess(), exit_code::RENDERER_ERROR as u32) };
        });
    if let Err(e) = spawned {
        log(&format!("Watchdog thread not started: {e}"));
    }
}

pub fn run(event_name: &str, parent_pid: u32, source_hwnd: isize, prefs: Preferences) -> i32 {
    let logger = Arc::new(Logger::default_location());
    let log: Arc<dyn Fn(&str) + Send + Sync> = {
        let l = logger.clone();
        Arc::new(move |m: &str| l.log(m))
    };
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // Set once `run_inner` has returned on any path: only then have the loop,
    // the engine's Stop and the surface teardown all completed. The watchdog
    // stands down so the final log and status lines below are written and
    // the real exit code, not its generic one, reaches the controls.
    let finished = Arc::new(AtomicBool::new(false));
    let result = run_inner(
        event_name,
        parent_pid,
        source_hwnd,
        &prefs,
        log.clone(),
        finished.clone(),
    );
    finished.store(true, Ordering::Release);
    let code = match result {
        Ok(ready) => {
            log("Renderer stopped cleanly.");
            if ready {
                exit_code::READY_AND_STOPPED_CLEANLY
            } else {
                exit_code::NEVER_BECAME_READY
            }
        }
        Err(Exit { code, message }) => {
            if let Some(m) = message {
                log(&m);
                report(&m);
            }
            code
        }
    };
    // End the process here rather than returning into `std::process::exit`.
    // ExitProcess would run DLL_PROCESS_DETACH for the vendor engine and its
    // CUDA/ONNX/LeiaSR dependencies under the loader lock, after killing the
    // engine's remaining threads mid-flight, with the watchdog already stood
    // down: a detach that blocks would leave an invisible process holding the
    // renderer mutex, so every later start reports "another renderer is
    // running". Nothing is lost by skipping it: stdio is null, the status
    // file and log lines above are complete, and the engine is never
    // FreeLibrary'd anyway.
    // SAFETY: terminating our own process with the exit code computed above.
    let _ = unsafe { TerminateProcess(GetCurrentProcess(), code as u32) };
    code
}

struct Exit {
    code: i32,
    message: Option<String>,
}

fn fail(code: i32, message: impl Into<String>) -> Exit {
    Exit {
        code,
        message: Some(message.into()),
    }
}

fn run_inner(
    event_name: &str,
    parent_pid: u32,
    source_hwnd: isize,
    prefs: &Preferences,
    log: Arc<dyn Fn(&str) + Send + Sync>,
    finished: Arc<AtomicBool>,
) -> Result<bool, Exit> {
    let Some(_guard) = InstanceGuard::acquire(ipc::RENDERER_MUTEX)
        .map_err(|e| fail(exit_code::RENDERER_ERROR, format!("Renderer error: {e}")))?
    else {
        report("Another companion renderer is running.");
        log("Another companion renderer is running; this one exits.");
        return Err(Exit {
            code: exit_code::ALREADY_RUNNING,
            message: None,
        });
    };
    if let Err(e) = engine::validate() {
        report(&e.to_string());
        log(&format!("Prerequisite check failed: {e}"));
        return Err(Exit {
            code: exit_code::PREREQUISITES_FAILED,
            message: None,
        });
    }
    let stop_event = Event::open(event_name).map_err(|e| {
        fail(
            exit_code::RENDERER_ERROR,
            format!("Renderer error: stop event: {e}"),
        )
    })?;
    let restore_event =
        Event::open(&format!("{event_name}{}", ipc::RESTORE_SUFFIX)).map_err(|e| {
            fail(
                exit_code::RENDERER_ERROR,
                format!("Renderer error: restore event: {e}"),
            )
        })?;
    // Hold the parent open for the whole run: a held handle pins the process
    // object, so a recycled PID can never be mistaken for a living parent.
    let parent = ParentWatch::open(parent_pid).ok_or_else(|| {
        fail(
            exit_code::RENDERER_ERROR,
            "Renderer error: the controls process is not running.",
        )
    })?;
    // A toggle that was undone, or a parent that quit, while prerequisites
    // ran: nothing is on screen yet, so leave without touching Samsung's
    // settings file or loading the engine and its dependency tree, which
    // takes seconds the controls' 7-second stop clock is already counting.
    let stopped_early = || stop_event.is_signaled() || !parent.is_alive();
    if stopped_early() {
        log("Stop requested during the prerequisite checks; the engine was not loaded.");
        return Ok(false);
    }
    // The engine reads depth/pop-out and output flags from Samsung's file.
    // Do not load or start it with stale values after a failed Apply.
    any2odyssey_core::overlay_settings::update_file(
        &paths::samsung_overlay_settings_file(),
        &paths::overlay_backup_file(),
        prefs.side_by_side,
        prefs.depth,
        prefs.popout,
    )
    .map_err(|e| {
        fail(
            exit_code::RENDERER_ERROR,
            format!("Could not update Samsung overlay settings; conversion was not started: {e}"),
        )
    })?;
    // Duplicates, so the watchdog's wait never touches a handle this function
    // is about to close. A missing backstop is logged, not fatal: the loop's
    // own checks still cover every ordinary stop.
    match (parent.try_clone(), stop_event.try_clone()) {
        (Ok(parent), Ok(stop)) => spawn_watchdog(parent, stop, finished, log.clone()),
        (Err(e), _) | (_, Err(e)) => log(&format!("Watchdog not started: {e}")),
    }
    let library = EngineLibrary::load()
        .map_err(|e| fail(exit_code::RENDERER_ERROR, format!("Renderer error: {e}")))?;
    let (screen, device_name, _) = win::primary_monitor().ok_or_else(|| {
        fail(
            exit_code::RENDERER_ERROR,
            "Renderer error: no primary monitor",
        )
    })?;
    // Loading the engine can take seconds; a stop that arrived meanwhile must
    // not create the capture.
    if stopped_early() {
        log("Stop requested before the capture was created; the engine was not started.");
        return Ok(false);
    }
    let mut surface = Mirror::new(
        prefs,
        parent_pid,
        HWND(source_hwnd as *mut _),
        screen,
        &device_name,
        log.clone(),
    )
    .map_err(|e| fail(exit_code::RENDERER_ERROR, format!("Renderer error: {e}")))?;
    if let Some(border) = surface.capture().and_then(|c| c.border.take()) {
        let _ = any2odyssey_core::fsutil::write_json_atomic(
            &paths::capture_border_status_file(),
            &border.status(current_pid()),
        );
    }
    let overlay_log = log.clone();
    let mut overlay = OverlayController::new(
        parent_pid,
        prefs.keep_overlay_on_top,
        prefs.mode == CaptureMode::Desktop,
        Box::new(move |m| overlay_log(m)),
    );

    report("Starting conversion");
    surface.show(overlay.capture_safe()).map_err(|e| {
        fail(
            exit_code::RENDERER_ERROR,
            format!("Desktop capture error: {e}"),
        )
    })?;
    let begun = Instant::now();
    let mut readiness = any2odyssey_core::log_readiness::LogReadiness::snapshot(
        &paths::samsung_conversion_log_file(),
        any2odyssey_core::engine::ENGINE_READY_MARKER,
    )
    .map_err(|e| {
        fail(
            exit_code::RENDERER_ERROR,
            format!("Cannot snapshot conversion log: {e}"),
        )
    })?;
    // Building the capture can take seconds (the capture-border broker waits
    // up to 10 s); a stop that arrived meanwhile must not start the model.
    if stopped_early() {
        log("Stop requested before the engine started; leaving without it.");
        return Ok(false);
    }
    library.start(surface.hwnd(), prefs.side_by_side, ENGINE_CORE_HINT);
    let mut started = true;
    let hwnd = surface.hwnd();
    // An idle loop is woken only by these timers; without them the stop event
    // and the parent's liveness would go unevaluated between frames.
    if !mirror::set_timer(hwnd, TIMER_POLL, Mirror::poll_interval().as_millis() as u32)
        || !mirror::set_timer(hwnd, TIMER_OVERLAY, 100)
    {
        library.stop(&stop_event);
        return Err(fail(
            exit_code::RENDERER_ERROR,
            "Renderer error: could not start the poll timer",
        ));
    }

    let mut ready = false;
    // Shared, not captured by unique borrow, so the loop can read them while
    // the shutdown closure is alive. `failure` holds the exit code of the
    // first error that stopped the loop; later errors are already ignored.
    let closing = Cell::new(false);
    let failure: Cell<Option<i32>> = Cell::new(None);
    let shutdown = |surface: &mut Mirror, started: &mut bool, code: Option<i32>| {
        if closing.replace(true) {
            return;
        }
        failure.set(code);
        mirror::kill_timer(hwnd, TIMER_POLL);
        mirror::kill_timer(hwnd, TIMER_OVERLAY);
        mirror::kill_timer(hwnd, TIMER_UPDATE);
        if *started {
            library.stop(&stop_event);
            *started = false;
        }
        surface.close();
    };

    let mut overlay_deadline = DispatchDeadline::new(Instant::now(), Duration::from_millis(100));
    let mut poll_deadline = DispatchDeadline::new(Instant::now(), Mirror::poll_interval());
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        if closing.get() {
            // The stop reason is already in worker-status.json and the
            // surface is destroyed. Only pump the engine's own windows until
            // WM_QUIT; a late overlay or capture error must not overwrite it.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            continue;
        }
        // Timers wake an idle loop; deadlines also run between posted frames
        // so sustained capture cannot starve stop, liveness or overlay checks.
        let now = Instant::now();
        if poll_deadline.due(now) {
            // Three triggers end the loop here; only the first is a clean,
            // requested stop. Name each in the log so companion.log tells a
            // user stop from the engine giving up or the controls vanishing.
            if stop_event.is_signaled() {
                log("Stop requested; stopping the engine.");
                shutdown(&mut surface, &mut started, None);
                continue;
            }
            if !parent.is_alive() {
                // Nobody is left to read the code, but the log and status
                // file should not call an orphaned stop clean.
                log("The controls process ended; stopping the engine.");
                report("Conversion stopped: the controls process ended.");
                shutdown(&mut surface, &mut started, Some(exit_code::RENDERER_ERROR));
                continue;
            }
            if engine::ENGINE_CLOSED.load(Ordering::Acquire) {
                log(ENGINE_CLOSED_LOG);
                report(ENGINE_CLOSED_STATUS);
                shutdown(&mut surface, &mut started, Some(exit_code::RENDERER_ERROR));
                continue;
            }
            if !ready && overlay.attached() && readiness.poll().unwrap_or(false) {
                ready = true;
                report(&format!(
                    "3D active — click-through overlay; {}",
                    if prefs.keep_overlay_on_top {
                        "stays above apps"
                    } else {
                        "Ctrl+Alt+F8 restores it"
                    }
                ));
            }
            if !ready && begun.elapsed() > Duration::from_secs(ENGINE_READY_TIMEOUT_SECS) {
                report("Conversion did not initialize. See conversion log.");
                log(&format!(
                    "Conversion did not initialize within {ENGINE_READY_TIMEOUT_SECS} s; stopping the engine."
                ));
                // Same code a pre-ready stop produced before; the supervisor
                // treats every exit alike, only the log line distinguishes it.
                shutdown(
                    &mut surface,
                    &mut started,
                    Some(exit_code::NEVER_BECAME_READY),
                );
                continue;
            }
        }
        if overlay_deadline.due(now) {
            // The engine may close between two polls; re-showing its window
            // now would race its own teardown.
            if engine::ENGINE_CLOSED.load(Ordering::Acquire) {
                log(ENGINE_CLOSED_LOG);
                report(ENGINE_CLOSED_STATUS);
                shutdown(&mut surface, &mut started, Some(exit_code::RENDERER_ERROR));
                continue;
            }
            if let Err(e) = overlay.tick(restore_event.is_signaled()) {
                log(&format!("Overlay error: {e}"));
                report(&e);
                shutdown(&mut surface, &mut started, Some(exit_code::RENDERER_ERROR));
                continue;
            }
        }
        if msg.hwnd == hwnd && msg.message == WM_TIMER {
            if msg.wParam.0 == TIMER_UPDATE {
                let result = surface.tick(overlay.capture_safe());
                if let Err(e) = result {
                    log(&format!("Desktop capture failed: {e}"));
                    report(&format!("Desktop capture error: {e}"));
                    shutdown(&mut surface, &mut started, Some(exit_code::RENDERER_ERROR));
                }
            }
            continue;
        }
        if msg.hwnd == hwnd && msg.message == WM_FRAME_ARRIVED {
            if let Err(e) = surface.frame_arrived(overlay.capture_safe()) {
                log(&format!("Desktop capture failed: {e}"));
                report(&format!("Desktop capture error: {e}"));
                shutdown(&mut surface, &mut started, Some(exit_code::RENDERER_ERROR));
            }
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    if !closing.get() {
        // WM_QUIT reached the loop although nothing here asked for it: the
        // capture surface was destroyed from outside, or code running on this
        // thread posted a quit. Not a clean stop, and the status file still
        // says whatever the last poll wrote; name it before the engine's Stop,
        // which the watchdog may have to cut short.
        log(UNREQUESTED_QUIT_LOG);
        report(UNREQUESTED_QUIT_STATUS);
        if started {
            library.stop(&stop_event);
        }
        return Err(Exit {
            code: exit_code::RENDERER_ERROR,
            message: None,
        });
    }
    if let Some(code) = failure.get() {
        // The reason is already in worker-status.json and the log; a message
        // here would overwrite it with something less specific.
        return Err(Exit {
            code,
            message: None,
        });
    }
    Ok(ready)
}
