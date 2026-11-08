//! Loading Samsung's `ConversionWrapper.dll` and the prerequisite checks that
//! run before conversion is started.

use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use any2odyssey_core::engine::{check_resolution, verify_engine_file, EngineError};
use any2odyssey_core::paths;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, SetDllDirectoryW, LOAD_WITH_ALTERED_SEARCH_PATH,
};

use crate::{devices, processes, win};

/// `void Start(HWND source, BOOL sideBySide, int cores, void (*closed)(void))`, cdecl.
pub type StartFn = unsafe extern "C" fn(*mut c_void, i32, i32, Option<unsafe extern "C" fn()>);
/// `void Stop(void)`, cdecl.
pub type StopFn = unsafe extern "C" fn();

/// Set by the engine's "closed" callback; polled by the worker loop.
pub static ENGINE_CLOSED: AtomicBool = AtomicBool::new(false);

pub unsafe extern "C" fn on_engine_closed() {
    ENGINE_CLOSED.store(true, Ordering::Release);
}

/// All prerequisite checks, in the order the user sees them.
pub fn validate() -> Result<(), EngineError> {
    verify_engine_file(&paths::engine_dll())?;
    if !paths::worker_exe().is_file() {
        return Err(EngineError::WorkerMissing);
    }
    let (screen, _, _) = win::primary_monitor().ok_or(EngineError::WrongResolution)?;
    check_resolution(screen.width, screen.height)?;
    match devices::device_is_working(paths::STEREO_CAMERA_NAME) {
        Ok(true) => {}
        Ok(false) => return Err(EngineError::CameraUnavailable),
        Err(e) => return Err(EngineError::CameraCheckFailed(e.to_string())),
    }
    if processes::is_process_running(paths::SAMSUNG_PLAYER_PROCESS) {
        return Err(EngineError::SamsungPlayerRunning);
    }
    Ok(())
}

pub struct EngineLibrary {
    _module: HMODULE,
    start: StartFn,
    stop: StopFn,
}

impl EngineLibrary {
    /// Make the engine folder the current directory, prepend Samsung's
    /// folders to PATH, then load the library and resolve `Start`/`Stop`.
    pub fn load() -> Result<EngineLibrary, String> {
        let engine = paths::engine_dir();
        std::env::set_current_dir(&engine).map_err(|e| format!("Engine folder: {e}"))?;
        let mut path = paths::ENGINE_PATH_EXTRA.join(";");
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(';');
            path.push_str(&existing.to_string_lossy());
        }
        std::env::set_var("PATH", path);
        unsafe { SetDllDirectoryW(PCWSTR(win::wide(&engine.to_string_lossy()).as_ptr())) }
            .map_err(|e| format!("SetDllDirectory: {e}"))?;
        let dll = paths::engine_dll();
        let module = unsafe {
            LoadLibraryExW(
                PCWSTR(win::wide(&dll.to_string_lossy()).as_ptr()),
                None,
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        }
        .map_err(|e| format!("Engine load error {}", e.code().0 as u32 & 0xffff))?;
        let start = unsafe { GetProcAddress(module, windows::core::s!("Start")) }
            .ok_or("Engine has no Start export.")?;
        let stop = unsafe { GetProcAddress(module, windows::core::s!("Stop")) }
            .ok_or("Engine has no Stop export.")?;
        // SAFETY: the exports' signatures are those documented for this exact DLL build (hash-pinned).
        let start: StartFn = unsafe { std::mem::transmute(start) };
        let stop: StopFn = unsafe { std::mem::transmute(stop) };
        Ok(EngineLibrary {
            _module: module,
            start,
            stop,
        })
    }

    pub fn start(&self, source: windows::Win32::Foundation::HWND, side_by_side: bool, cores: i32) {
        ENGINE_CLOSED.store(false, Ordering::Release);
        unsafe { (self.start)(source.0, side_by_side as i32, cores, Some(on_engine_closed)) };
    }

    pub fn stop(&self) {
        unsafe { (self.stop)() };
    }
}

/// True when Samsung's conversion log reports the renderer UI minimized,
/// which is the only observable "running" signal the engine gives.
pub fn conversion_log_reports_ready(log_path: &Path) -> bool {
    any2odyssey_core::fsutil::tail(log_path, 9000)
        .contains(any2odyssey_core::engine::ENGINE_READY_MARKER)
}
