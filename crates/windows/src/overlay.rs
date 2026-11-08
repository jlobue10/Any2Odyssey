//! Keeps Samsung's renderer window (created inside our worker process)
//! click-through, non-activating, hidden from Alt+Tab, excluded from desktop
//! capture and, optionally, topmost — without ever stealing keyboard focus.

use std::time::{Duration, Instant};

use any2odyssey_core::ipc;
use windows::Win32::Foundation::HWND;

use crate::processes::current_pid;
use crate::taskbar;
use crate::win::{
    self, desired_overlay_ex_style, INPUT_SAFE_EX_STYLES, WS_EX_APPWINDOW_BIT, WS_EX_TOPMOST_BIT,
};

pub struct OverlayController {
    worker_pid: u32,
    parent_pid: u32,
    keep_on_top: bool,
    exclude_from_capture: bool,
    output: Option<HWND>,
    previous_focus: HWND,
    last_raise: Option<Instant>,
    verify_taskbar_at: Option<Instant>,
    configured: bool,
    pub log: Box<dyn Fn(&str)>,
}

impl OverlayController {
    pub fn new(
        parent_pid: u32,
        keep_on_top: bool,
        exclude_from_capture: bool,
        log: Box<dyn Fn(&str)>,
    ) -> OverlayController {
        OverlayController {
            worker_pid: current_pid(),
            parent_pid,
            keep_on_top,
            exclude_from_capture,
            output: None,
            previous_focus: HWND::default(),
            last_raise: None,
            verify_taskbar_at: None,
            configured: false,
            log,
        }
    }

    pub fn attached(&self) -> bool {
        self.configured && self.output.is_some_and(win::is_window)
    }

    /// Desktop capture may run only once the output is excluded from capture;
    /// otherwise the converted image would be captured again (feedback).
    pub fn capture_safe(&self) -> bool {
        self.attached() && self.output.is_some_and(win::is_capture_excluded)
    }

    fn find_output(&self) -> Option<HWND> {
        let mut found = None;
        win::enum_windows(|h| {
            if found.is_none()
                && win::process_id(h) == self.worker_pid
                && win::class_name(h) == ipc::ENGINE_OUTPUT_CLASS
                && ipc::is_output_title(&win::title(h))
            {
                found = Some(h);
            }
        });
        found
    }

    /// Called every 100 ms. `restore` forces the output back on top.
    pub fn tick(&mut self, restore: bool) -> Result<(), String> {
        if !self.output.is_some_and(win::is_window) {
            self.output = None;
            self.configured = false;
            self.output = self.find_output();
            let Some(_) = self.output else { return Ok(()) };
        }
        let output = self.output.expect("set above");
        let old = win::ex_style(output);
        let next = desired_overlay_ex_style(old);
        let changed = old != next;
        if changed {
            win::set_ex_style(output, next);
        }
        if !self.configured || changed {
            let _ = win::set_title(output, ipc::OUTPUT_WINDOW_TITLE);
            // Alpha 255 keeps the image intact. LAYERED + TRANSPARENT passes
            // real mouse input to other processes.
            win::set_layer_alpha(output, 255)
                .map_err(|e| format!("Could not make the 3D output click-through: {e}"))?;
            let actual = win::ex_style(output);
            if actual & INPUT_SAFE_EX_STYLES != INPUT_SAFE_EX_STYLES
                || actual & WS_EX_APPWINDOW_BIT != 0
            {
                return Err("The 3D output did not accept its input-safe styles.".into());
            }
            (self.log)(&format!("Renderer overlay configured: click-through, no activation, hidden from Alt+Tab; HWND {:?}", output.0));
            taskbar::remove_renderer_tab(output)?;
            (self.log)("Removed native renderer taskbar entry so Windows auto-hide can resume.");
            // Shell registration is asynchronous; verify once more shortly.
            self.verify_taskbar_at = Some(Instant::now() + Duration::from_secs(2));
            self.configured = true;
        }
        if self.exclude_from_capture && !self.capture_safe() {
            let ok = win::set_capture_excluded(output).is_ok() && self.capture_safe();
            if !ok {
                return Err("Windows could not exclude the 3D output from desktop capture; conversion stopped to prevent image feedback.".into());
            }
            (self.log)("3D output excluded from desktop capture to prevent feedback.");
        }
        if self
            .verify_taskbar_at
            .is_some_and(|at| Instant::now() >= at)
        {
            let _ = taskbar::remove_renderer_tab(output);
            self.verify_taskbar_at = None;
        }
        let focus = win::foreground();
        let focus_changed = focus != self.previous_focus;
        self.previous_focus = focus;
        let due = self
            .last_raise
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
        if restore
            || (self.keep_on_top
                && (changed
                    || focus_changed
                    || win::is_iconic(output)
                    || !win::is_visible(output)
                    || old & WS_EX_TOPMOST_BIT == 0
                    || due))
        {
            if win::is_iconic(output) || !win::is_visible(output) {
                win::show_window(output, win::SW_SHOWNOACTIVATE);
            }
            let flags = win::SWP_NOSIZE_NOMOVE_NOACTIVATE
                | win::SWP_SHOWWINDOW
                | if changed { win::SWP_FRAMECHANGED } else { 0 };
            win::set_window_pos(output, Some(win::hwnd_topmost()), None, flags)
                .map_err(|e| format!("Could not restore the 3D overlay: {e}"))?;
            // Keep the controls (parent process) reachable above the output,
            // without activating them.
            let parent = self.parent_pid;
            win::enum_windows(|h| {
                if win::process_id(h) == parent && win::is_visible(h) && !win::is_iconic(h) {
                    let _ = win::set_window_pos(
                        h,
                        Some(win::hwnd_topmost()),
                        None,
                        win::SWP_NOSIZE_NOMOVE_NOACTIVATE,
                    );
                }
            });
            self.last_raise = Some(Instant::now());
            if focus_changed || restore {
                (self.log)(&format!(
                    "Restored 3D overlay without activation; foreground HWND {:?}",
                    focus.0
                ));
            }
        }
        Ok(())
    }
}
