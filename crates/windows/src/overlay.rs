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

/// With Keep 3D above apps (the default) every foreground change raises the
/// output. One log line each would fill `companion.log`, which is only moved
/// aside when a process starts, with every Alt+Tab and click on another window; these raises are
/// summarised at most this often. A requested restore is always logged.
const RAISE_LOG_INTERVAL: Duration = Duration::from_secs(60);

pub struct OverlayController {
    worker_pid: u32,
    parent_pid: u32,
    keep_on_top: bool,
    exclude_from_capture: bool,
    output: Option<HWND>,
    previous_focus: HWND,
    last_raise: Option<Instant>,
    /// When a foreground-change raise was last logged, and how many have
    /// happened since.
    raise_logged: Option<Instant>,
    raises_unlogged: u32,
    verify_taskbar_at: Option<Instant>,
    /// A requested restore that has not been carried out yet. The request
    /// arrives once (an auto-reset event), possibly before Samsung's output
    /// window exists.
    restore_pending: bool,
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
            raise_logged: None,
            raises_unlogged: 0,
            verify_taskbar_at: None,
            restore_pending: false,
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
                // Not `win::title`: that would send a message to the engine's
                // window thread and wait while it is busy loading the model.
                && ipc::is_output_title(&win::stored_title(h))
            {
                found = Some(h);
            }
        });
        found
    }

    /// Called every 100 ms. `restore` forces the output back on top. The
    /// caller reads it from an auto-reset event, so it is true for one tick
    /// only, and it is kept here until a raise has been carried out: a
    /// request made while the engine was still starting, before its output
    /// window existed, was dropped at the early return below.
    pub fn tick(&mut self, restore: bool) -> Result<(), String> {
        self.restore_pending |= restore;
        let restore = self.restore_pending;
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
            // Win32 applies a GWL_EXSTYLE change only once SetWindowPos has
            // been told the frame changed; this must not depend on whether
            // the keep-on-top raise below runs. The styles are verified
            // afterwards, so a refused notification needs no handling here.
            let _ = win::set_window_pos(
                output,
                None,
                None,
                win::SWP_NOSIZE_NOMOVE_NOACTIVATE | win::SWP_NOZORDER | win::SWP_FRAMECHANGED,
            );
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
            // Cosmetic: a missing or restarting shell must not stop conversion.
            // The input-safety checks above remain fatal.
            match taskbar::remove_renderer_tab(output) {
                Ok(()) => (self.log)(
                    "Removed native renderer taskbar entry so Windows auto-hide can resume.",
                ),
                Err(e) => (self.log)(&format!(
                    "Could not remove the renderer taskbar entry (retrying once): {e}"
                )),
            }
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
            // The windows of the controls (parent process) that must stay
            // reachable above the output. Looking them up sends nothing.
            let parent = self.parent_pid;
            let mut controls = Vec::new();
            win::enum_windows(|h| {
                if win::process_id(h) == parent && win::is_visible(h) && !win::is_iconic(h) {
                    controls.push(h);
                }
            });
            // The output belongs to the engine's thread and the controls to
            // another process. A plain SetWindowPos or ShowWindow is sent to
            // the owning thread and waits for it: this runs on the capture
            // thread at least once a second, so every wait is a late frame,
            // and a controls window that stopped answering would freeze
            // capture with the watchdog unarmed.
            //
            // With no controls window on screen (hidden in the tray, the
            // usual state) nothing has to be ordered against the output, so
            // its raise is posted. With one on screen the order matters:
            // the output must be raised first and the controls after it,
            // and two posted requests are processed by two threads in
            // either order. The output's raise is then sent as before, and
            // only the controls' raise, which follows it, is posted.
            let posted = if controls.is_empty() {
                win::SWP_ASYNCWINDOWPOS
            } else {
                0
            };
            if win::is_iconic(output) || !win::is_visible(output) {
                if controls.is_empty() {
                    win::show_window_async(output, win::SW_SHOWNOACTIVATE);
                } else {
                    win::show_window(output, win::SW_SHOWNOACTIVATE);
                }
            }
            let flags = win::SWP_NOSIZE_NOMOVE_NOACTIVATE | win::SWP_SHOWWINDOW | posted;
            win::set_window_pos(output, Some(win::hwnd_topmost()), None, flags)
                .map_err(|e| format!("Could not restore the 3D overlay: {e}"))?;
            // Above the output, without activating them.
            for h in controls {
                let _ = win::set_window_pos(
                    h,
                    Some(win::hwnd_topmost()),
                    None,
                    win::SWP_NOSIZE_NOMOVE_NOACTIVATE | win::SWP_ASYNCWINDOWPOS,
                );
            }
            self.last_raise = Some(Instant::now());
            self.restore_pending = false;
            if restore {
                (self.log)(&format!(
                    "Restored 3D overlay without activation; foreground HWND {:?}",
                    focus.0
                ));
            } else if focus_changed {
                self.raises_unlogged = self.raises_unlogged.saturating_add(1);
                if self
                    .raise_logged
                    .is_none_or(|t| t.elapsed() >= RAISE_LOG_INTERVAL)
                {
                    (self.log)(&format!(
                        "Raised 3D overlay without activation after a foreground change ({} since the last such line); foreground HWND {:?}",
                        self.raises_unlogged, focus.0
                    ));
                    self.raise_logged = Some(Instant::now());
                    self.raises_unlogged = 0;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW,
        WS_POPUP,
    };

    /// Run `test` on a thread whose windows are on a desktop of their own,
    /// which is never shown: the raise under test shows its window, topmost,
    /// and that belongs on nobody's screen. Where Windows gives no such
    /// desktop the test runs on the one it was started on.
    fn on_a_desktop_of_its_own(test: impl FnOnce() + Send) {
        use std::ptr::null;
        #[link(name = "user32")]
        unsafe extern "system" {
            fn CreateDesktopW(
                name: *const u16,
                device: *const u16,
                mode: *const std::ffi::c_void,
                flags: u32,
                access: u32,
                security: *const std::ffi::c_void,
            ) -> isize;
            fn SetThreadDesktop(desktop: isize) -> i32;
            fn CloseDesktop(desktop: isize) -> i32;
        }
        const GENERIC_ALL: u32 = 0x1000_0000;
        let name = win::wide(&format!("Any2Odyssey-overlay-test-{}", current_pid()));
        // SAFETY: a NUL-terminated name that outlives the call; the other
        // pointers are optional and absent.
        let desktop =
            unsafe { CreateDesktopW(name.as_ptr(), null(), null(), 0, GENERIC_ALL, null()) };
        let outcome = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    if desktop != 0 {
                        // SAFETY: a new thread, which has no window yet.
                        let _ = unsafe { SetThreadDesktop(desktop) };
                    }
                    test()
                })
                .join()
        });
        if desktop != 0 {
            // SAFETY: the handle made above; its one thread has ended.
            let _ = unsafe { CloseDesktop(desktop) };
        }
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    /// Ctrl+Alt+F8 while the engine is still starting. The request is an
    /// auto-reset event, read once; with no output window yet it was dropped
    /// at the early return, under a log line that it had been asked for.
    #[test]
    fn a_restore_asked_for_before_the_output_window_exists_is_carried_out_when_it_comes() {
        on_a_desktop_of_its_own(|| {
            let lines: Rc<RefCell<Vec<String>>> = Rc::default();
            let sink = lines.clone();
            // Keep 3D above apps off: nothing but a requested restore raises.
            let mut controller = OverlayController::new(
                0,
                false,
                false,
                Box::new(move |line| sink.borrow_mut().push(line.to_owned())),
            );
            let restores = || {
                lines
                    .borrow()
                    .iter()
                    .filter(|line| line.starts_with("Restored 3D overlay without activation"))
                    .count()
            };
            controller.tick(true).unwrap();
            controller.tick(false).unwrap();
            assert!(!controller.attached() && restores() == 0);

            // The 3D picture comes: a stand-in for the engine's window, of
            // its class and with its title, in this process.
            let class_name = win::wide(ipc::ENGINE_OUTPUT_CLASS);
            let title = win::wide(ipc::ENGINE_OUTPUT_TITLE);
            let class = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            assert_ne!(unsafe { RegisterClassW(&class) }, 0);
            let window = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    PCWSTR(class_name.as_ptr()),
                    PCWSTR(title.as_ptr()),
                    WS_POPUP,
                    0,
                    0,
                    64,
                    64,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap();

            controller.tick(false).unwrap();
            assert!(controller.attached());
            assert_eq!(restores(), 1, "{:?}", lines.borrow());
            // The request is spent: the next tick does not restore again.
            controller.tick(false).unwrap();
            assert_eq!(restores(), 1, "{:?}", lines.borrow());
            // With the window there a request is carried out at once.
            controller.tick(true).unwrap();
            assert_eq!(restores(), 2, "{:?}", lines.borrow());
            unsafe { DestroyWindow(window) }.unwrap();
        });
    }
}
