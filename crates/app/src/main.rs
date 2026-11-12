//! Any2Odyssey entry point: desktop controls by default, or one of the
//! command-line modes used by the installer, the worker and diagnostics.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod capture;
mod core;
#[cfg(not(windows))]
mod depth;
mod guide;
mod hotkeys;
mod lens_switch;
#[cfg(target_os = "linux")]
mod live;
mod platform;
#[cfg(windows)]
mod tray;
mod ui;
mod weaver;
// Used by the Windows start path; compiled for tests everywhere.
#[cfg(any(windows, test))]
mod worker_version;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use any2odyssey_core::geometry::{fit, offscreen_bounds, Rect, Size};
use any2odyssey_core::log::Logger;
use any2odyssey_core::{ipc, paths, APP_NAME};

fn usage() -> String {
    let mut text = format!(
        "{APP_NAME} {}\n\nUsage:\n  Any2Odyssey                      open the controls\n  Any2Odyssey --toggle             turn 3D on/off in the running instance (or start one and turn 3D on)\n  Any2Odyssey --show               show the running controls (or open them when no instance is running)\n  Any2Odyssey --quit               ask the running instance to quit (exit 1 when none is running)\n  Any2Odyssey --help               print this text\n  Any2Odyssey --self-test          geometry and source-isolation checks\n  Any2Odyssey --check-updates      ask GitHub for the newest release (exit 0 current, 10 update available, 1 error)\n  Any2Odyssey --export-guide FILE  write the offline HTML guide\n  Any2Odyssey --screenshot FILE [--dark] [--help-page]   render the controls in an isolated preview to a PNG\n  Any2Odyssey --worker EVENT PARENT_PID SOURCE_HWND PREFERENCES_JSON   (used internally)\n  Any2Odyssey --inspect PID | --layout-probe PARENT WORKER | --test-capture-exclusion | --test-offscreen",
        any2odyssey_core::VERSION
    );
    if cfg!(not(windows)) {
        text.push_str("\n  Any2Odyssey [--open] FILE         open an image in the stereo viewer (in the running instance when there is one)");
        text.push_str("\n  Any2Odyssey --install-depth-model  download the AI depth model for 2D → 3D (about 95 MB, needs curl)");
    }
    text
}

fn self_test(logger: &Logger) -> i32 {
    let fitted = fit(Rect::new(0, 0, 1920, 1080), Rect::new(0, 0, 3840, 2160));
    if fitted != Rect::new(0, 0, 3840, 2160) {
        return 1;
    }
    if fit(Rect::EMPTY, Rect::new(0, 0, 3840, 2160)) != Rect::EMPTY {
        return 2;
    }
    let arrangements: [&[Rect]; 2] = [
        &[
            Rect::new(0, 0, 3840, 2160),
            Rect::new(610, -1440, 2560, 1440),
            Rect::new(3840, 473, 1440, 2560),
        ],
        &[
            Rect::new(-2560, -1440, 2560, 1440),
            Rect::new(0, 0, 3840, 2160),
        ],
    ];
    for monitors in arrangements {
        let off = offscreen_bounds(
            Size {
                width: 3840,
                height: 2160,
            },
            monitors,
        );
        if monitors.iter().any(|m| m.intersects(&off))
            || off.size()
                != (Size {
                    width: 3840,
                    height: 2160,
                })
        {
            return 3;
        }
    }
    logger.log("Geometry and multi-monitor source-isolation checks passed.");
    0
}

/// `--check-updates`: one blocking query, a line of output and an exit code
/// scripts can act on. The GUI performs the same check on its own thread.
fn check_updates_cli(logger: &Logger) -> i32 {
    use any2odyssey_core::update::{exit_code, Comparison};
    let running = any2odyssey_core::VERSION;
    let (message, code) = match platform::check_updates() {
        Ok(Comparison::UpToDate) => (
            format!("{APP_NAME} {running} is up to date."),
            exit_code::UP_TO_DATE,
        ),
        Ok(Comparison::AheadOfRelease) => (
            format!("{APP_NAME} {running} is newer than the latest published release."),
            exit_code::UP_TO_DATE,
        ),
        Ok(Comparison::Available(release)) => (
            format!(
                "Update available: {APP_NAME} {} (running {running}). {}",
                release.version, release.page_url
            ),
            exit_code::UPDATE_AVAILABLE,
        ),
        Err(error) => (
            format!("Could not check for updates: {error}"),
            exit_code::CHECK_FAILED,
        ),
    };
    logger.log(&message);
    if code == exit_code::CHECK_FAILED {
        eprintln!("{message}");
    } else {
        println!("{message}");
    }
    code
}

fn worker_preferences(snapshot: &str) -> serde_json::Result<any2odyssey_core::prefs::Preferences> {
    let mut preferences: any2odyssey_core::prefs::Preferences = serde_json::from_str(snapshot)?;
    preferences.sanitize();
    Ok(preferences)
}

/// Release builds are GUI-subsystem processes with no console, so `println!`
/// from the terminal modes would be discarded. Attaching to the caller's
/// console makes `--check-updates` and friends print; when there is none (a
/// double-click, or a debug build that already has one) the call fails and
/// nothing changes.
#[cfg(windows)]
fn attach_parent_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}
#[cfg(not(windows))]
fn attach_parent_console() {}

/// A launch that loses the single-instance race may find the winner holding
/// the mutex but not yet owning its control events, so a request handed over
/// too early would be lost. Knock a few times before giving up.
const SIGNAL_ATTEMPTS: u32 = 5;
const SIGNAL_RETRY_PAUSE: Duration = Duration::from_millis(50);

/// Run `attempt` until it succeeds or `attempts` tries are used up, pausing
/// between tries (never after the last).
fn retry(attempts: u32, pause: Duration, mut attempt: impl FnMut() -> bool) -> bool {
    for n in 0..attempts {
        if attempt() {
            return true;
        }
        if n + 1 < attempts {
            std::thread::sleep(pause);
        }
    }
    false
}

/// Hand the request this launch carried to the instance that owns the
/// controls. Exit 1 when it never accepted it, so a script can tell the
/// request was lost rather than delivered.
fn signal_running_instance(event: &str, logger: &Logger) -> i32 {
    if retry(SIGNAL_ATTEMPTS, SIGNAL_RETRY_PAUSE, || {
        platform::signal(event)
    }) {
        0
    } else {
        logger.log(&format!(
            "Another instance owns the controls but did not accept {event} after {SIGNAL_ATTEMPTS} attempts."
        ));
        1
    }
}

/// A usage error: say what was expected and exit 2. The instance commands
/// (`--toggle`, `--show`, `--quit`) and `--worker` are not terminal modes, so
/// a release build has no console yet when one of them is misspelled or given
/// a stray argument; attach before printing, or the text goes nowhere.
/// Attaching twice is harmless: the second call fails and changes nothing.
fn usage_error() -> i32 {
    attach_parent_console();
    eprintln!("{}", usage());
    2
}

/// The command line as text. Every argument this program takes is an option
/// word, a number, an event name, JSON or a path it prints back, so one that
/// is not Unicode is a usage error rather than something to pass on lossily.
/// `Err` carries the 1-based position of the first such argument.
fn unicode_args(args: impl Iterator<Item = std::ffi::OsString>) -> Result<Vec<String>, usize> {
    args.enumerate()
        .map(|(index, arg)| arg.into_string().map_err(|_| index + 1))
        .collect()
}

/// An output path may not look like an option: `--export-guide --dark` once
/// wrote a file named `--dark`.
fn option_like(path: &str) -> bool {
    path.starts_with("--")
}

/// Modes that talk to a terminal rather than to the running instance.
fn is_terminal_mode(args: &[String]) -> bool {
    !matches!(
        args.first().map(String::as_str),
        None | Some("--worker") | Some("--toggle") | Some("--show") | Some("--quit")
    )
}

/// `--screenshot FILE [--dark] [--help-page]` with nothing else: the output
/// path may not look like an option, and unknown words are an error rather
/// than silently ignored.
fn screenshot_request(path: &str, rest: &[String]) -> Option<core::ScreenshotRequest> {
    if option_like(path) || !rest.iter().all(|a| a == "--dark" || a == "--help-page") {
        return None;
    }
    Some(core::ScreenshotRequest {
        path: path.into(),
        dark: rest.iter().any(|a| a == "--dark"),
        help_page: rest.iter().any(|a| a == "--help-page"),
    })
}

fn main() {
    // `std::env::args` panics on an argument that is not Unicode.
    let args = match unicode_args(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(position) => {
            attach_parent_console();
            eprintln!("Argument {position} is not valid Unicode.\n");
            std::process::exit(usage_error());
        }
    };
    if is_terminal_mode(&args) {
        attach_parent_console();
    }
    // An isolated preview leaves the real data directory alone, log included.
    // Its log name carries the process id, so two previews never share a
    // file and a fixed name in a shared temp folder is never reused.
    let preview = args.first().is_some_and(|a| a == "--screenshot");
    let logger = if preview {
        Logger::new(
            std::env::temp_dir().join(format!("any2odyssey-preview-{}.log", std::process::id())),
        )
    } else {
        let _ = std::fs::create_dir_all(paths::data_dir());
        Logger::default_location()
    };
    let code = match args.as_slice() {
        [flag, event, parent, source, snapshot] if flag == "--worker" => {
            let parent: u32 = parent.parse().unwrap_or(0);
            let source: isize = source.parse().unwrap_or(0);
            match worker_preferences(snapshot) {
                Ok(preferences) => platform::run_worker(event, parent, source, preferences),
                Err(error) => {
                    logger.log(&format!("Invalid worker preferences: {error}"));
                    any2odyssey_core::status::exit_code::RENDERER_ERROR
                }
            }
        }
        // A process id that does not parse is a usage error, not PID 0: with 0
        // the probes ran against nothing and reported success.
        [flag, pid] if flag == "--inspect" => match pid.parse() {
            Ok(pid) => platform::inspect(pid),
            Err(_) => usage_error(),
        },
        [flag, parent, worker] if flag == "--layout-probe" => {
            match (parent.parse(), worker.parse()) {
                (Ok(parent), Ok(worker)) => platform::layout_probe(parent, worker),
                _ => usage_error(),
            }
        }
        [flag] if flag == "--test-capture-exclusion" => platform::test_capture_exclusion(),
        [flag] if flag == "--test-offscreen" => platform::test_offscreen(),
        [flag] if flag == "--self-test" => self_test(&logger),
        [flag] if flag == "--check-updates" => check_updates_cli(&logger),
        [flag, path] if flag == "--export-guide" && option_like(path) => usage_error(),
        [flag, path] if flag == "--export-guide" => match guide::export(std::path::Path::new(path))
        {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        // Exit 1 when nothing was running to receive the request, so scripts
        // can tell "quit delivered" from "no instance".
        [flag] if flag == "--quit" => {
            if platform::signal(ipc::QUIT_EVENT) {
                0
            } else {
                1
            }
        }
        [flag] if flag == "--show" && platform::signal(ipc::SHOW_EVENT) => 0,
        [flag] if flag == "--toggle" && platform::signal(ipc::TOGGLE_EVENT) => 0,
        [flag] if flag == "--toggle" || flag == "--show" => {
            run_gui(flag == "--toggle", logger, None)
        }
        [flag, path, rest @ ..] if flag == "--screenshot" => match screenshot_request(path, rest) {
            Some(request) => run_gui(false, logger, Some(request)),
            None => usage_error(),
        },
        // An image for the stereo viewer: handed to the running instance, or
        // opened in this one. The viewer is the Linux build's home page.
        #[cfg(not(windows))]
        [flag, path] if flag == "--open" && !option_like(path) => open_image(path, logger),
        #[cfg(not(windows))]
        [flag] if flag == "--install-depth-model" => match depth::install_model() {
            Ok(path) => {
                println!("Depth model installed: {}", path.display());
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        // What a desktop entry's `%f` and "Open with" pass: the bare path.
        #[cfg(not(windows))]
        [path] if !path.starts_with('-') => open_image(path, logger),
        [] => run_gui(false, logger, None),
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", usage());
            0
        }
        _ => usage_error(),
    };
    std::process::exit(code);
}

/// Open `path` in the stereo viewer: in the running instance when there is
/// one, otherwise in this launch.
#[cfg(not(windows))]
fn open_image(path: &str, logger: Logger) -> i32 {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.into());
    if platform::signal_open(&path) {
        0
    } else {
        platform::queue_open(path);
        run_gui(false, logger, None)
    }
}

fn run_gui(
    start_requested: bool,
    logger: Logger,
    screenshot: Option<core::ScreenshotRequest>,
) -> i32 {
    if let Some(request) = screenshot {
        // Isolated preview: no single-instance guard, no events, no worker, no saved preferences.
        let mut core = core::Core::new_preview(logger);
        core.ui.dark_mode = request.dark;
        core.screenshot = Some(request);
        let core = Arc::new(Mutex::new(core));
        let run = ui::run(core.clone());
        let outcome = core
            .lock()
            .map(|c| c.screenshot_outcome.clone())
            .unwrap_or(None);
        // A preview that produced no image is a failure, so a docs or CI step
        // regenerating screenshots cannot pass with stale or missing files.
        return match (run, outcome) {
            (Ok(()), Some(Ok(()))) => 0,
            (Ok(()), Some(Err(e))) => {
                eprintln!("{e}");
                1
            }
            (Ok(()), None) => {
                eprintln!("Screenshot was not delivered.");
                1
            }
            (Err(e), _) => {
                eprintln!("{e}");
                1
            }
        };
    }
    let Some(_guard) = platform::single_instance() else {
        // Another instance owns the controls: hand it the request this launch
        // carried, so `--toggle` losing the race still toggles.
        return signal_running_instance(
            if start_requested {
                ipc::TOGGLE_EVENT
            } else {
                ipc::SHOW_EVENT
            },
            &logger,
        );
    };
    let events = match platform::ControlEvents::create() {
        Ok(e) => Some(e),
        Err(e) => {
            logger.log(&format!("Control events unavailable: {e}"));
            None
        }
    };
    let core = Arc::new(Mutex::new(core::Core::new(logger, events, start_requested)));
    match ui::run(core) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod argument_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn screenshot_arguments_reject_option_like_paths_and_unknown_words() {
        let ok = screenshot_request("out.png", &args(&["--dark", "--help-page"])).unwrap();
        assert!(ok.dark && ok.help_page);
        assert_eq!(ok.path, std::path::PathBuf::from("out.png"));
        assert!(!screenshot_request("out.png", &[]).unwrap().dark);
        assert!(screenshot_request("--dark", &[]).is_none());
        assert!(screenshot_request("out.png", &args(&["--light"])).is_none());
    }

    #[test]
    fn output_paths_may_not_look_like_options() {
        assert!(option_like("--dark"));
        assert!(option_like("--guide.html"));
        assert!(!option_like("guide.html"));
        assert!(!option_like("-guide.html"), "a single dash is a file name");
        assert!(!option_like("./--dark"));
    }

    #[test]
    fn arguments_that_are_not_unicode_are_reported_by_position() {
        use std::ffi::OsString;
        let good = vec![OsString::from("--export-guide"), OsString::from("g.html")];
        assert_eq!(
            unicode_args(good.into_iter()),
            Ok(args(&["--export-guide", "g.html"]))
        );
        #[cfg(unix)]
        let broken = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0xff, b'.', b'h', b't', b'm', b'l'])
        };
        #[cfg(windows)]
        let broken = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0xD800, u16::from(b'.'), u16::from(b'h')])
        };
        #[cfg(any(unix, windows))]
        assert_eq!(
            unicode_args(vec![OsString::from("--export-guide"), broken].into_iter()),
            Err(2)
        );
    }

    #[test]
    fn usage_names_every_instance_command_and_help() {
        let text = usage();
        for word in ["--toggle", "--show", "--quit", "--help", "--export-guide"] {
            assert!(text.contains(word), "{word}");
        }
        assert!(text.contains("or open them when no instance is running"));
    }

    #[test]
    fn retry_stops_at_the_first_success_or_after_the_last_attempt() {
        let mut calls = 0;
        assert!(retry(5, Duration::ZERO, || {
            calls += 1;
            calls == 3
        }));
        assert_eq!(calls, 3);
        let mut calls = 0;
        assert!(!retry(5, Duration::ZERO, || {
            calls += 1;
            false
        }));
        assert_eq!(calls, 5);
        assert!(!retry(0, Duration::ZERO, || panic!(
            "no attempt was asked for"
        )));
    }

    #[test]
    fn terminal_modes_exclude_the_gui_and_instance_commands() {
        for gui in [
            &[][..],
            &["--toggle"],
            &["--show"],
            &["--quit"],
            &["--worker", "e"],
        ] {
            assert!(!is_terminal_mode(&args(gui)), "{gui:?}");
        }
        for terminal in [
            &["--check-updates"][..],
            &["--self-test"],
            &["--export-guide", "x.html"],
            &["--screenshot", "x.png"],
            &["--help"],
            &["--bogus"],
        ] {
            assert!(is_terminal_mode(&args(terminal)), "{terminal:?}");
        }
    }
}

#[cfg(test)]
mod worker_snapshot_tests {
    use super::*;
    use any2odyssey_core::prefs::{CaptureMode, Preferences};

    #[test]
    fn launch_snapshot_preserves_scope_without_reading_settings() {
        let mut prefs = Preferences {
            mode: CaptureMode::Window,
            side_by_side: true,
            depth: 1.25,
            ..Default::default()
        };
        let snapshot = serde_json::to_string(&prefs).unwrap();
        prefs.mode = CaptureMode::Desktop;
        assert_eq!(prefs.mode, CaptureMode::Desktop);
        let launched = worker_preferences(&snapshot).unwrap();
        assert_eq!(launched.mode, CaptureMode::Window);
        assert!(launched.side_by_side);
        assert_eq!(launched.depth, 1.25);
        for invalid in ["not json", "null", r#"{"Mode":"Unknown"}"#] {
            assert!(worker_preferences(invalid).is_err(), "{invalid}");
        }
    }
}
