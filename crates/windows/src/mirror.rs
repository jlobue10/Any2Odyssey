//! The capture surface the Samsung engine is pointed at.
//!
//! In whole-desktop mode it is a borderless window placed outside every
//! monitor whose swap chain receives the desktop capture. In window / active
//! modes it sits on the primary screen (beneath the topmost 3D output) and
//! shows a DWM thumbnail of the chosen window, fitted and centred.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use any2odyssey_core::frame_wake::FrameWake;
use any2odyssey_core::geometry::{fit, offscreen_bounds, Rect};
use any2odyssey_core::prefs::{CaptureMode, Preferences};
use any2odyssey_core::status::DesktopCaptureStatus;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    COLORREF, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, ValidateRect, HBRUSH, HDC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, KillTimer,
    PostMessageW, PostQuitMessage, RegisterClassExW, SetTimer, SetWindowLongPtrW, GWLP_USERDATA,
    HWND_BOTTOM, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE,
    WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WNDCLASSEXW, WS_POPUP,
};

use crate::capture::GpuDesktopCapture;
use crate::processes::current_pid;
use crate::win::{self, wide};

/// `WM_APP + 1`: a capture frame arrived (posted from the capture thread).
pub const WM_FRAME_ARRIVED: u32 = 0x8001;
pub const TIMER_UPDATE: usize = 1;
pub const TIMER_POLL: usize = 2;
pub const TIMER_OVERLAY: usize = 3;

const CLASS_NAME: &str = "Any2OdysseySurface";
/// `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT`.
const SURFACE_EX_STYLE: u32 = 0x0800_0000 | 0x80 | 0x20;
const HTTRANSPARENT: isize = -1;
const MA_NOACTIVATE: isize = 3;

/// How long a passed off-screen check of the capture surface stands. The
/// check enumerates the monitors; once per frame it ran 60 times a second
/// to guard something that changes with the display arrangement.
const OFFSCREEN_RECHECK: Duration = Duration::from_millis(100);

/// Set by the window procedure when Windows announces a display change, so
/// the next frame checks the surface again without waiting out
/// [`OFFSCREEN_RECHECK`].
static DISPLAY_CHANGED: AtomicBool = AtomicBool::new(false);

/// Whether the surface must be checked against the monitors before the next
/// frame is presented: never checked, a display change was announced, or
/// the last check is older than [`OFFSCREEN_RECHECK`].
fn offscreen_check_due(checked: Option<Instant>, now: Instant, display_changed: bool) -> bool {
    display_changed
        || checked.is_none_or(|at| now.saturating_duration_since(at) >= OFFSCREEN_RECHECK)
}

/// Window procedure shared by the surface and the diagnostic test windows.
/// `GWLP_USERDATA` holds `0xFFFFFFFF` for "do not paint" or a COLORREF to fill.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE),
        WM_ERASEBKGND | WM_PAINT => {
            let data = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as u32;
            if data == u32::MAX {
                if msg == WM_PAINT {
                    let _ = unsafe { ValidateRect(Some(hwnd), None) };
                }
                return LRESULT(1);
            }
            if msg == WM_ERASEBKGND {
                let dc = HDC(w.0 as *mut _);
                let brush: HBRUSH = unsafe { CreateSolidBrush(COLORREF(data)) };
                let mut rc = windows::Win32::Foundation::RECT::default();
                let _ = unsafe { GetClientRect(hwnd, &mut rc) };
                unsafe { FillRect(dc, &rc, brush) };
                let _ = unsafe { DeleteObject(brush.into()) };
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        // These windows are destroyed only by the code that created them. The
        // default handling would let any process that posts WM_CLOSE (taskkill
        // without /F, a window-management tool) destroy the capture surface
        // under a running engine and end the worker's loop through WM_QUIT.
        WM_CLOSE => LRESULT(0),
        WM_DISPLAYCHANGE => {
            DISPLAY_CHANGED.store(true, Ordering::Release);
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn register_class() -> windows::core::Result<()> {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.load(Ordering::Acquire) {
        return Ok(());
    }
    let class = wide(CLASS_NAME);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: unsafe { GetModuleHandleW(None)? }.into(),
        lpszClassName: PCWSTR(class.as_ptr()),
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&wc) } == 0 {
        let error = windows::core::Error::from_thread();
        // Another caller registered it first; the class exists either way.
        if error.code() != ERROR_CLASS_ALREADY_EXISTS.to_hresult() {
            // Leave DONE clear so the next window creation retries instead of
            // failing later with a confusing "class not found".
            return Err(error);
        }
    }
    DONE.store(true, Ordering::Release);
    Ok(())
}

pub fn colorref(rgb: [u8; 3]) -> u32 {
    rgb[0] as u32 | (rgb[1] as u32) << 8 | (rgb[2] as u32) << 16
}

/// Create a hidden popup window of the surface class. `fill` is the solid
/// background colour, or `None` to leave painting to a swap chain.
pub fn create_plain_window(
    title: &str,
    bounds: Rect,
    fill: Option<[u8; 3]>,
    topmost: bool,
) -> windows::core::Result<HWND> {
    register_class()?;
    let class = wide(CLASS_NAME);
    let title = wide(title);
    let ex = SURFACE_EX_STYLE | if topmost { 0x8 } else { 0 };
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(WS_POPUP.0),
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            None,
            None,
            Some(GetModuleHandleW(None)?.into()),
            None,
        )?
    };
    let data = fill.map_or(u32::MAX, colorref) as isize;
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, data) };
    Ok(hwnd)
}

/// Start (or re-arm) a window timer; false when Windows refused it.
pub fn set_timer(hwnd: HWND, id: usize, millis: u32) -> bool {
    unsafe { SetTimer(Some(hwnd), id, millis, None) != 0 }
}
pub fn kill_timer(hwnd: HWND, id: usize) {
    let _ = unsafe { KillTimer(Some(hwnd), id) };
}

pub struct Mirror {
    hwnd: HWND,
    mode: CaptureMode,
    screen: Rect,
    parent_pid: u32,
    selected: HWND,
    last_active: HWND,
    thumbnail: Option<(HWND, win::Thumbnail)>,
    thumbnail_size: Option<(i32, i32)>,
    desktop: Option<GpuDesktopCapture>,
    frame_wake: Arc<FrameWake>,
    /// When the surface was last found clear of every monitor.
    offscreen_checked: Option<Instant>,
    sampled: Instant,
    sample_frames: u64,
    capture_millis: f64,
    /// Window modes: whether a source is on the surface. `None` until the
    /// first evaluation, so a start without a source is reported as well.
    source_shown: Option<bool>,
    /// The latest change of `source_shown` the owner has not collected yet.
    source_change: Option<bool>,
    pub update_interval_ms: u32,
    log: Arc<dyn Fn(&str) + Send + Sync>,
}

impl Mirror {
    pub fn new(
        prefs: &Preferences,
        parent_pid: u32,
        selected: HWND,
        screen: Rect,
        device_name: &str,
        log: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Result<Mirror, String> {
        let desktop_mode = prefs.mode == CaptureMode::Desktop;
        let bounds = if desktop_mode {
            offscreen_bounds(screen.size(), &win::monitor_rects())
        } else {
            screen
        };
        let fill = if desktop_mode {
            None
        } else {
            Some([20, 24, 32])
        };
        let hwnd = create_plain_window(
            any2odyssey_core::ipc::CAPTURE_SURFACE_TITLE,
            bounds,
            fill,
            false,
        )
        .map_err(|e| format!("Capture surface window: {e}"))?;
        let mut mirror = Mirror {
            hwnd,
            mode: prefs.mode,
            screen,
            parent_pid,
            selected,
            last_active: HWND::default(),
            thumbnail: None,
            thumbnail_size: None,
            desktop: None,
            frame_wake: Arc::new(FrameWake::default()),
            offscreen_checked: None,
            sampled: Instant::now(),
            sample_frames: 0,
            capture_millis: 0.0,
            source_shown: None,
            source_change: None,
            update_interval_ms: if desktop_mode { 100 } else { 250 },
            log,
        };
        if desktop_mode {
            let target = hwnd.0 as isize;
            let wake = mirror.frame_wake.clone();
            let on_frame = move || {
                wake.notify(|| unsafe {
                    PostMessageW(
                        Some(HWND(target as *mut _)),
                        WM_FRAME_ARRIVED,
                        WPARAM(0),
                        LPARAM(0),
                    )
                    .is_ok()
                });
            };
            let log = mirror.log.clone();
            let capture = GpuDesktopCapture::new(screen, device_name, on_frame, &|m| log(m))
                .inspect_err(|_| {
                    let _ = unsafe { DestroyWindow(hwnd) };
                })?;
            mirror.desktop = Some(capture);
        }
        Ok(mirror)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
    pub fn capture(&mut self) -> Option<&mut GpuDesktopCapture> {
        self.desktop.as_mut()
    }

    /// Show without activation at the bottom of the Z order and start updates.
    pub fn show(&mut self, can_capture: bool) -> Result<(), String> {
        win::show_window(self.hwnd, win::SW_SHOWNOACTIVATE);
        let _ = win::set_window_pos(
            self.hwnd,
            Some(HWND_BOTTOM),
            None,
            win::SWP_NOSIZE_NOMOVE_NOACTIVATE,
        );
        if self.desktop.is_some() {
            self.assert_offscreen()?;
            if can_capture {
                self.capture_desktop()?;
            }
        } else {
            self.refresh_sources();
        }
        if !set_timer(self.hwnd, TIMER_UPDATE, self.update_interval_ms) {
            return Err("Could not start the capture update timer.".into());
        }
        Ok(())
    }

    fn assert_offscreen(&self) -> Result<(), String> {
        let Some(actual) = win::window_rect(self.hwnd) else {
            return Err("Capture surface has no bounds.".into());
        };
        if win::intersects_any_monitor(actual)? {
            return Err(
                "Capture surface overlaps a monitor. Conversion stopped to prevent feedback."
                    .into(),
            );
        }
        Ok(())
    }

    /// Timer tick: desktop mode presents a frame when allowed; window modes
    /// re-evaluate the source window.
    pub fn tick(&mut self, can_capture: bool) -> Result<(), String> {
        if self.desktop.is_some() {
            if can_capture {
                self.capture_desktop()?;
            }
        } else {
            self.refresh_sources();
        }
        Ok(())
    }

    /// A frame-arrived message: present it (desktop mode only).
    pub fn frame_arrived(&mut self, can_capture: bool) -> Result<(), String> {
        self.frame_wake.acknowledge();
        if self.desktop.is_some() && can_capture {
            self.capture_desktop()?;
        }
        Ok(())
    }

    fn capture_desktop(&mut self) -> Result<(), String> {
        // Swap first: a change announced during the check is kept for the
        // next frame instead of being cleared unseen.
        let display_changed = DISPLAY_CHANGED.swap(false, Ordering::AcqRel);
        let now = Instant::now();
        if offscreen_check_due(self.offscreen_checked, now, display_changed) {
            self.assert_offscreen()?;
            self.offscreen_checked = Some(now);
        }
        let started = Instant::now();
        let hwnd = self.hwnd;
        let capture = self.desktop.as_mut().expect("desktop mode");
        // The average is per presented frame; calls that found no new frame
        // (and the once-a-second display queries they carry) are not frames.
        if capture.present(hwnd)? {
            self.capture_millis += started.elapsed().as_secs_f64() * 1000.0;
        }
        let frames = capture.frames;
        let seconds = self.sampled.elapsed().as_secs_f64();
        if seconds >= 10.0 {
            let count = frames - self.sample_frames;
            let status = DesktopCaptureStatus {
                pid: current_pid(),
                time: any2odyssey_core::log::now_iso(),
                backend: "WGC GPU, cursor excluded, latest frame".into(),
                frames,
                fps: count as f64 / seconds,
                average_capture_milliseconds: if count > 0 {
                    self.capture_millis / count as f64
                } else {
                    0.0
                },
                bounds: self.screen,
                source_bounds: win::window_rect(hwnd).unwrap_or_default(),
            };
            // Telemetry, on the capture thread: one attempt, and a write
            // that fails is simply skipped until the next sample.
            let _ = any2odyssey_core::fsutil::write_json_atomic_once(
                &any2odyssey_core::paths::desktop_capture_status_file(),
                &status,
            );
            self.sample_frames = frames;
            self.sampled = Instant::now();
            self.capture_millis = 0.0;
        }
        Ok(())
    }

    /// Window modes: re-evaluate the source and note when the surface starts
    /// or stops showing one. Every way of having nothing to show used to be
    /// silent, which left a dark 3D screen with no trace of why.
    fn refresh_sources(&mut self) {
        let missing = self.update_thumbnail();
        let shown = missing.is_none();
        if self.source_shown == Some(shown) {
            return;
        }
        self.source_shown = Some(shown);
        self.source_change = Some(shown);
        match missing {
            None => (self.log)("Window capture: the source window is on the 3D surface."),
            Some(why) => (self.log)(&format!(
                "Window capture: nothing to show ({why}); waiting for a source window."
            )),
        }
    }

    /// Window modes: a change the last ticks made to whether a source is
    /// shown (`true`: one is). Each change is handed out once.
    pub fn take_source_change(&mut self) -> Option<bool> {
        self.source_change.take()
    }

    fn drop_thumbnail(&mut self) {
        self.thumbnail = None;
        self.thumbnail_size = None;
    }

    /// Point the thumbnail at the current target. `None` when the surface
    /// shows a source, otherwise why it does not.
    fn update_thumbnail(&mut self) -> Option<&'static str> {
        let exclude = [self.parent_pid, current_pid()];
        let mut target = self.selected;
        if self.mode == CaptureMode::Active {
            let active = win::foreground();
            if win::is_capture_candidate(active, &exclude) {
                if let Some(r) = win::visible_bounds(active) {
                    if r.intersects(&self.screen) {
                        self.last_active = active;
                    }
                }
            }
            target = if self.last_active != HWND::default() {
                self.last_active
            } else {
                self.selected
            };
        }
        if !win::is_capture_candidate(target, &exclude) {
            self.drop_thumbnail();
            return Some(if target == HWND::default() {
                "no source window yet"
            } else if !win::is_window(target) {
                "the source window was closed"
            } else if win::is_iconic(target) {
                "the source window is minimized"
            } else {
                "the source window is hidden or not eligible"
            });
        }
        if self
            .thumbnail
            .as_ref()
            .is_none_or(|(source, _)| *source != target)
        {
            self.drop_thumbnail();
            let Ok(thumb) = win::Thumbnail::register(self.hwnd, target) else {
                // Retry on the next tick, even when the window has not moved.
                return Some("Windows refused a thumbnail of the source window");
            };
            self.thumbnail = Some((target, thumb));
        }
        let (_, thumb) = self.thumbnail.as_ref().expect("registered");
        let Ok((w, h)) = thumb.source_size() else {
            self.drop_thumbnail();
            return Some("the source window's thumbnail was lost");
        };
        if w < 2 || h < 2 {
            self.drop_thumbnail();
            return Some("the source window has no area");
        }
        if self.thumbnail_size == Some((w, h)) {
            return None;
        }
        let source = Rect::new(0, 0, w, h);
        let dest = fit(
            source,
            Rect::new(0, 0, self.screen.width, self.screen.height),
        );
        // Moving a source needs no DWM update. Resizing updates the existing
        // thumbnail; only switching sources replaces the registration.
        if thumb.update(dest, source).is_ok() {
            self.thumbnail_size = Some((w, h));
            return None;
        }
        // A refused resize leaves the previous layout on the surface; only a
        // thumbnail that was never laid out shows nothing. Retried next tick.
        if self.thumbnail_size.is_some() {
            None
        } else {
            Some("Windows refused the thumbnail layout")
        }
    }

    /// Stop timers, drop capture and destroy the window (posts WM_QUIT).
    pub fn close(&mut self) {
        kill_timer(self.hwnd, TIMER_UPDATE);
        self.drop_thumbnail();
        self.desktop = None;
        if win::is_window(self.hwnd) {
            let _ = unsafe { DestroyWindow(self.hwnd) };
        }
    }

    pub fn poll_interval() -> Duration {
        Duration::from_millis(500)
    }
}

impl Drop for Mirror {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offscreen_check_runs_at_first_on_a_display_change_and_when_stale() {
        let t0 = Instant::now();
        assert!(offscreen_check_due(None, t0, false), "never checked");
        // Frames between two checks ride on the last one.
        for millis in [0, 8, 16, 99] {
            assert!(
                !offscreen_check_due(Some(t0), t0 + Duration::from_millis(millis), false),
                "{millis} ms"
            );
        }
        assert!(offscreen_check_due(Some(t0), t0 + OFFSCREEN_RECHECK, false));
        assert!(offscreen_check_due(
            Some(t0),
            t0 + Duration::from_secs(5),
            false
        ));
        // An announced display change does not wait for the interval.
        assert!(offscreen_check_due(
            Some(t0),
            t0 + Duration::from_millis(1),
            true
        ));
        // A check stamped after `now` (never in practice) is not "stale".
        assert!(!offscreen_check_due(
            Some(t0 + Duration::from_millis(5)),
            t0,
            false
        ));
    }
}
