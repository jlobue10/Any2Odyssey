//! The two Windows probes when their report cannot be written.
//!
//! `--inspect` failed with exit code 1 and not a word about why, and
//! `--layout-probe` said why (without the file's name) with `eprintln!`, which
//! ends the program with a panic and exit code 101 where nobody reads its
//! error output. On Linux both only refuse (see `outputs.rs`); what they do
//! when they fail can be seen on Windows alone.
//!
//! Neither mode shows a window or reaches a running instance: `--inspect`
//! reads the windows of a process, and `--layout-probe` makes one window that
//! is never shown. The data folder here is a path under a file, so nothing
//! can be written anywhere.

#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

/// No console of its own for the program, whatever started the tests.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A data folder that cannot be made: the folder above it is a file.
fn unwritable_data_folder(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let file = dir.path().join("a-file");
    std::fs::write(&file, b"not a folder").unwrap();
    file.join("data")
}

fn probe(arguments: &[&str], data: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_Any2Odyssey"));
    command
        .args(arguments)
        .env("ANY2ODYSSEY_DATA_DIR", data)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

#[test]
fn a_probe_says_which_file_it_could_not_write() {
    for (arguments, report) in [
        (&["--inspect", "1"][..], "window-diagnostics.json"),
        (&["--layout-probe", "1", "2"], "projection-diagnostics.json"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let output = probe(arguments, &unwritable_data_folder(&dir))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{arguments:?}");
        let said = String::from_utf8_lossy(&output.stderr);
        // In a session without a display the layout probe has said so and
        // never comes to its report.
        let no_display = said.contains("No primary monitor");
        assert!(
            no_display || (said.contains("Could not write") && said.contains(report)),
            "{arguments:?}: {said:?}"
        );
    }
}

#[test]
fn a_probe_that_fails_ends_with_exit_code_1_when_nobody_reads_why() {
    for arguments in [&["--inspect", "1"][..], &["--layout-probe", "1", "2"]] {
        let dir = tempfile::tempdir().unwrap();
        // Both outputs go to a pipe whose reader has gone.
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let status = probe(arguments, &unwritable_data_folder(&dir))
            .stdout(writer.try_clone().unwrap())
            .stderr(writer)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(1), "{arguments:?}");
    }
}
