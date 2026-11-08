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
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, ValidateRect, HBRUSH, HDC,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, KillTimer,
    PostMessageW, PostQuitMessage, RegisterClassExW, SetTimer, SetWindowLongPtrW, GWLP_USERDATA,
    HWND_BOTTOM, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WM_ERASEBKGND, WM_MOUSEACTIVATE,
    WM_NCHITTEST, WM_PAINT, WNDCLASSEXW, WS_POPUP,
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
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn register_class() -> windows::core::Result<()> {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::AcqRel) {
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
        return Err(windows::core::Error::from_thread());
    }
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

pub fn set_timer(hwnd: HWND, id: usize, millis: u32) {
    unsafe { SetTimer(Some(hwnd), id, millis, None) };
}
pub fn kill_timer(hwnd: HWND, id: usize) {
    let _ = unsafe { KillTimer(Some(hwnd), id) };
}

pub struct Mirror {
    hwnd: HWND,
    mode: CaptureMode,
    screen: Rect,
    device_name: String,
    parent_pid: u32,
    selected: HWND,
    last_active: HWND,
    thumbs: Vec<win::Thumbnail>,
    signature: String,
    desktop: Option<GpuDesktopCapture>,
    frame_wake: Arc<FrameWake>,
    sampled: Instant,
    sample_frames: u64,
    capture_millis: f64,
    pub sources: usize,
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
            device_name: device_name.to_owned(),
            parent_pid,
            selected,
            last_active: HWND::default(),
            thumbs: Vec::new(),
            signature: String::new(),
            desktop: None,
            frame_wake: Arc::new(FrameWake::default()),
            sampled: Instant::now(),
            sample_frames: 0,
            capture_millis: 0.0,
            sources: 0,
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
    pub fn is_desktop(&self) -> bool {
        self.desktop.is_some()
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
        set_timer(self.hwnd, TIMER_UPDATE, self.update_interval_ms);
        Ok(())
    }

    fn assert_offscreen(&self) -> Result<(), String> {
        let Some(actual) = win::window_rect(self.hwnd) else {
            return Err("Capture surface has no bounds.".into());
        };
        if win::monitor_rects().iter().any(|m| m.intersects(&actual)) {
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
        self.assert_offscreen()?;
        let started = Instant::now();
        let hwnd = self.hwnd;
        let capture = self.desktop.as_mut().expect("desktop mode");
        capture.present(hwnd)?;
        self.capture_millis += started.elapsed().as_secs_f64() * 1000.0;
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
            let _ = any2odyssey_core::fsutil::write_json_atomic(
                &any2odyssey_core::paths::desktop_capture_status_file(),
                &status,
            );
            self.sample_frames = frames;
            self.sampled = Instant::now();
            self.capture_millis = 0.0;
        }
        Ok(())
    }

    fn refresh_sources(&mut self) {
        let all = win::candidate_windows(&[self.parent_pid, current_pid()]);
        let mut target = self.selected;
        if self.mode == CaptureMode::Active {
            let active = win::foreground();
            if all.contains(&active) {
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
        let candidates: Vec<HWND> = if target != HWND::default() && all.contains(&target) {
            vec![target]
        } else {
            Vec::new()
        };
        let signature = candidates
            .iter()
            .map(|h| format!("{:?}:{:?}", h.0, win::window_rect(*h).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(";");
        if signature == self.signature {
            return;
        }
        self.signature = signature;
        self.thumbs.clear();
        self.sources = 0;
        for h in candidates {
            let Ok(thumb) = win::Thumbnail::register(self.hwnd, h) else {
                continue;
            };
            let Ok((w, hgt)) = thumb.source_size() else {
                continue;
            };
            if w < 2 || hgt < 2 {
                continue;
            }
            let source = Rect::new(0, 0, w, hgt);
            let dest = fit(
                source,
                Rect::new(0, 0, self.screen.width, self.screen.height),
            );
            if thumb.update(dest, source).is_ok() {
                self.thumbs.push(thumb);
                self.sources += 1;
            }
        }
    }

    /// Stop timers, drop capture and destroy the window (posts WM_QUIT).
    pub fn close(&mut self) {
        kill_timer(self.hwnd, TIMER_UPDATE);
        self.thumbs.clear();
        self.desktop = None;
        if win::is_window(self.hwnd) {
            let _ = unsafe { DestroyWindow(self.hwnd) };
        }
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
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
