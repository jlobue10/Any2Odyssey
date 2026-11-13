//! Device-manager query for the Odyssey's stereo camera. Equivalent to the
//! WMI query `Win32_PnPEntity WHERE Name='3D Stereo WebCam'` with
//! `ConfigManagerErrorCode = 0`, done through SetupAPI/CfgMgr32 instead of WMI.

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_Status, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
    SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW, CM_DEVNODE_STATUS_FLAGS, CM_PROB,
    DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SETUP_DI_REGISTRY_PROPERTY, SPDRP_DEVICEDESC,
    SPDRP_FRIENDLYNAME, SP_DEVINFO_DATA,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceState {
    pub name: String,
    /// Device-manager problem code (0 = working).
    pub problem: u32,
}

/// The device setup classes a USB camera is installed in: `Camera`, where
/// the Odyssey's stereo camera appears with Windows' own driver (seen on a
/// G90XF: `USB\VID_04E8&PID_20D7&MI_00`), and `Image`, where vendor camera
/// drivers still put theirs.
const CAMERA_CLASSES: [GUID; 2] = [
    GUID::from_u128(0xca3e7ab9_b4c3_4ae6_8251_579ef933890f),
    GUID::from_u128(0x6bdd1fc6_810f_11d0_bec7_08002be2092f),
];

/// Every present PnP device whose friendly name or description equals `name`.
pub fn find_devices(name: &str) -> windows::core::Result<Vec<DeviceState>> {
    find_devices_in(None, name)
}

/// [`find_devices`] within one device setup class, or in all of them.
fn find_devices_in(class: Option<&GUID>, name: &str) -> windows::core::Result<Vec<DeviceState>> {
    let set = unsafe {
        match class {
            Some(class) => SetupDiGetClassDevsW(Some(class), PCWSTR::null(), None, DIGCF_PRESENT)?,
            None => {
                SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES | DIGCF_PRESENT)?
            }
        }
    };
    let mut found = Vec::new();
    let mut index = 0u32;
    loop {
        let mut data = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInfo(set, index, &mut data) }.is_err() {
            break;
        }
        index += 1;
        for prop in [SPDRP_FRIENDLYNAME, SPDRP_DEVICEDESC] {
            let Some(value) = property(set, &data, prop) else {
                continue;
            };
            if value == name {
                let mut status = CM_DEVNODE_STATUS_FLAGS(0);
                let mut problem = CM_PROB(0);
                let rc =
                    unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, data.DevInst, 0) };
                let problem = if rc.0 == 0 { problem.0 } else { u32::MAX };
                found.push(DeviceState {
                    name: value,
                    problem,
                });
                break;
            }
        }
    }
    let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
    Ok(found)
}

/// One string property of a device; `None` when the device has none.
fn property(
    set: HDEVINFO,
    data: &SP_DEVINFO_DATA,
    prop: SETUP_DI_REGISTRY_PROPERTY,
) -> Option<String> {
    let mut buf = [0u8; 1024];
    let mut required = 0u32;
    unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set,
            data,
            prop,
            None,
            Some(&mut buf),
            Some(&mut required),
        )
    }
    .ok()?;
    let u16s: Vec<u16> = buf
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&c| c != 0)
        .collect();
    Some(String::from_utf16_lossy(&u16s))
}

/// True when at least one present device called `name` reports no problem.
pub fn device_is_working(name: &str) -> windows::core::Result<bool> {
    Ok(find_devices(name)?.iter().any(|d| d.problem == 0))
}

/// [`device_is_working`] for a camera. Both the controls and the worker run
/// this before every start, and reading two properties of each of the few
/// hundred present devices is most of its cost; a working camera is found in
/// one of the two camera classes, which hold a handful. Only when it is not
/// found there is every class searched, as before, so the answer never
/// differs from [`device_is_working`]: a camera installed in some other
/// class is still found, and an error is still the full search's error.
pub fn camera_is_working(name: &str) -> windows::core::Result<bool> {
    let in_camera_class = CAMERA_CLASSES.iter().any(|class| {
        find_devices_in(Some(class), name).is_ok_and(|found| found.iter().any(|d| d.problem == 0))
    });
    if in_camera_class {
        return Ok(true);
    }
    device_is_working(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_that_does_not_exist_is_not_found_in_any_search() {
        let name = "Any2Odyssey test: no such device";
        for class in &CAMERA_CLASSES {
            assert_eq!(find_devices_in(Some(class), name).unwrap(), Vec::new());
        }
        assert_eq!(find_devices(name).unwrap(), Vec::new());
        assert!(!device_is_working(name).unwrap());
        assert!(!camera_is_working(name).unwrap());
    }

    #[test]
    fn the_class_search_agrees_with_the_search_of_every_class() {
        // Whatever cameras this machine has (possibly none): each one found
        // by class is found, in the same state, by the full search.
        for class in &CAMERA_CLASSES {
            let set =
                unsafe { SetupDiGetClassDevsW(Some(class), PCWSTR::null(), None, DIGCF_PRESENT) }
                    .unwrap();
            let mut names = Vec::new();
            let mut index = 0u32;
            loop {
                let mut data = SP_DEVINFO_DATA {
                    cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                    ..Default::default()
                };
                if unsafe { SetupDiEnumDeviceInfo(set, index, &mut data) }.is_err() {
                    break;
                }
                index += 1;
                if let Some(name) = property(set, &data, SPDRP_FRIENDLYNAME) {
                    names.push(name);
                }
            }
            let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
            for name in names {
                let by_class = find_devices_in(Some(class), &name).unwrap();
                assert!(!by_class.is_empty(), "{name}");
                let all = find_devices(&name).unwrap();
                for device in &by_class {
                    assert!(all.contains(device), "{name}: {device:?} not in {all:?}");
                }
                assert_eq!(
                    camera_is_working(&name).unwrap(),
                    device_is_working(&name).unwrap(),
                    "{name}"
                );
            }
        }
    }
}
