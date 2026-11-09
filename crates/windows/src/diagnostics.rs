//! Diagnostic commands: `--inspect <pid>`, `--layout-probe <parent> <worker>`,
//! `--test-capture-exclusion`, `--test-offscreen`. They write JSON reports to
//! the data directory. The two visible tests create their own coloured windows
//! and capture only those pixels; no screenshots of user content are saved.

use std::time::Instant;

use any2odyssey_core::fsutil::write_json_atomic;
use any2odyssey_core::geometry::{offscreen_bounds, Rect};
use any2odyssey_core::paths;
use serde::Serialize;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, DispatchMessageW, GetMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    WM_TIMER,
};

use crate::capture::{GpuDesktopCapture, GpuSurface};
use crate::mirror::{create_plain_window, kill_timer, set_timer};
use crate::win;

#[derive(Serialize)]
struct WindowInfo {
    handle: isize,
    title: String,
    #[serde(rename = "windowClass")]
    window_class: String,
    style: String,
    extended: String,
    visible: bool,
    minimized: bool,
    bounds: Rect,
    foreground: bool,
}

/// All windows of `target_pid`, with styles and bounds.
pub fn inspect(target_pid: u32) -> Vec<serde_json::Value> {
    let fg = win::foreground();
    let mut out = Vec::new();
    win::enum_windows(|h| {
        if win::process_id(h) == target_pid {
            let info = WindowInfo {
                handle: h.0 as isize,
                title: win::title(h),
                window_class: win::class_name(h),
                style: format!("{:X}", win::style(h)),
                extended: format!("{:X}", win::ex_style(h)),
                visible: win::is_visible(h),
                minimized: win::is_iconic(h),
                bounds: win::window_rect(h).unwrap_or_default(),
                foreground: fg == h,
            };
            out.push(serde_json::to_value(info).expect("serializable"));
        }
    });
    out
}

pub fn write_inspect(target_pid: u32) -> std::io::Result<()> {
    write_json_atomic(&paths::window_diagnostics_file(), &inspect(target_pid))
}

/// For every candidate source window: raw and DWM frame bounds, thumbnail
/// source size, and whether each clipping rule would include it.
pub fn write_layout_probe(parent_pid: u32, worker_pid: u32) -> Result<(), String> {
    let (screen, _, _) = win::primary_monitor().ok_or("No primary monitor.")?;
    let destination = create_plain_window(
        "Any2Odyssey layout probe",
        Rect::new(0, 0, 10, 10),
        Some([0, 0, 0]),
        false,
    )
    .map_err(|e| e.to_string())?;
    let mut rows = Vec::new();
    for h in win::candidate_windows(&[parent_pid, worker_pid]) {
        let Some(raw) = win::window_rect(h) else {
            continue;
        };
        let frame = win::visible_bounds(h);
        let (tw, th) = win::Thumbnail::register(destination, h)
            .ok()
            .and_then(|t| t.source_size().ok())
            .unwrap_or((0, 0));
        let old_clip = screen.intersect(&raw);
        let new_clip = frame.map(|f| screen.intersect(&f)).unwrap_or_default();
        rows.push(serde_json::json!({
            "handle": h.0 as isize,
            "windowClass": win::class_name(h),
            "raw": raw,
            "visibleFrame": frame.unwrap_or_default(),
            "frameResult": if frame.is_some() { 0 } else { -1 },
            "thumbnailWidth": tw,
            "thumbnailHeight": th,
            "oldClip": old_clip,
            "newClip": new_clip,
            "oldIncluded": old_clip.width > 1 && old_clip.height > 1,
            "visibleIncluded": frame.is_some() && new_clip.width > 1 && new_clip.height > 1,
        }));
    }
    let _ = unsafe { DestroyWindow(destination) };
    let monitors: Vec<_> = win::monitors().into_iter().map(|(bounds, primary, name, _)| serde_json::json!({"name": name, "bounds": bounds, "primary": primary})).collect();
    write_json_atomic(&paths::projection_diagnostics_file(), &serde_json::json!({"time": any2odyssey_core::log::now_iso(), "monitors": monitors, "windows": rows})).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct TestReport {
    passed: bool,
    result: String,
    checks: Vec<String>,
    time: String,
}

fn pixel(pixels: &[u8], width: i32, x: i32, y: i32) -> [u8; 3] {
    let i = ((y * width + x) * 4) as usize;
    [pixels[i + 2], pixels[i + 1], pixels[i]] // BGRA -> RGB
}

/// `pixel` for error messages: a buffer that never received a frame (or a
/// short one) reads as "no frame" instead of indexing out of bounds.
fn describe_pixel(pixels: &[u8], width: i32, height: i32, x: i32, y: i32) -> String {
    if x < 0 || y < 0 || x >= width || y >= height {
        return "no frame".to_owned();
    }
    let end = ((y * width + x) * 4) as usize + 4;
    if pixels.len() < (width * height * 4) as usize || pixels.len() < end {
        return "no frame".to_owned();
    }
    format!("{:?}", pixel(pixels, width, x, y))
}

/// Pump messages for roughly `millis`, then return.
fn pump(millis: u64) {
    let until = Instant::now() + std::time::Duration::from_millis(millis);
    let mut msg = MSG::default();
    while Instant::now() < until {
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

const CYAN: [u8; 3] = [30, 170, 210];
const GREEN: [u8; 3] = [170, 230, 50];
const RED: [u8; 3] = [255, 0, 0];
const MAGENTA: [u8; 3] = [255, 0, 255];

/// Capture must exclude a window marked `WDA_EXCLUDEFROMCAPTURE`, and moved or
/// closed content must disappear from fresh frames. Returns the process exit code.
pub fn test_capture_exclusion() -> i32 {
    let report = |passed: bool, result: String, checks: Vec<String>| {
        let _ = write_json_atomic(
            &paths::data_dir().join("capture-exclusion-test.json"),
            &TestReport {
                passed,
                result,
                checks,
                time: any2odyssey_core::log::now_iso(),
            },
        );
        if passed {
            0
        } else {
            1
        }
    };
    let Some((screen, device, _)) = win::primary_monitor() else {
        return report(false, "No primary monitor.".into(), vec![]);
    };
    let bounds = Rect::new(screen.x + 80, screen.y + 80, 160, 80);
    let mut checks = Vec::new();
    let outcome = (|| -> Result<(), String> {
        let background =
            create_plain_window("Any2Odyssey test background", bounds, Some(CYAN), true)
                .map_err(|e| e.to_string())?;
        let excluded =
            create_plain_window("Any2Odyssey test excluded", bounds, Some(MAGENTA), true)
                .map_err(|e| e.to_string())?;
        let popup = create_plain_window(
            "Any2Odyssey test popup",
            Rect::new(bounds.x, bounds.y, 40, 80),
            Some(RED),
            true,
        )
        .map_err(|e| e.to_string())?;
        let mut capture = GpuDesktopCapture::new(bounds, &device, || {}, &|_| {})?;
        win::show_window(background, win::SW_SHOWNOACTIVATE);
        pump(300);
        let mut pixels = Vec::new();
        let expect = |capture: &mut GpuDesktopCapture,
                      pixels: &mut Vec<u8>,
                      left: [u8; 3],
                      right: [u8; 3],
                      stage: &str|
         -> Result<(), String> {
            for _ in 0..30 {
                pump(100);
                if capture.read_back(pixels)?
                    && pixels.len() >= (bounds.width * bounds.height * 4) as usize
                    && pixel(pixels, bounds.width, 20, 40) == left
                    && pixel(pixels, bounds.width, 120, 40) == right
                {
                    return Ok(());
                }
            }
            Err(format!(
                "Stage {stage} failed: left={} right={} frames={}",
                describe_pixel(pixels, bounds.width, bounds.height, 20, 40),
                describe_pixel(pixels, bounds.width, bounds.height, 120, 40),
                capture.frames
            ))
        };
        expect(
            &mut capture,
            &mut pixels,
            CYAN,
            CYAN,
            "0 initial desktop frame",
        )?;
        checks.push("Initial desktop frame".into());
        win::set_capture_excluded(excluded).map_err(|e| format!("Window exclusion failed: {e}"))?;
        win::show_window(excluded, win::SW_SHOWNOACTIVATE);
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                background,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                crate::mirror::colorref(GREEN) as isize,
            )
        };
        let _ =
            unsafe { windows::Win32::Graphics::Gdi::InvalidateRect(Some(background), None, true) };
        expect(
            &mut capture,
            &mut pixels,
            GREEN,
            GREEN,
            "1 fresh pixels through excluded output",
        )?;
        checks.push("Fresh underlying pixels through excluded output".into());
        win::show_window(excluded, 0);
        win::show_window(popup, win::SW_SHOWNOACTIVATE);
        expect(&mut capture, &mut pixels, RED, GREEN, "2 transient popup")?;
        checks.push("Transient popup appears".into());
        win::set_window_pos(
            popup,
            None,
            Some(Rect::new(bounds.x + 100, bounds.y, 40, 80)),
            win::SWP_NOSIZE_NOMOVE_NOACTIVATE & !0x0002,
        )
        .map_err(|e| e.to_string())?;
        expect(
            &mut capture,
            &mut pixels,
            GREEN,
            RED,
            "3 moving popup clears",
        )?;
        checks.push("Moving popup clears its previous position".into());
        win::show_window(popup, 0);
        expect(
            &mut capture,
            &mut pixels,
            GREEN,
            GREEN,
            "4 closing popup restores background",
        )?;
        checks.push("Closing popup restores the background".into());
        for h in [popup, excluded, background] {
            let _ = unsafe { DestroyWindow(h) };
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => report(
            true,
            "All desktop capture regression checks passed.".into(),
            checks,
        ),
        Err(e) => report(false, e, checks),
    }
}

/// A GPU surface placed outside every monitor must stay live and capturable
/// through a DWM thumbnail shown on screen.
pub fn test_offscreen() -> i32 {
    let report = |passed: bool, result: String, bounds: Rect| {
        let _ = write_json_atomic(
            &paths::data_dir().join("offscreen-surface-test.json"),
            &serde_json::json!({"passed": passed, "result": result, "bounds": bounds, "time": any2odyssey_core::log::now_iso()}),
        );
        if passed {
            0
        } else {
            1
        }
    };
    let Some((screen, device, _)) = win::primary_monitor() else {
        return report(false, "No primary monitor.".into(), Rect::EMPTY);
    };
    let monitors = win::monitor_rects();
    let outside = offscreen_bounds(screen.size(), &monitors);
    let on_screen = Rect::new(screen.x + 80, screen.y + 80, 80, 80);
    let outcome = (|| -> Result<(), String> {
        let source = create_plain_window("Any2Odyssey offscreen source", outside, None, true)
            .map_err(|e| e.to_string())?;
        let preview = create_plain_window(
            "Any2Odyssey offscreen preview",
            on_screen,
            Some([0, 0, 0]),
            true,
        )
        .map_err(|e| e.to_string())?;
        let mut capture = GpuDesktopCapture::new(on_screen, &device, || {}, &|_| {})?;
        let mut device_obj: Option<ID3D11Device> = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device_obj),
                None,
                None,
            )
        }
        .map_err(|e| e.to_string())?;
        let device_obj = device_obj.ok_or("No Direct3D device")?;
        let surface = GpuSurface::new(
            &device_obj,
            source,
            screen.width as u32,
            screen.height as u32,
        )
        .map_err(|e| e.to_string())?;
        win::show_window(preview, win::SW_SHOWNOACTIVATE);
        win::show_window(source, win::SW_SHOWNOACTIVATE);
        surface.clear(CYAN).map_err(|e| e.to_string())?;
        let actual = win::window_rect(source).ok_or("Source window has no bounds")?;
        if monitors.iter().any(|m| m.intersects(&actual)) {
            return Err("Source surface overlaps a monitor.".into());
        }
        let thumb = win::Thumbnail::register(preview, source)
            .map_err(|e| format!("Offscreen thumbnail failed: {e}"))?;
        thumb
            .update(
                Rect::new(0, 0, 80, 80),
                Rect::new(0, 0, screen.width, screen.height),
            )
            .map_err(|e| e.to_string())?;
        let mut pixels = Vec::new();
        for (stage, expected) in [(0, CYAN), (1, GREEN)] {
            let mut ok = false;
            for _ in 0..30 {
                surface.clear(expected).map_err(|e| e.to_string())?;
                pump(100);
                if capture.read_back(&mut pixels)?
                    && pixels.len() >= (80 * 80 * 4)
                    && pixel(&pixels, 80, 40, 40) == expected
                {
                    ok = true;
                    break;
                }
            }
            if !ok {
                return Err(format!(
                    "Offscreen surface did not deliver fresh pixels at stage {stage}; pixel {}",
                    describe_pixel(&pixels, 80, 80, 40, 40)
                ));
            }
        }
        drop(thumb);
        let _ = unsafe { DestroyWindow(source) };
        let _ = unsafe { DestroyWindow(preview) };
        Ok(())
    })();
    match outcome {
        Ok(()) => report(
            true,
            "A GPU offscreen surface remains live and capturable, without overlapping any monitor."
                .into(),
            outside,
        ),
        Err(e) => report(false, e, outside),
    }
}

/// Keep the unused-import lints quiet for message helpers used only on some paths.
#[allow(dead_code)]
fn _unused(_: HWND, _: &MSG) {
    let _ = (GetMessageW, WM_TIMER, set_timer, kill_timer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_pixel_reports_missing_frames_instead_of_panicking() {
        assert_eq!(describe_pixel(&[], 80, 80, 40, 40), "no frame");
        assert_eq!(describe_pixel(&[0; 16], 80, 80, 40, 40), "no frame");
        let mut full = vec![0u8; 80 * 80 * 4];
        let i = (40 * 80 + 40) * 4;
        full[i..i + 4].copy_from_slice(&[1, 2, 3, 255]);
        assert_eq!(describe_pixel(&full, 80, 80, 40, 40), "[3, 2, 1]");
        assert_eq!(describe_pixel(&full, 80, 80, 80, 40), "no frame");
    }
}
