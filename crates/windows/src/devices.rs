//! Device-manager query for the Odyssey's stereo camera. Equivalent to the
//! WMI query `Win32_PnPEntity WHERE Name='3D Stereo WebCam'` with
//! `ConfigManagerErrorCode = 0`, done through SetupAPI/CfgMgr32 instead of WMI.

use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_Status, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo,
    SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW, CM_DEVNODE_STATUS_FLAGS, CM_PROB,
    DIGCF_ALLCLASSES, DIGCF_PRESENT, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SP_DEVINFO_DATA,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceState {
    pub name: String,
    /// Device-manager problem code (0 = working).
    pub problem: u32,
}

/// Every present PnP device whose friendly name or description equals `name`.
pub fn find_devices(name: &str) -> windows::core::Result<Vec<DeviceState>> {
    let set = unsafe {
        SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES | DIGCF_PRESENT)?
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
            let mut buf = [0u8; 1024];
            let mut required = 0u32;
            let ok = unsafe {
                SetupDiGetDeviceRegistryPropertyW(
                    set,
                    &data,
                    prop,
                    None,
                    Some(&mut buf),
                    Some(&mut required),
                )
            }
            .is_ok();
            if !ok {
                continue;
            }
            let u16s: Vec<u16> = buf
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&c| c != 0)
                .collect();
            let value = String::from_utf16_lossy(&u16s);
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

/// True when at least one present device called `name` reports no problem.
pub fn device_is_working(name: &str) -> windows::core::Result<bool> {
    Ok(find_devices(name)?.iter().any(|d| d.problem == 0))
}
