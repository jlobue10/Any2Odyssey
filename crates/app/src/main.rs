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
    usage_for(cfg!(windows))
}

/// The help text of the Windows build (`windows`) or of the others. Each
/// names its own modes only: the conversion worker, the update check and
/// the capture diagnostics exist on Windows alone, and the viewer's image
/// argument and the depth model elsewhere.
fn usage_for(windows: bool) -> String {
    let mut text = format!(
        "{APP_NAME} {}\n\nUsage:\n  Any2Odyssey                      open the controls\n  Any2Odyssey --toggle             turn 3D on/off in the running instance (or start one and turn 3D on)\n  Any2Odyssey --show               show the running controls (or open them when no instance is running)\n  Any2Odyssey --quit               ask the running instance to quit (exit 1 when none is running)\n  Any2Odyssey --help               print this text\n  Any2Odyssey --self-test          geometry and source-isolation checks",
        any2odyssey_core::VERSION
    );
    if windows {
        text.push_str("\n  Any2Odyssey --check-updates      ask GitHub for the newest release (exit 0 current, 10 update available, 1 error)");
    }
    text.push_str("\n  Any2Odyssey --export-guide FILE  write the offline HTML guide\n  Any2Odyssey --screenshot FILE [--dark] [--help-page]   render the controls in an isolated preview to a PNG");
    if windows {
        text.push_str("\n  Any2Odyssey --worker EVENT PARENT_PID SOURCE_HWND PREFERENCES_JSON   (used internally)\n  Any2Odyssey --inspect PID | --layout-probe PARENT WORKER | --test-capture-exclusion | --test-offscreen");
    } else {
        text.push_str("\n  Any2Odyssey [--open] FILE         open an image in the stereo viewer (in the running instance when there is one)");
        text.push_str("\n  Any2Odyssey --install-depth-model  download the AI depth model for 2D → 3D (about 95 MB, needs curl)");
    }
    text
}

/// Print a line of this program's answer. Unlike `println!` it does not
/// panic when the line cannot be written: `--help | true` ended with a
/// panic message and exit code 101 where the reader had gone, and a usage
/// error with 101 instead of 2 where standard error could not be written.
fn say(text: &str) {
    write_line(std::io::stdout().lock(), text);
}

/// [`say`] for standard error.
fn complain(text: &str) {
    write_line(std::io::stderr().lock(), text);
}

fn write_line(mut to: impl std::io::Write, text: &str) {
    let _ = writeln!(to, "{text}").and_then(|()| to.flush());
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
        complain(&message);
    } else {
        say(&message);
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

/// `--quit`: ask the instance that owns the controls to leave. It may hold
/// its mutex and not yet own its control events (it was started a moment
/// ago), so this knocks as often as a launch that lost the race does. Exit 1
/// when nobody accepted the request: nothing is running.
fn quit_running_instance(mut signal: impl FnMut(&str) -> bool) -> i32 {
    if retry(SIGNAL_ATTEMPTS, SIGNAL_RETRY_PAUSE, || {
        signal(ipc::QUIT_EVENT)
    }) {
        0
    } else {
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
    complain(&usage());
    2
}

/// The command line as text. Every argument this program takes is an option
/// word, a number, an event name, JSON or a path it prints back, so one that
/// is not Unicode is a usage error rather than something to pass on lossily.
/// (The image for the stereo viewer is the exception and is taken out
/// first: `image_argument`.) `Err` carries the 1-based position of the
/// first such argument.
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

/// The image a launch names for the stereo viewer, `FILE` or `--open FILE`
/// (what a desktop entry's `%f` and "Open with" pass is the bare path). A
/// file's name is whatever bytes the file system holds, so this one argument
/// is taken as it is and never has to be Unicode; the option word does, and
/// so does every other argument ([`unicode_args`]). As elsewhere, a path
/// after `--open` may not look like an option, and a bare one may not start
/// with a dash.
#[cfg(not(windows))]
fn image_argument(args: &[std::ffi::OsString]) -> Option<&std::ffi::OsStr> {
    let starts_with =
        |arg: &std::ffi::OsStr, prefix: &str| arg.as_encoded_bytes().starts_with(prefix.as_bytes());
    match args {
        [flag, path] if flag == "--open" && !starts_with(path, "--") => Some(path),
        [path] if !starts_with(path, "-") => Some(path),
        _ => None,
    }
}

/// Whether the command only answers or asks the running instance, and so
/// leaves the data folder alone: `--help` and `--quit` made the folder on a
/// machine the program had never run on.
fn leaves_the_data_folder_alone(args: &[String]) -> bool {
    matches!(
        args.first().map(String::as_str),
        Some("--help" | "-h" | "--quit" | "--export-guide")
    )
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

/// Allocations of this size and above are given back to the system when
/// they are freed (see [`return_freed_pictures`]). Pictures and what is
/// made of them are larger; what a frame of the page or one run of the
/// depth model allocates is smaller and stays with the allocator.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const RETURNED_WHEN_FREED: std::ffi::c_int = 4 * 1024 * 1024;

/// Have glibc give the memory of a freed picture back to the system.
///
/// Left to itself it raises, at the first large buffer freed, the size from
/// which a buffer is a mapping of its own to that buffer's size (up to
/// 32 MiB), and with it the amount of free memory an arena keeps. From then
/// on every thread that has held a picture (the one reading a file, each
/// conversion worker) keeps its share for good: the viewer, showing a small
/// image again, stayed about 200 MB larger for every 8K-wide pair it had
/// opened, and was at 1.7 GB after a thousand images. Naming the size
/// switches that adjustment off. `false` when the library refused, which
/// changes nothing else.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn return_freed_pictures() -> bool {
    extern "C" {
        fn mallopt(parameter: std::ffi::c_int, value: std::ffi::c_int) -> std::ffi::c_int;
    }
    const M_MMAP_THRESHOLD: std::ffi::c_int = -3;
    // SAFETY: two numbers; sets a field of the allocator under its own lock.
    unsafe { mallopt(M_MMAP_THRESHOLD, RETURNED_WHEN_FREED) == 1 }
}

fn main() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    return_freed_pictures();
    // `std::env::args` panics on an argument that is not Unicode.
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // An image for the stereo viewer is the one argument taken as it is.
    #[cfg(not(windows))]
    let image = image_argument(&raw).map(std::path::PathBuf::from);
    #[cfg(not(windows))]
    let raw = if image.is_some() { Vec::new() } else { raw };
    let args = match unicode_args(raw.into_iter()) {
        Ok(args) => args,
        Err(position) => {
            attach_parent_console();
            complain(&format!("Argument {position} is not valid Unicode.\n"));
            std::process::exit(usage_error());
        }
    };
    if is_terminal_mode(&args) {
        attach_parent_console();
    }
    // An isolated preview leaves the real data directory alone, log included.
    let preview = args.first().is_some_and(|a| a == "--screenshot");
    let logger = if preview {
        preview_logger()
    } else if leaves_the_data_folder_alone(&args) {
        Logger::default_location()
    } else {
        let _ = std::fs::create_dir_all(paths::data_dir());
        let logger = Logger::default_location();
        logger.rotate_if_large();
        logger
    };
    // Handed to the running instance, or opened in this one. The viewer is
    // the Linux build's home page.
    #[cfg(not(windows))]
    if let Some(image) = image {
        std::process::exit(open_image(&image, logger));
    }
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
                complain(&e.to_string());
                1
            }
        },
        // Exit 1 when nothing was running to receive the request, so scripts
        // can tell "quit delivered" from "no instance".
        [flag] if flag == "--quit" => quit_running_instance(platform::signal),
        [flag] if flag == "--show" && platform::signal(ipc::SHOW_EVENT) => 0,
        [flag] if flag == "--toggle" && platform::signal(ipc::TOGGLE_EVENT) => 0,
        [flag] if flag == "--toggle" || flag == "--show" => {
            run_gui(flag == "--toggle", logger, None)
        }
        [flag, path, rest @ ..] if flag == "--screenshot" => match screenshot_request(path, rest) {
            Some(request) => run_gui(false, logger, Some(request)),
            None => usage_error(),
        },
        #[cfg(not(windows))]
        [flag] if flag == "--install-depth-model" => match depth::install_model() {
            Ok(path) => {
                say(&format!("Depth model installed: {}", path.display()));
                0
            }
            Err(e) => {
                complain(&e);
                1
            }
        },
        [] => run_gui(false, logger, None),
        [flag] if flag == "--help" || flag == "-h" => {
            say(&usage());
            0
        }
        _ => usage_error(),
    };
    std::process::exit(code);
}

/// The log of an isolated preview (`--screenshot`): one file in a new folder
/// of its own in the temporary folder. The folder is made for this run and
/// this user only, so nothing can be waiting there under the log's name (a
/// link to a file to append to, in a temporary folder everyone shares). It
/// is left behind for whoever wants to read the log.
fn preview_logger() -> Logger {
    let mut builder = tempfile::Builder::new();
    builder.prefix("any2odyssey-preview-");
    // Asked for: a new folder is otherwise as open as the umask leaves it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let folder = builder.tempdir().map(tempfile::TempDir::keep);
    match folder {
        Ok(folder) => Logger::new(folder.join("preview.log")),
        // No folder, no log: an empty path is one nothing can be written to.
        Err(_) => Logger::new(std::path::PathBuf::new()),
    }
}

/// Open `path` in the stereo viewer: in the running instance when there is
/// one, otherwise in this launch.
#[cfg(not(windows))]
fn open_image(path: &std::path::Path, logger: Logger) -> i32 {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.into());
    if platform::signal_open(&path) {
        0
    } else {
        platform::queue_open(path);
        run_gui(false, logger, None)
    }
}

/// Hand every image in `images` to the instance that owns the controls,
/// knocking as often as for the other requests. False as soon as one was
/// not accepted.
#[cfg(not(windows))]
fn hand_over_images(
    images: &[std::path::PathBuf],
    mut signal: impl FnMut(&std::path::Path) -> bool,
) -> bool {
    images
        .iter()
        .all(|image| retry(SIGNAL_ATTEMPTS, SIGNAL_RETRY_PAUSE, || signal(image)))
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
                complain(&e);
                1
            }
            (Ok(()), None) => {
                complain("Screenshot was not delivered.");
                1
            }
            (Err(e), _) => {
                complain(&e.to_string());
                1
            }
        };
    }
    let Some(_guard) = platform::single_instance() else {
        // Another instance owns the controls: hand it the request this launch
        // carried, so `--toggle` losing the race still toggles.
        #[cfg(not(windows))]
        {
            // So does an image: nobody answered when this launch first
            // asked, and another launch has become the instance since.
            let images = platform::take_open_requests();
            if !images.is_empty() {
                if hand_over_images(&images, platform::signal_open) {
                    return 0;
                }
                logger.log(&format!(
                    "Another instance owns the controls but did not accept the image to open after {SIGNAL_ATTEMPTS} attempts."
                ));
                return 1;
            }
        }
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
            complain(&e.to_string());
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

    #[cfg(unix)]
    #[test]
    fn an_image_to_open_keeps_its_bytes_and_a_broken_option_is_still_an_error() {
        use std::ffi::{OsStr, OsString};
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let os = |bytes: &[u8]| OsString::from_vec(bytes.to_vec());
        let name = os(b"/pictures/pair-\xff\xfe.png");
        // Bare, as a desktop entry's `%f` passes it, and after `--open`.
        assert_eq!(
            image_argument(std::slice::from_ref(&name)),
            Some(name.as_os_str())
        );
        assert_eq!(
            image_argument(&[os(b"--open"), name.clone()]),
            Some(name.as_os_str())
        );
        // The bytes reach the instance socket's request unchanged.
        let image = std::path::PathBuf::from(image_argument(std::slice::from_ref(&name)).unwrap());
        assert_eq!(image.as_os_str().as_bytes(), b"/pictures/pair-\xff\xfe.png");
        // Ordinary names as before, relative ones and single dashes inside.
        for plain in ["pair.png", "./--dark", "a-b.png", ""] {
            assert_eq!(
                image_argument(&[os(plain.as_bytes())]),
                Some(OsStr::new(plain))
            );
        }
        assert_eq!(
            image_argument(&[os(b"--open"), os(b"-pair.png")]),
            Some(OsStr::new("-pair.png")),
            "a single dash is a file name after --open"
        );

        // Not an image: options, option-like paths, and anything longer.
        for other in [
            &[][..],
            &[os(b"--show")],
            &[os(b"-h")],
            &[os(b"-\xff.png")],
            &[os(b"--open")],
            &[os(b"--open"), os(b"--dark")],
            &[os(b"--open"), os(b"--\xff")],
            &[os(b"--open"), name.clone(), os(b"extra")],
            &[os(b"--export-guide"), name.clone()],
            &[os(b"--op\xffn"), os(b"pair.png")],
            &[name.clone(), name.clone()],
        ] {
            assert_eq!(image_argument(other), None, "{other:?}");
        }
        // And those are still refused by position when they are not Unicode.
        assert_eq!(
            unicode_args([os(b"--export-guide"), name.clone()].into_iter()),
            Err(2)
        );
        assert_eq!(
            unicode_args([os(b"--op\xffn"), os(b"pair.png")].into_iter()),
            Err(1)
        );
        assert_eq!(unicode_args([os(b"-\xff.png")].into_iter()), Err(1));
        assert_eq!(
            unicode_args([os(b"--open"), os(b"--\xff")].into_iter()),
            Err(2)
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn images_of_a_launch_that_lost_the_race_are_handed_to_the_winner() {
        let images = [
            std::path::PathBuf::from("/a.png"),
            std::path::PathBuf::from("/b.png"),
        ];
        // The winner does not answer at once: knocked again, in order.
        let mut calls = Vec::new();
        assert!(hand_over_images(&images, |image| {
            calls.push(image.to_owned());
            calls.len() != 1
        }));
        assert_eq!(
            calls,
            [images[0].clone(), images[0].clone(), images[1].clone()]
        );
        // Never accepted: reported, after the usual number of attempts.
        let mut attempts = 0;
        assert!(!hand_over_images(&images, |_| {
            attempts += 1;
            false
        }));
        assert_eq!(attempts, SIGNAL_ATTEMPTS);
        assert!(hand_over_images(&[], |_| panic!("nothing to hand over")));
    }

    #[test]
    fn a_preview_logs_into_a_folder_made_for_it() {
        let logger = preview_logger();
        let folder = logger.path().parent().unwrap().to_owned();
        assert!(folder.starts_with(std::env::temp_dir()));
        assert_eq!(logger.path().file_name().unwrap(), "preview.log");
        assert!(folder.is_dir() && !logger.path().exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&folder).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "nobody else can put a name there");
        }
        logger.log("preview");
        assert!(std::fs::read_to_string(logger.path())
            .unwrap()
            .ends_with("preview\n"));
        // Two previews never share a log.
        let other = preview_logger();
        assert_ne!(other.path(), logger.path());
        for made in [folder.as_path(), other.path().parent().unwrap()] {
            std::fs::remove_dir_all(made).unwrap();
        }
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
    fn each_platforms_help_names_its_own_modes_only() {
        let (windows, others) = (usage_for(true), usage_for(false));
        let own = if cfg!(windows) { &windows } else { &others };
        assert_eq!(&usage(), own);
        let windows_only = [
            "--worker",
            "--inspect",
            "--layout-probe",
            "--test-capture-exclusion",
            "--test-offscreen",
            "--check-updates",
        ];
        for word in windows_only {
            assert!(windows.contains(word), "{word}");
            assert!(!others.contains(word), "{word}");
        }
        for word in ["--open", "--install-depth-model"] {
            assert!(!windows.contains(word), "{word}");
            assert!(others.contains(word), "{word}");
        }
        for word in [
            "--toggle",
            "--show",
            "--quit",
            "--help",
            "--self-test",
            "--export-guide",
            "--screenshot",
        ] {
            assert!(windows.contains(word) && others.contains(word), "{word}");
        }
        // One mode a line, none of them run together.
        for text in [&windows, &others] {
            assert!(text.lines().skip(3).all(|l| l.starts_with("  Any2Odyssey")));
        }
    }

    #[test]
    fn a_line_that_cannot_be_written_is_not_a_panic() {
        struct Gone;
        impl std::io::Write for Gone {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
        }
        write_line(Gone, &usage());
        let mut written = Vec::new();
        write_line(&mut written, "two\nlines");
        assert_eq!(written, b"two\nlines\n");
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
    fn quit_knocks_again_while_the_instance_is_not_yet_listening() {
        // The instance owns its control events from the third knock on.
        let mut knocks = Vec::new();
        assert_eq!(
            quit_running_instance(|event| {
                knocks.push(event.to_owned());
                knocks.len() == 3
            }),
            0
        );
        assert_eq!(knocks, [ipc::QUIT_EVENT; 3]);
        // Nothing is running: said so, after the usual number of attempts.
        let mut attempts = 0;
        assert_eq!(
            quit_running_instance(|_| {
                attempts += 1;
                false
            }),
            1
        );
        assert_eq!(attempts, SIGNAL_ATTEMPTS);
    }

    #[test]
    fn answering_and_asking_commands_leave_the_data_folder_alone() {
        for quiet in [
            &["--help"][..],
            &["-h"],
            &["--quit"],
            &["--export-guide", "g.html"],
        ] {
            assert!(leaves_the_data_folder_alone(&args(quiet)), "{quiet:?}");
        }
        // Everything that logs, keeps settings or becomes the instance.
        for keeps in [
            &[][..],
            &["--show"],
            &["--toggle"],
            &["--self-test"],
            &["--check-updates"],
            &["--worker", "e", "1", "2", "{}"],
            &["--install-depth-model"],
        ] {
            assert!(!leaves_the_data_folder_alone(&args(keeps)), "{keeps:?}");
        }
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

#[cfg(all(test, target_os = "linux", target_env = "gnu"))]
mod allocator_tests {
    use super::*;

    /// Resident memory of this process in bytes.
    fn resident() -> usize {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let line = status.lines().find(|l| l.starts_with("VmRSS:")).unwrap();
        let kib: usize = line.split_whitespace().nth(1).unwrap().parse().unwrap();
        kib * 1024
    }

    /// Eight threads each hold a buffer of a picture's size at the same
    /// time, as the conversion's workers do, and free it. Measured in a
    /// process of its own (this test, started once more): other tests'
    /// memory would be in the figure otherwise, and the allocator's setting
    /// is one per process.
    #[test]
    fn memory_of_freed_pictures_goes_back_to_the_system() {
        const CHILD: &str = "ANY2ODYSSEY_TEST_ALLOCATOR_CHILD";
        const PICTURE: usize = 24 * 1024 * 1024;
        const HOLDERS: usize = 8;
        if std::env::var_os(CHILD).is_some() {
            assert!(return_freed_pictures());
            // The first one freed is what sets the sizes when left alone.
            drop(std::hint::black_box(vec![1u8; PICTURE]));
            let before = resident();
            for _ in 0..3 {
                let barrier = std::sync::Barrier::new(HOLDERS);
                std::thread::scope(|scope| {
                    for _ in 0..HOLDERS {
                        scope.spawn(|| {
                            let picture = std::hint::black_box(vec![1u8; PICTURE]);
                            barrier.wait();
                            drop(picture);
                        });
                    }
                });
            }
            let kept = resident().saturating_sub(before);
            // Left alone, every holder's arena keeps its buffer: 192 MiB.
            assert!(
                kept < 2 * PICTURE,
                "{} MiB of freed pictures stayed with the process",
                kept / (1024 * 1024)
            );
            return;
        }
        let this_test = format!(
            "{}::memory_of_freed_pictures_goes_back_to_the_system",
            module_path!().split_once("::").unwrap().1
        );
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &this_test, "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(
            String::from_utf8_lossy(&child.stdout).contains("1 passed"),
            "the measurement did not run"
        );
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
