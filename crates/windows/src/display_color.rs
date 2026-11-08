//! SDR white level of a monitor when Windows advanced color (HDR) is active.
//! Captured HDR frames are divided by this scale before the sRGB transfer.

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
    let path_data = active_paths()?;
    for path in &path_data {
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
        if String::from_utf16_lossy(&name.viewGdiDeviceName[..len]) != monitor {
            continue;
        }
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
        return None;
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
