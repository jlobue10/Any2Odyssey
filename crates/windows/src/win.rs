//! Thin, safe-ish wrappers over the Win32 window APIs the program needs.

use any2odyssey_core::geometry::Rect;
use windows::core::BOOL;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail,
    DwmUnregisterThumbnail, DwmUpdateThumbnailProperties, DWMWA_CLOAKED,
    DWMWA_EXTENDED_FRAME_BOUNDS, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY,
    DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};

/// `MONITORINFOF_PRIMARY`.
const MONITORINFOF_PRIMARY: u32 = 1;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, InternalGetWindowText, IsIconic, IsWindow,
    IsWindowVisible, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
    ShowWindow, ShowWindowAsync, GWL_EXSTYLE, GWL_STYLE, HWND_TOPMOST,
    LAYERED_WINDOW_ATTRIBUTES_FLAGS, SET_WINDOW_POS_FLAGS, SHOW_WINDOW_CMD, WINDOW_EX_STYLE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowDisplayAffinity, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
};

/// `WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW`: the
/// styles that make a window click-through, keyboard-neutral and hidden from
/// Alt+Tab. `WS_EX_APPWINDOW` (0x40000) must be cleared alongside.
pub const INPUT_SAFE_EX_STYLES: isize = 0x0800_0000 | 0x0008_0000 | 0x0000_0020 | 0x0000_0080;
pub const WS_EX_APPWINDOW_BIT: isize = 0x0004_0000;
pub const WS_EX_TOPMOST_BIT: isize = 0x0000_0008;

/// Compute the extended style the overlay must have.
pub const fn desired_overlay_ex_style(current: isize) -> isize {
    (current | INPUT_SAFE_EX_STYLES) & !WS_EX_APPWINDOW_BIT
}

pub fn rect_from(r: RECT) -> Rect {
    Rect::from_ltrb(r.left, r.top, r.right, r.bottom)
}
pub fn rect_to(r: Rect) -> RECT {
    RECT {
        left: r.left(),
        top: r.top(),
        right: r.right(),
        bottom: r.bottom(),
    }
}

/// UTF-16, NUL-terminated copy of `s` for PCWSTR parameters.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn is_window(h: HWND) -> bool {
    unsafe { IsWindow(Some(h)).as_bool() }
}
pub fn is_visible(h: HWND) -> bool {
    unsafe { IsWindowVisible(h).as_bool() }
}
pub fn is_iconic(h: HWND) -> bool {
    unsafe { IsIconic(h).as_bool() }
}
pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

pub fn title(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}
/// The title as the window manager stores it, read without sending
/// `WM_GETTEXT`. `GetWindowTextW` sends that message, and waits for the
/// answer, when the window belongs to another thread of this process; a
/// thread that is busy or stuck then holds the caller. For a window whose
/// class leaves its text to the default window procedure the two agree.
pub fn stored_title(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { InternalGetWindowText(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.clamp(0, buf.len() as i32) as usize])
}
pub fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}
pub fn set_title(h: HWND, text: &str) -> windows::core::Result<()> {
    unsafe { SetWindowTextW(h, PCWSTR(wide(text).as_ptr())) }
}
pub fn process_id(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}
pub fn window_rect(h: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(h, &mut r) }
        .ok()
        .map(|_| rect_from(r))
}
/// DWM's extended frame bounds (the visible frame without the invisible resize
/// border), falling back to `GetWindowRect`. `None` when the window has no area.
pub fn visible_bounds(h: HWND) -> Option<Rect> {
    let mut frame = RECT::default();
    let hr = unsafe {
        DwmGetWindowAttribute(
            h,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if hr.is_ok() {
        let r = rect_from(frame);
        if !r.is_empty() {
            return Some(r);
        }
    }
    window_rect(h).filter(|r| !r.is_empty())
}
pub fn is_cloaked(h: HWND) -> bool {
    let mut cloaked = 0i32;
    let hr =
        unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut i32 as *mut _, 4) };
    hr.is_ok() && cloaked != 0
}
pub fn ex_style(h: HWND) -> isize {
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) }
}
pub fn style(h: HWND) -> isize {
    unsafe { GetWindowLongPtrW(h, GWL_STYLE) }
}
pub fn set_ex_style(h: HWND, value: isize) -> isize {
    unsafe { SetWindowLongPtrW(h, GWL_EXSTYLE, value) }
}
/// `SetLayeredWindowAttributes(h, 0, alpha, LWA_ALPHA)`.
pub fn set_layer_alpha(h: HWND, alpha: u8) -> windows::core::Result<()> {
    unsafe {
        SetLayeredWindowAttributes(
            h,
            windows::Win32::Foundation::COLORREF(0),
            alpha,
            LAYERED_WINDOW_ATTRIBUTES_FLAGS(2),
        )
    }
}
pub fn show_window(h: HWND, cmd: i32) -> bool {
    unsafe { ShowWindow(h, SHOW_WINDOW_CMD(cmd)).as_bool() }
}
/// `ShowWindow` posted to the thread that owns `h` instead of sent to it:
/// the caller does not wait for that thread. False when it could not be
/// posted.
pub fn show_window_async(h: HWND, cmd: i32) -> bool {
    unsafe { ShowWindowAsync(h, SHOW_WINDOW_CMD(cmd)).as_bool() }
}
pub fn set_window_pos(
    h: HWND,
    after: Option<HWND>,
    r: Option<Rect>,
    flags: u32,
) -> windows::core::Result<()> {
    let (x, y, w, hgt) = r.map_or((0, 0, 0, 0), |r| (r.x, r.y, r.width, r.height));
    unsafe { SetWindowPos(h, after, x, y, w, hgt, SET_WINDOW_POS_FLAGS(flags)) }
}
pub fn hwnd_topmost() -> HWND {
    HWND_TOPMOST
}
/// `SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE` and the extra `SWP_NOZORDER` /
/// `SWP_SHOWWINDOW` / `SWP_FRAMECHANGED` bits the overlay controller uses.
pub const SWP_NOSIZE_NOMOVE_NOACTIVATE: u32 = 0x0001 | 0x0002 | 0x0010;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const SWP_FRAMECHANGED: u32 = 0x0020;
/// The request is posted to the thread that owns the window rather than sent
/// to it, so the caller does not wait for that thread to process it. Only
/// for a window of another thread; the new position applies a moment later.
pub const SWP_ASYNCWINDOWPOS: u32 = 0x4000;
pub const SW_SHOWNOACTIVATE: i32 = 4;

pub fn set_capture_excluded(h: HWND) -> windows::core::Result<()> {
    unsafe { SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE) }
}
pub fn is_capture_excluded(h: HWND) -> bool {
    let mut affinity = 0u32;
    unsafe { GetWindowDisplayAffinity(h, &mut affinity) }.is_ok()
        && affinity == WDA_EXCLUDEFROMCAPTURE.0
}

/// Enumerate every top-level window.
pub fn enum_windows(mut f: impl FnMut(HWND)) {
    unsafe extern "system" fn thunk(h: HWND, l: LPARAM) -> BOOL {
        let f = unsafe { &mut *(l.0 as *mut &mut dyn FnMut(HWND)) };
        f(h);
        BOOL(1)
    }
    let mut dynf: &mut dyn FnMut(HWND) = &mut f;
    let _ = unsafe { EnumWindows(Some(thunk), LPARAM(&mut dynf as *mut _ as isize)) };
}

/// Check a known source directly; capture refresh does not need a desktop scan.
pub fn is_capture_candidate(h: HWND, exclude_pids: &[u32]) -> bool {
    is_window(h)
        && is_visible(h)
        && !is_iconic(h)
        && !is_cloaked(h)
        && !exclude_pids.contains(&process_id(h))
}

/// Windows eligible as capture sources: visible, not minimized, not cloaked,
/// and not owned by the controls or worker processes.
pub fn candidate_windows(exclude_pids: &[u32]) -> Vec<HWND> {
    let mut out = Vec::new();
    enum_windows(|h| {
        if is_capture_candidate(h, exclude_pids) {
            out.push(h);
        }
    });
    out
}

/// Monitor rectangles; the primary monitor first.
pub fn monitors() -> Vec<(Rect, bool, String, HMONITOR)> {
    unsafe extern "system" fn thunk(m: HMONITOR, _dc: HDC, _r: *mut RECT, l: LPARAM) -> BOOL {
        let out = unsafe { &mut *(l.0 as *mut Vec<(Rect, bool, String, HMONITOR)>) };
        let mut info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
                ..Default::default()
            },
            ..Default::default()
        };
        // The pointer must cover the whole extended struct, which cbSize
        // tells Windows to fill: one derived from `info.monitorInfo` alone
        // would not be valid for the device name written after it.
        let whole = &mut info as *mut MONITORINFOEXW as *mut MONITORINFO;
        if unsafe { GetMonitorInfoW(m, whole) }.as_bool() {
            let name_len = info
                .szDevice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(info.szDevice.len());
            let name = String::from_utf16_lossy(&info.szDevice[..name_len]);
            let primary = (info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;
            out.push((rect_from(info.monitorInfo.rcMonitor), primary, name, m));
        }
        BOOL(1)
    }
    let mut out: Vec<(Rect, bool, String, HMONITOR)> = Vec::new();
    let _ = unsafe {
        EnumDisplayMonitors(None, None, Some(thunk), LPARAM(&mut out as *mut _ as isize))
    };
    out.sort_by_key(|m| !m.1);
    out
}
pub fn primary_monitor() -> Option<(Rect, String, HMONITOR)> {
    monitors()
        .into_iter()
        .find(|m| m.1)
        .map(|m| (m.0, m.2, m.3))
}
pub fn monitor_rects() -> Vec<Rect> {
    monitors().into_iter().map(|m| m.0).collect()
}

#[derive(Default)]
struct MonitorOverlap {
    seen: bool,
    overlaps: bool,
    invalid: bool,
}

impl MonitorOverlap {
    fn observe(&mut self, bounds: Rect, monitor: Option<Rect>) -> BOOL {
        let Some(monitor) = monitor.filter(|r| !r.is_empty()) else {
            self.invalid = true;
            return BOOL(0);
        };
        self.seen = true;
        self.overlaps = monitor.intersects(&bounds);
        BOOL((!self.overlaps) as i32)
    }

    fn finish(self, enumeration_succeeded: bool) -> Result<bool, String> {
        // EnumDisplayMonitors returns false when our callback stops early.
        if self.overlaps {
            return Ok(true);
        }
        if self.invalid || !self.seen || !enumeration_succeeded {
            return Err(
                "Could not verify monitor bounds. Conversion stopped to prevent feedback.".into(),
            );
        }
        Ok(false)
    }
}

/// Check current monitor rectangles without building a list, looking up names,
/// sorting, or allocating. With a null HDC, the callback supplies each monitor's
/// rectangle in virtual-screen coordinates. Stop at the first overlap.
pub fn intersects_any_monitor(bounds: Rect) -> Result<bool, String> {
    unsafe extern "system" fn check(_: HMONITOR, _: HDC, rect: *mut RECT, data: LPARAM) -> BOOL {
        let (bounds, state) = unsafe { &mut *(data.0 as *mut (Rect, MonitorOverlap)) };
        state.observe(*bounds, unsafe { rect.as_ref() }.map(|r| rect_from(*r)))
    }
    let mut query = (bounds, MonitorOverlap::default());
    let succeeded = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(check),
            LPARAM(&mut query as *mut _ as isize),
        )
    }
    .as_bool();
    query.1.finish(succeeded)
}

/// A DWM thumbnail of `source` painted into `destination`.
pub struct Thumbnail(pub isize);

impl Thumbnail {
    pub fn register(destination: HWND, source: HWND) -> windows::core::Result<Thumbnail> {
        let id = unsafe { DwmRegisterThumbnail(destination, source)? };
        Ok(Thumbnail(id))
    }
    pub fn source_size(&self) -> windows::core::Result<(i32, i32)> {
        let size = unsafe { DwmQueryThumbnailSourceSize(self.0)? };
        Ok((size.cx, size.cy))
    }
    /// Show the whole `source` rectangle scaled into `dest`, opaque, frame included.
    pub fn update(&self, dest: Rect, source: Rect) -> windows::core::Result<()> {
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION
                | DWM_TNP_RECTSOURCE
                | DWM_TNP_OPACITY
                | DWM_TNP_VISIBLE
                | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: rect_to(dest),
            rcSource: rect_to(source),
            opacity: 255,
            fVisible: BOOL(1),
            fSourceClientAreaOnly: BOOL(0),
        };
        unsafe { DwmUpdateThumbnailProperties(self.0, &props) }
    }
}

impl Drop for Thumbnail {
    fn drop(&mut self) {
        let _ = unsafe { DwmUnregisterThumbnail(self.0) };
    }
}

#[allow(dead_code)]
fn _assert_ex_style_bits() {
    const _: () = assert!(WINDOW_EX_STYLE(0x0800_0000).0 == 0x0800_0000);
    const _: () = assert!(desired_overlay_ex_style(WS_EX_APPWINDOW_BIT) & WS_EX_APPWINDOW_BIT == 0);
}

#[cfg(test)]
mod monitor_overlap_tests {
    use super::*;

    #[test]
    fn overlapping_monitor_stops_enumeration_without_hiding_the_overlap() {
        let bounds = Rect::new(100, 100, 100, 100);
        let mut state = MonitorOverlap::default();
        assert!(!state
            .observe(bounds, Some(Rect::new(0, 0, 3840, 2160)))
            .as_bool());
        assert_eq!(state.finish(false), Ok(true));
    }

    #[test]
    fn every_monitor_must_be_checked_before_declaring_no_overlap() {
        let bounds = Rect::new(5000, 0, 100, 100);
        let mut state = MonitorOverlap::default();
        assert!(state
            .observe(bounds, Some(Rect::new(0, 0, 3840, 2160)))
            .as_bool());
        assert!(state
            .observe(bounds, Some(Rect::new(-1920, 0, 1920, 1080)))
            .as_bool());
        assert_eq!(state.finish(true), Ok(false));
    }

    #[test]
    fn missing_or_failed_topology_is_not_treated_as_safe() {
        assert!(MonitorOverlap::default().finish(true).is_err());
        let bounds = Rect::new(5000, 0, 100, 100);
        for invalid in [None, Some(Rect::EMPTY)] {
            let mut state = MonitorOverlap::default();
            assert!(!state.observe(bounds, invalid).as_bool());
            assert!(state.finish(false).is_err());
        }
        let mut state = MonitorOverlap::default();
        assert!(state
            .observe(bounds, Some(Rect::new(0, 0, 3840, 2160)))
            .as_bool());
        assert!(state.finish(false).is_err());
    }
}

/// Open a folder or a URL with whatever Windows associates with it, as a
/// double-click would, through the shell itself rather than a helper
/// process. Blocking; call it off the GUI thread.
///
/// The caller decides what may be opened (a folder of ours, an allow-listed
/// URL): the shell runs a program when handed one.
pub fn shell_open(target: &std::ffi::OsStr) -> windows::core::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::E_INVALIDARG;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file: Vec<u16> = target.encode_wide().collect();
    // The shell would stop at an embedded NUL and open something shorter
    // than what the caller checked.
    if file.is_empty() || file.contains(&0) {
        return Err(E_INVALIDARG.into());
    }
    let file: Vec<u16> = file.into_iter().chain(std::iter::once(0)).collect();
    // The shell may hand the request to a COM extension. A thread already in
    // another apartment model keeps it; only a successful call is undone.
    let initialized =
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // NOASYNC: the calling thread ends right after this returns, so the
        // launch must be complete by then. NO_UI: a failure is the caller's
        // to handle, not a message box from a tray application.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: windows::core::w!("open"),
        lpFile: PCWSTR(file.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is fully initialized and `file` outlives the call.
    let result = unsafe { ShellExecuteExW(&mut info) };
    if initialized.is_ok() {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(test)]
mod shell_open_tests {
    use super::*;

    // Opening something for real would put a browser or an Explorer window
    // on the desktop of whoever runs the tests; only the refusals run here.
    #[test]
    fn nothing_is_opened_for_an_empty_or_nul_carrying_target() {
        use std::ffi::OsStr;
        use windows::Win32::Foundation::E_INVALIDARG;
        for target in ["", "https://github.com/\0calc.exe", "\0"] {
            let error = shell_open(OsStr::new(target)).unwrap_err();
            assert_eq!(error.code(), E_INVALIDARG, "{target:?}");
        }
    }
}
