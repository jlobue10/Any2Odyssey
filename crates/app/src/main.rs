//! Any2Odyssey entry point: desktop controls by default, or one of the
//! command-line modes used by the installer, the worker and diagnostics.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod core;
mod guide;
mod hotkeys;
mod platform;
#[cfg(windows)]
mod tray;
mod ui;

use std::sync::{Arc, Mutex};

use any2odyssey_core::geometry::{fit, offscreen_bounds, Rect, Size};
use any2odyssey_core::log::Logger;
use any2odyssey_core::{ipc, paths, APP_NAME};

fn usage() -> String {
    format!(
        "{APP_NAME} {}\n\nUsage:\n  Any2Odyssey                      open the controls\n  Any2Odyssey --toggle             turn 3D on/off in the running instance (or start one and turn 3D on)\n  Any2Odyssey --show               show the running controls\n  Any2Odyssey --quit               ask the running instance to quit\n  Any2Odyssey --self-test          geometry and source-isolation checks\n  Any2Odyssey --export-guide FILE  write the offline HTML guide\n  Any2Odyssey --screenshot FILE [--dark] [--help-page]   render the controls in an isolated preview to a PNG\n  Any2Odyssey --worker EVENT PARENT_PID SOURCE_HWND   (used internally)\n  Any2Odyssey --inspect PID | --layout-probe PARENT WORKER | --test-capture-exclusion | --test-offscreen",
        any2odyssey_core::VERSION
    )
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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let _ = std::fs::create_dir_all(paths::data_dir());
    let logger = Logger::default_location();
    let code = match args.as_slice() {
        [flag, event, parent, source] if flag == "--worker" => {
            let parent: u32 = parent.parse().unwrap_or(0);
            let source: isize = source.parse().unwrap_or(0);
            platform::run_worker(event, parent, source)
        }
        [flag, pid] if flag == "--inspect" => platform::inspect(pid.parse().unwrap_or(0)),
        [flag, parent, worker] if flag == "--layout-probe" => {
            platform::layout_probe(parent.parse().unwrap_or(0), worker.parse().unwrap_or(0))
        }
        [flag] if flag == "--test-capture-exclusion" => platform::test_capture_exclusion(),
        [flag] if flag == "--test-offscreen" => platform::test_offscreen(),
        [flag] if flag == "--self-test" => self_test(&logger),
        [flag, path] if flag == "--export-guide" => match guide::export(std::path::Path::new(path))
        {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        [flag] if flag == "--quit" => {
            platform::signal(ipc::QUIT_EVENT);
            0
        }
        [flag] if flag == "--show" && platform::signal(ipc::SHOW_EVENT) => 0,
        [flag] if flag == "--toggle" && platform::signal(ipc::TOGGLE_EVENT) => 0,
        [flag] if flag == "--toggle" || flag == "--show" => {
            run_gui(flag == "--toggle", logger, None)
        }
        [flag, path, rest @ ..] if flag == "--screenshot" => run_gui(
            false,
            logger,
            Some(core::ScreenshotRequest {
                path: path.into(),
                dark: rest.iter().any(|a| a == "--dark"),
                help_page: rest.iter().any(|a| a == "--help-page"),
            }),
        ),
        [] => run_gui(false, logger, None),
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", usage());
            0
        }
        _ => {
            eprintln!("{}", usage());
            2
        }
    };
    std::process::exit(code);
}

fn run_gui(
    start_requested: bool,
    logger: Logger,
    screenshot: Option<core::ScreenshotRequest>,
) -> i32 {
    if let Some(request) = screenshot {
        // Isolated preview: no single-instance guard, no events, no worker, no saved preferences.
        let mut core = core::Core::new(logger, None, false);
        core.preview = true;
        core.ui.dark_mode = request.dark;
        core.screenshot = Some(request);
        return match ui::run(Arc::new(Mutex::new(core))) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }
    let Some(_guard) = platform::single_instance() else {
        // Another instance owns the controls: bring it forward instead.
        platform::signal(ipc::SHOW_EVENT);
        return 0;
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
