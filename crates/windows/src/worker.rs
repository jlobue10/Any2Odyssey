//! The renderer worker process: `Any2OdysseyWorker.exe --worker <event> <parentPid> <sourceHwnd>`.
//! It validates prerequisites, loads Samsung's engine, owns the capture
//! surface and the overlay controller, and reports status through
//! `worker-status.json` until the parent signals its stop event, exits, or
//! the engine closes.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use any2odyssey_core::engine::{ENGINE_CORE_HINT, ENGINE_READY_TIMEOUT_SECS};
use any2odyssey_core::frame_wake::DispatchDeadline;
use any2odyssey_core::ipc;
use any2odyssey_core::log::Logger;
use any2odyssey_core::paths;
use any2odyssey_core::prefs::{CaptureMode, Preferences};
use any2odyssey_core::status::{exit_code, WorkerStatus};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_TIMER,
};

use crate::engine::{self, EngineLibrary};
use crate::ipc::{Event, InstanceGuard};
use crate::mirror::{self, Mirror, TIMER_OVERLAY, TIMER_POLL, TIMER_UPDATE, WM_FRAME_ARRIVED};
use crate::overlay::OverlayController;
use crate::processes::{current_pid, is_pid_alive};
use crate::win;

fn report(state: &str) {
    let _ = any2odyssey_core::fsutil::write_json_atomic(
        &paths::worker_status_file(),
        &WorkerStatus::now(state, current_pid()),
    );
}

pub fn run(event_name: &str, parent_pid: u32, source_hwnd: isize) -> i32 {
    let logger = Arc::new(Logger::default_location());
    let log: Arc<dyn Fn(&str) + Send + Sync> = {
        let l = logger.clone();
        Arc::new(move |m: &str| l.log(m))
    };
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    match run_inner(event_name, parent_pid, source_hwnd, log.clone()) {
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
    }
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
    log: Arc<dyn Fn(&str) + Send + Sync>,
) -> Result<bool, Exit> {
    let prefs = Preferences::load();
    let Some(_guard) = InstanceGuard::acquire(ipc::RENDERER_MUTEX)
        .map_err(|e| fail(exit_code::RENDERER_ERROR, format!("Renderer error: {e}")))?
    else {
        report("Another companion renderer is running.");
        return Err(Exit {
            code: exit_code::ALREADY_RUNNING,
            message: None,
        });
    };
    if let Err(e) = engine::validate() {
        report(&e.to_string());
        return Err(Exit {
            code: exit_code::PREREQUISITES_FAILED,
            message: None,
        });
    }
    let library = EngineLibrary::load()
        .map_err(|e| fail(exit_code::RENDERER_ERROR, format!("Renderer error: {e}")))?;
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
    let (screen, device_name, _) = win::primary_monitor().ok_or_else(|| {
        fail(
            exit_code::RENDERER_ERROR,
            "Renderer error: no primary monitor",
        )
    })?;
    let mut surface = Mirror::new(
        &prefs,
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
    if let Err(e) = any2odyssey_core::overlay_settings::update_file(
        &paths::samsung_overlay_settings_file(),
        &paths::overlay_backup_file(),
        prefs.side_by_side,
        prefs.depth,
        prefs.popout,
    ) {
        log(&format!("Could not update Samsung overlay settings: {e}"));
    }
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
    library.start(surface.hwnd(), prefs.side_by_side, ENGINE_CORE_HINT);
    let mut started = true;
    let hwnd = surface.hwnd();
    mirror::set_timer(hwnd, TIMER_POLL, Mirror::poll_interval().as_millis() as u32);
    mirror::set_timer(hwnd, TIMER_OVERLAY, 100);

    let mut ready = false;
    let mut closing = false;
    let mut shutdown = |surface: &mut Mirror, started: &mut bool| {
        if closing {
            return;
        }
        closing = true;
        mirror::kill_timer(hwnd, TIMER_POLL);
        mirror::kill_timer(hwnd, TIMER_OVERLAY);
        mirror::kill_timer(hwnd, TIMER_UPDATE);
        if *started {
            library.stop();
            *started = false;
        }
        surface.close();
    };

    let mut overlay_deadline = DispatchDeadline::new(Instant::now(), Duration::from_millis(100));
    let mut poll_deadline = DispatchDeadline::new(Instant::now(), Mirror::poll_interval());
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        // Timers wake an idle loop; deadlines also run between posted frames
        // so sustained capture cannot starve stop, liveness or overlay checks.
        let now = Instant::now();
        if poll_deadline.due(now) {
            let alive = is_pid_alive(parent_pid);
            if stop_event.is_signaled() || !alive || engine::ENGINE_CLOSED.load(Ordering::Acquire) {
                shutdown(&mut surface, &mut started);
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
                shutdown(&mut surface, &mut started);
                continue;
            }
        }
        if overlay_deadline.due(now) {
            if let Err(e) = overlay.tick(restore_event.is_signaled()) {
                log(&format!("Overlay error: {e}"));
                report(&e);
                shutdown(&mut surface, &mut started);
                continue;
            }
        }
        if msg.hwnd == hwnd && msg.message == WM_TIMER {
            if msg.wParam.0 == TIMER_UPDATE {
                let result = surface.tick(overlay.capture_safe());
                if let Err(e) = result {
                    log(&format!("Desktop capture failed: {e}"));
                    report(&format!("Desktop capture error: {e}"));
                    shutdown(&mut surface, &mut started);
                }
            }
            continue;
        }
        if msg.hwnd == hwnd && msg.message == WM_FRAME_ARRIVED {
            if let Err(e) = surface.frame_arrived(overlay.capture_safe()) {
                log(&format!("Desktop capture failed: {e}"));
                report(&format!("Desktop capture error: {e}"));
                shutdown(&mut surface, &mut started);
            }
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    if started {
        library.stop();
    }
    Ok(ready)
}
