//! The program's terminal modes when their output cannot be written.
//!
//! A mode that reports with `println!` or `eprintln!` panics when the line
//! cannot be written, and ends with exit code 101 and a panic message where
//! it should have ended with its own code. Each mode here only prints and
//! leaves: none opens a window or reaches a running instance.

#![cfg(unix)]

use std::process::{Command, Stdio};

/// Run the program with both outputs going to a pipe whose reader has gone,
/// and return its exit code.
fn exit_code_with_nobody_reading(arguments: &[&str]) -> Option<i32> {
    let data = tempfile::tempdir().unwrap();
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let status = Command::new(env!("CARGO_BIN_EXE_Any2Odyssey"))
        .args(arguments)
        // Its log goes to a folder of this test's own.
        .env("ANY2ODYSSEY_DATA_DIR", data.path())
        .stdin(Stdio::null())
        .stdout(writer.try_clone().unwrap())
        .stderr(writer)
        .status()
        .unwrap();
    status.code()
}

#[test]
fn a_mode_whose_output_nobody_reads_ends_with_its_own_exit_code() {
    for (arguments, code) in [
        (&["--help"][..], 0),
        (&["--no-such-mode"], 2),
        (&["--export-guide", "--dark"], 2),
        (&["--export-guide", "/dev/null/guide.html"], 1),
    ] {
        assert_eq!(
            exit_code_with_nobody_reading(arguments),
            Some(code),
            "{arguments:?}"
        );
    }
}

/// The conversion worker and the capture diagnostics exist on Windows only,
/// and say so elsewhere.
#[test]
fn a_windows_only_mode_refuses_with_exit_code_1_when_nobody_reads_it() {
    for arguments in [
        &["--inspect", "1"][..],
        &["--layout-probe", "1", "2"],
        &["--test-capture-exclusion"],
        &["--test-offscreen"],
        &["--worker", "event", "1", "2", "{}"],
        &["--toggle"],
    ] {
        assert_eq!(
            exit_code_with_nobody_reading(arguments),
            Some(1),
            "{arguments:?}"
        );
    }
}
