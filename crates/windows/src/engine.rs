//! Loading Samsung's `ConversionWrapper.dll` and the prerequisite checks that
//! run before conversion is started.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use any2odyssey_core::engine::{check_resolution, verify_engine_file, EngineError};
use any2odyssey_core::paths;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    AddDllDirectory, GetProcAddress, LoadLibraryExW, SetDefaultDllDirectories,
    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_LIBRARY_SEARCH_USER_DIRS,
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

/// Limit implicit engine dependencies and later bare-name loads to Windows
/// and the explicitly configured Samsung/Leia folders. The caller's PATH and
/// current directory must not supply executable code to the worker.
fn configure_dll_search() -> Result<(), String> {
    static CONFIGURED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    CONFIGURED
        .get_or_init(|| {
            unsafe {
                SetDefaultDllDirectories(
                    LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS,
                )
            }
            .map_err(|e| format!("DLL search policy: {e}"))?;
            for directory in paths::ENGINE_DLL_DIRS {
                // Optional CUDA/ONNX/Leia components can be absent. A required
                // missing dependency will produce a load error, never a PATH fallback.
                if !std::path::Path::new(directory).is_dir() {
                    continue;
                }
                // The buffer must outlive the call, and the thread's last error
                // must be read before any deallocation can overwrite it.
                let dir = win::wide(directory);
                let cookie = unsafe { AddDllDirectory(PCWSTR(dir.as_ptr())) };
                if cookie.is_null() {
                    return Err(format!(
                        "DLL search folder {directory}: {}",
                        windows::core::Error::from_thread()
                    ));
                }
                // Keep these fixed directories registered for the worker lifetime:
                // the engine may load more dependencies after Start or during Stop.
            }
            Ok(())
        })
        .clone()
}

pub struct EngineLibrary {
    _module: HMODULE,
    start: StartFn,
    stop: StopFn,
}

impl EngineLibrary {
    /// Use the engine working folder for its data and a restricted DLL search
    /// policy for its executable dependencies, then resolve Start/Stop.
    pub fn load() -> Result<EngineLibrary, String> {
        configure_dll_search()?;
        let engine = paths::engine_dir();
        std::env::set_current_dir(&engine).map_err(|e| format!("Engine folder: {e}"))?;
        let dll = paths::engine_dll();
        let module = unsafe {
            LoadLibraryExW(
                PCWSTR(win::wide(&dll.to_string_lossy()).as_ptr()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR
                    | LOAD_LIBRARY_SEARCH_SYSTEM32
                    | LOAD_LIBRARY_SEARCH_USER_DIRS,
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

    /// Wake the worker watchdog before entering the vendor's blocking Stop.
    /// Requiring the event here also covers timer setup failures and WM_QUIT,
    /// which can bypass the worker loop's normal shutdown closure.
    pub fn stop(&self, stop_event: &crate::ipc::Event) {
        let _ = stop_event.set();
        unsafe { (self.stop)() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, LOAD_LIBRARY_FLAGS};

    const CHILD_ENV: &str = "ANY2ODYSSEY_DLL_SEARCH_TEST";

    #[test]
    fn stop_signals_the_watchdog_before_entering_the_engine() {
        use std::cell::Cell;
        use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::WaitForSingleObject;

        // The fake export observes the real kernel event at the instant Stop
        // is entered. Thread-local state keeps parallel tests independent.
        thread_local! {
            static STOP_HANDLE: Cell<isize> = const { Cell::new(0) };
            static WAS_SIGNALED: Cell<bool> = const { Cell::new(false) };
        }
        unsafe extern "C" fn start(
            _: *mut c_void,
            _: i32,
            _: i32,
            _: Option<unsafe extern "C" fn()>,
        ) {
        }
        unsafe extern "C" fn stop() {
            let handle = STOP_HANDLE.with(|h| HANDLE(h.get() as *mut _));
            WAS_SIGNALED
                .with(|seen| seen.set(unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0));
        }
        let name = format!(
            "Local\\Any2Odyssey-stop-test-{}",
            crate::ipc::new_session_id()
        );
        let event = crate::ipc::Event::create(&name, true).unwrap();
        STOP_HANDLE.with(|h| h.set(event.handle().0 as isize));
        let library = EngineLibrary {
            _module: HMODULE::default(),
            start,
            stop,
        };
        assert!(!event.is_signaled());
        library.stop(&event);
        assert!(
            WAS_SIGNALED.with(Cell::get),
            "Stop entered before the watchdog was armed"
        );
        // A previously requested stop remains set for both the watchdog and
        // the message loop; stopping must not reset or consume the signal.
        WAS_SIGNALED.with(|seen| seen.set(false));
        library.stop(&event);
        assert!(WAS_SIGNALED.with(Cell::get));
        assert!(event.is_signaled());
    }

    #[test]
    fn dll_search_rejects_path_and_cwd_but_allows_explicit_directories() {
        let root = tempfile::tempdir().unwrap();
        let system = unsafe {
            LoadLibraryExW(
                windows::core::w!("version.dll"),
                None,
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }
        .unwrap();
        let mut buffer = vec![0u16; 32768];
        let length = unsafe { GetModuleFileNameW(Some(system), &mut buffer) } as usize;
        assert!(length > 0 && length < buffer.len());
        let source = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]));
        unsafe { FreeLibrary(system) }.unwrap();
        for folder in ["cwd", "path", "allowed"] {
            let dir = root.path().join(folder);
            std::fs::create_dir(&dir).unwrap();
            std::fs::copy(&source, dir.join(format!("any2odyssey-{folder}-probe.dll"))).unwrap();
        }
        let mut path = vec![root.path().join("path")];
        path.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        // DLL search policy is process-wide and cannot be reset to the standard
        // policy. Run the real loader assertions in a disposable test process.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "engine::tests::dll_search_child", "--nocapture"])
            .env(CHILD_ENV, root.path())
            .env("PATH", std::env::join_paths(path).unwrap())
            .current_dir(root.path().join("cwd"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn dll_search_child() {
        let Some(root) = std::env::var_os(CHILD_ENV) else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let load = |name: &str| unsafe {
            LoadLibraryExW(
                PCWSTR(win::wide(name).as_ptr()),
                None,
                LOAD_LIBRARY_FLAGS(0),
            )
        };
        // Prove the fixtures are loadable with the original search order.
        for name in ["any2odyssey-cwd-probe.dll", "any2odyssey-path-probe.dll"] {
            let module = load(name).unwrap();
            unsafe { FreeLibrary(module) }.unwrap();
        }
        configure_dll_search().unwrap();
        for name in ["any2odyssey-cwd-probe.dll", "any2odyssey-path-probe.dll"] {
            assert!(load(name).is_err(), "loaded untrusted fixture {name}");
        }
        let allowed = root.join("allowed");
        let cookie =
            unsafe { AddDllDirectory(PCWSTR(win::wide(&allowed.to_string_lossy()).as_ptr())) };
        assert!(!cookie.is_null());
        let module = load("any2odyssey-allowed-probe.dll").unwrap();
        unsafe { FreeLibrary(module) }.unwrap();
        unsafe { windows::Win32::System::LibraryLoader::RemoveDllDirectory(cookie) }.unwrap();
        let system = load("version.dll").unwrap();
        unsafe { FreeLibrary(system) }.unwrap();
    }
}
