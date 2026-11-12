//! SDR white level of a monitor when Windows advanced color (HDR) is active.
//! Captured HDR frames are divided by this scale before the sRGB transfer.
//! The same display-configuration query gives the monitor's refresh rate,
//! which sets the capture cadence.

use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
};

use any2odyssey_core::capture_color::CaptureColor;
use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;

/// Query failure is distinct from an HDR result. The caller decides how long
/// an already running pipeline may retain its last confirmed color state.
pub fn capture_color(monitor: &str) -> Option<CaptureColor> {
    let path = monitor_path(monitor)?;
    let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO,
            size: std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32,
            adapterId: path.targetInfo.adapterId,
            id: path.targetInfo.id,
        },
        ..Default::default()
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut color.header) } != 0 {
        return None;
    }
    // Bit 1 of the flags word: advancedColorEnabled.
    let flags = unsafe { color.Anonymous.value };
    if flags & 2 == 0 {
        return Some(CaptureColor {
            advanced_color: false,
            white_scale: 1.0,
        });
    }
    let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL {
        header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
            r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
            size: std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32,
            adapterId: path.targetInfo.adapterId,
            id: path.targetInfo.id,
        },
        SDRWhiteLevel: 1000,
    };
    if unsafe { DisplayConfigGetDeviceInfo(&mut white.header) } == 0 {
        let value = white.SDRWhiteLevel as f32 / 1000.0;
        return Some(CaptureColor {
            advanced_color: true,
            white_scale: value.clamp(1.0, 12.5),
        });
    }
    None
}

/// Refresh rate in Hz of the monitor behind the GDI device `monitor`, as its
/// active display path reports it. `None` when the query fails, the monitor
/// is not found, or Windows does not know the rate.
pub fn refresh_rate(monitor: &str) -> Option<f64> {
    let rate = monitor_path(monitor)?.targetInfo.refreshRate;
    rational_hz(rate.Numerator, rate.Denominator)
}

/// A `DISPLAYCONFIG_RATIONAL` as Hz. An unknown rate is reported as 0/0 (or
/// with only one of the two set).
fn rational_hz(numerator: u32, denominator: u32) -> Option<f64> {
    (numerator != 0 && denominator != 0).then(|| numerator as f64 / denominator as f64)
}

/// The active display path whose source is the GDI device `monitor`.
fn monitor_path(monitor: &str) -> Option<DISPLAYCONFIG_PATH_INFO> {
    for path in active_paths()? {
        let mut name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            ..Default::default()
        };
        if unsafe { DisplayConfigGetDeviceInfo(&mut name.header) } != 0 {
            continue;
        }
        let len = name
            .viewGdiDeviceName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(name.viewGdiDeviceName.len());
        if String::from_utf16_lossy(&name.viewGdiDeviceName[..len]) == monitor {
            return Some(path);
        }
    }
    None
}

/// Display topology can grow between the sizing and query calls. Retry that
/// documented race with fresh buffers, but bound work on the capture thread.
fn active_paths() -> Option<Vec<DISPLAYCONFIG_PATH_INFO>> {
    for _ in 0..3 {
        let mut paths = 0;
        let mut modes = 0;
        unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes) }
            .ok()
            .ok()?;
        let mut path_data = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
        let mut mode_data = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
        let result = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut paths,
                path_data.as_mut_ptr(),
                &mut modes,
                mode_data.as_mut_ptr(),
                None,
            )
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        result.ok().ok()?;
        path_data.truncate(paths as usize);
        return Some(path_data);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_refresh_rate_is_not_a_number() {
        assert_eq!(rational_hz(60, 1), Some(60.0));
        assert_eq!(rational_hz(60_000, 1001), Some(60_000.0 / 1001.0));
        assert_eq!(rational_hz(164_956, 1000), Some(164.956));
        for unknown in [(0, 0), (0, 1), (60, 0)] {
            assert_eq!(rational_hz(unknown.0, unknown.1), None, "{unknown:?}");
        }
    }
}
