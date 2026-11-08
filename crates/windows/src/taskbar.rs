//! Shell integration: remove the renderer's stale taskbar entry and register
//! the controls' jump-list tasks.

use any2odyssey_core::ipc;
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{HWND, PROPERTYKEY};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ITaskbarList,
    ShellLink, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{FlashWindowEx, FLASHWINFO, FLASHW_STOP};

use crate::processes::current_pid;
use crate::win::{class_name, process_id, title, wide};

/// Remove `window`'s taskbar button. Refuses anything that is not this
/// process's renderer output window, so a bug cannot touch other apps.
pub fn remove_renderer_tab(window: HWND) -> Result<(), String> {
    if process_id(window) != current_pid()
        || class_name(window) != ipc::ENGINE_OUTPUT_CLASS
        || !ipc::is_output_title(&title(window))
    {
        return Err("Refusing taskbar cleanup for a window outside this renderer.".into());
    }
    let flash = FLASHWINFO {
        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
        hwnd: window,
        dwFlags: FLASHW_STOP,
        uCount: 0,
        dwTimeout: 0,
    };
    let _ = unsafe { FlashWindowEx(&flash) };
    let taskbar: ITaskbarList =
        unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| e.to_string())?;
    unsafe {
        taskbar.HrInit().map_err(|e| e.to_string())?;
        taskbar.DeleteTab(window).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `PKEY_Title`: {F29F85E0-4FF9-1068-AB91-08002B27B3D9}, 2.
const PKEY_TITLE: PROPERTYKEY = PROPERTYKEY {
    fmtid: windows::core::GUID::from_u128(0xF29F85E0_4FF9_1068_AB91_08002B27B3D9),
    pid: 2,
};

pub struct JumpTask<'a> {
    pub title: &'a str,
    pub description: &'a str,
    pub arguments: &'a str,
}

/// Register user tasks ("Turn 3D on / off", "Open 3D controls") for `exe`.
pub fn install_jump_list(exe: &str, tasks: &[JumpTask]) -> windows::core::Result<()> {
    unsafe {
        let list: ICustomDestinationList =
            CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
        let mut slots = 0u32;
        let _removed: IObjectArray = list.BeginList(&mut slots)?;
        let collection: IObjectCollection =
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
        for task in tasks {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(PCWSTR(wide(exe).as_ptr()))?;
            link.SetArguments(PCWSTR(wide(task.arguments).as_ptr()))?;
            link.SetDescription(PCWSTR(wide(task.description).as_ptr()))?;
            link.SetIconLocation(PCWSTR(wide(exe).as_ptr()), 0)?;
            let store: IPropertyStore = link.cast()?;
            store.SetValue(&PKEY_TITLE, &PROPVARIANT::from(task.title))?;
            store.Commit()?;
            collection.AddObject(&link)?;
        }
        let array: IObjectArray = collection.cast()?;
        list.AddUserTasks(&array)?;
        list.CommitList()?;
    }
    Ok(())
}

pub fn default_tasks() -> [JumpTask<'static>; 2] {
    [
        JumpTask {
            title: "Turn 3D on / off",
            description: "Toggle Any2Odyssey 3D conversion",
            arguments: "--toggle",
        },
        JumpTask {
            title: "Open 3D controls",
            description: "Show Any2Odyssey",
            arguments: "--show",
        },
    ]
}
