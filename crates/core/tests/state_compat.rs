//! What earlier releases saved, read by this one, and what this one saves,
//! compared with what earlier releases look for.
//!
//! `tests/fixtures/releases/<tag>/` holds files written by each release's
//! own code (see the README there). A user's data folder outlives every
//! update, and since 0.9.4 the setup program also goes back to an older
//! release, so a renamed key, a respelled enum value or a number whose range
//! changed is a setting silently lost in one direction or the other. These
//! tests fail in the pull request that does it.

use any2odyssey_core::about::{AboutInfo, InstallRecord, Installation};
use any2odyssey_core::fsutil;
use any2odyssey_core::geometry::Rect;
use any2odyssey_core::prefs::{CaptureMode, Preferences, UiSettings};
use any2odyssey_core::status::{CaptureBorderStatus, DesktopCaptureStatus, WorkerStatus};
use any2odyssey_core::weave::Lens;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The profile folder the installation records were made for: Latin,
/// Cyrillic and CJK letters, an apostrophe and an ampersand.
const RECORD_APP: &str =
    "C:\\Users\\Zo\u{eb} \u{416}\u{142}\u{4e2d} O'Brien & Co\\AppData\\Local\\Programs\\Any2Odyssey";
const RECORD_WORKER: &str = r"C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe";
const STATUS_TIME: &str = "2026-11-08T10:00:00.000+01:00";
const STATUS_TEXT: &str = "Stopping 3D\u{2026} \u{e9}\u{4e09}";

struct Release {
    tag: String,
    version: [u32; 3],
    dir: PathBuf,
}

impl Release {
    fn file(&self, state: &str, name: &str) -> Option<PathBuf> {
        let path = self.dir.join(state).join(name);
        path.is_file().then_some(path)
    }
}

/// The releases that have fixtures, oldest first.
fn releases() -> Vec<Release> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/releases");
    let mut releases: Vec<Release> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let tag = entry.file_name().to_string_lossy().into_owned();
            let numbers: Vec<u32> = tag
                .strip_prefix('v')
                .unwrap_or_else(|| panic!("{tag} is not a release tag"))
                .split('.')
                .map(|part| part.parse().unwrap_or_else(|_| panic!("{tag}")))
                .collect();
            Release {
                version: numbers.try_into().unwrap_or_else(|_| panic!("{tag}")),
                dir: entry.path(),
                tag,
            }
        })
        .collect();
    releases.sort_by_key(|release| release.version);
    releases
}

fn states(release: &Release, name: &str) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(&release.dir)
        .unwrap()
        .flatten()
        .filter_map(|entry| {
            let state = entry.file_name().to_string_lossy().into_owned();
            release.file(&state, name).map(|path| (state, path))
        })
        .collect();
    found.sort();
    found
}

fn json(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap();
    serde_json::from_slice(fsutil::strip_utf8_bom(&bytes))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every value in the saved `file` is the value that was `loaded`, and what
/// the file does not have (a setting newer than its release) is the default.
fn assert_kept(file: &Value, loaded: &Value, default: &Value, what: &str) {
    let (file, loaded, default) = (
        file.as_object().unwrap(),
        loaded.as_object().unwrap(),
        default.as_object().unwrap(),
    );
    for (key, value) in file {
        assert_eq!(
            loaded.get(key),
            Some(value),
            "{what}: {key} was saved as {value} and is not read back as that"
        );
    }
    for (key, value) in loaded {
        if !file.contains_key(key) {
            assert_eq!(
                Some(value),
                default.get(key),
                "{what}: {key} is not in the file and must be the default"
            );
        }
    }
}

/// As `crates/app/src/ui/lens.rs` reads `lens.json`.
fn read_lens(path: &Path) -> Result<Lens, serde_json::Error> {
    let bytes = std::fs::read(path).unwrap();
    serde_json::from_slice::<Lens>(fsutil::strip_utf8_bom(&bytes)).map(Lens::sanitized)
}

#[test]
fn the_corpus_has_every_release_up_to_a_recent_one() {
    let releases = releases();
    assert!(releases.len() >= 14, "{} releases", releases.len());
    assert_eq!(releases[0].tag, "v0.8.0");
    for release in &releases {
        for state in ["default", "all-changed"] {
            for name in ["settings.json", "ui-settings.json"] {
                assert!(
                    release.file(state, name).is_some(),
                    "{}/{state}/{name}",
                    release.tag
                );
            }
        }
        assert!(
            release.file("status", "worker-status.json").is_some(),
            "{}",
            release.tag
        );
    }
}

#[test]
fn settings_of_every_release_load_with_every_value_kept() {
    let default = serde_json::to_value(Preferences::default()).unwrap();
    for release in releases() {
        let files = states(&release, "settings.json");
        assert!(files.len() >= 5, "{}", release.tag);
        for (state, path) in files {
            let what = format!("{} {state} settings.json", release.tag);
            let loaded = Preferences::load_from(&path)
                .unwrap_or_else(|e| panic!("{what} is called damaged: {e}"));
            let file = json(&path);
            assert_kept(
                &file,
                &serde_json::to_value(&loaded).unwrap(),
                &default,
                &what,
            );
            if state == "all-changed" {
                assert_eq!(loaded.mode, CaptureMode::Active, "{what}");
                assert!(loaded.side_by_side, "{what}");
                assert!(
                    !loaded.auto_convert
                        && !loaded.minimize_to_tray
                        && !loaded.recover
                        && !loaded.keep_overlay_on_top,
                    "{what}"
                );
                assert_eq!((loaded.depth, loaded.popout), (1.37, -0.42), "{what}");
                // The update check came with 0.8.2; older files do not turn it off.
                assert_eq!(loaded.check_updates, release.version < [0, 8, 2], "{what}");
            }
            // Saved again by this release, the file still holds all of it.
            let dir = tempfile::tempdir().unwrap();
            let copy = dir.path().join("settings.json");
            let mut again = loaded.clone();
            again.save_to(&copy).unwrap();
            assert_eq!(Preferences::load_from(&copy).unwrap(), loaded, "{what}");
            assert_kept(&file, &json(&copy), &default, &what);
        }
    }
}

#[test]
fn ui_settings_of_every_release_load_with_every_value_kept() {
    let default = serde_json::to_value(UiSettings::default()).unwrap();
    for release in releases() {
        let files = states(&release, "ui-settings.json");
        assert!(files.len() >= 4, "{}", release.tag);
        for (state, path) in files {
            let what = format!("{} {state} ui-settings.json", release.tag);
            let (loaded, problem) = UiSettings::load_from(&path);
            assert_eq!(problem, None, "{what} was reset to defaults");
            let file = json(&path);
            assert_kept(
                &file,
                &serde_json::to_value(&loaded).unwrap(),
                &default,
                &what,
            );
            if state == "all-changed" {
                assert!(loaded.dark_mode, "{what}");
                let expected = (release.version >= [0, 8, 2]).then_some(1_800_000_000);
                assert_eq!(loaded.last_update_check, expected, "{what}");
            }
            let dir = tempfile::tempdir().unwrap();
            let copy = dir.path().join("ui-settings.json");
            loaded.save_to(&copy).unwrap();
            assert_eq!(UiSettings::load_from(&copy), (loaded, None), "{what}");
            assert_kept(&file, &json(&copy), &default, &what);
        }
    }
}

#[test]
fn lens_numbers_of_every_release_load_as_they_were_saved() {
    let default = serde_json::to_value(Lens::default()).unwrap();
    let mut seen = 0;
    for release in releases() {
        for (state, path) in states(&release, "lens.json") {
            seen += 1;
            let what = format!("{} {state} lens.json", release.tag);
            let loaded = read_lens(&path).unwrap_or_else(|e| panic!("{what}: {e}"));
            assert_kept(
                &json(&path),
                &serde_json::to_value(loaded).unwrap(),
                &default,
                &what,
            );
            if state == "all-changed" {
                let expected = Lens {
                    pitch: 5.4321,
                    slant: -0.2987,
                    offset: 0.75,
                    bgr: true,
                };
                assert_eq!(loaded, expected, "{what}");
            }
            // Written through the window of the published Linux program.
            if state == "linux-gui" {
                assert!(loaded.bgr, "{what}");
                assert_ne!(loaded.pitch, Lens::default().pitch, "{what}");
            }
        }
    }
    // Six releases have a lens file so far, in five states each.
    assert!(seen >= 30, "{seen} lens files");
}

#[test]
fn status_files_of_every_release_parse() {
    for release in releases() {
        let what = &release.tag;
        let read = |state: &str, name: &str| {
            std::fs::read_to_string(release.file(state, name).unwrap_or_else(|| {
                panic!("{what}/{state}/{name} is missing");
            }))
            .unwrap()
        };
        let status: WorkerStatus =
            serde_json::from_str(&read("status", "worker-status.json")).unwrap();
        assert_eq!(
            status,
            WorkerStatus {
                state: STATUS_TEXT.into(),
                time: STATUS_TIME.into(),
                pid: 4242,
                // Workers before 0.8.7 never said so, and a status that is
                // not the ready one does not either.
                ready: false,
            },
            "{what}"
        );
        if let Some(path) = release.file("status-ready", "worker-status.json") {
            let status: WorkerStatus =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            assert!(status.ready && status.pid == 4242, "{what}");
        } else {
            assert!(release.version < [0, 8, 7], "{what} has no ready status");
        }
        let capture: DesktopCaptureStatus =
            serde_json::from_str(&read("status", "desktop-capture-status.json")).unwrap();
        assert_eq!(capture, capture_status(), "{what}");
        let border: CaptureBorderStatus =
            serde_json::from_str(&read("status", "capture-border-status.json")).unwrap();
        assert_eq!(border, border_status(), "{what}");
    }
}

#[test]
fn install_records_of_every_release_load() {
    let mut seen = 0;
    for release in releases() {
        let version = release.tag.trim_start_matches('v');
        for shell in ["pwsh", "ps51"] {
            for (scenario, controls_only) in
                [("install-full", false), ("upgrade-controls-only", true)]
            {
                let path = release
                    .dir
                    .join("windows-record")
                    .join(shell)
                    .join(scenario)
                    .join("version.json");
                let what = format!("{} {shell} {scenario}", release.tag);
                let record = InstallRecord::load_from(&path)
                    .unwrap_or_else(|e| panic!("{what}: {e}"))
                    .unwrap_or_else(|| panic!("{what} is missing"));
                seen += 1;
                assert_eq!(record.version.as_deref(), Some(version), "{what}");
                assert_eq!(record.controls_only, controls_only, "{what}");
                assert_eq!(record.app.as_deref(), Some(RECORD_APP), "{what}");
                assert_eq!(
                    record.installed_display().as_deref(),
                    Some("2026-11-08 14:10"),
                    "{what}"
                );
                assert_eq!(record.hash.as_deref().map(str::len), Some(64), "{what}");
                // Up to 0.9.5 the replaced version was recorded in the four
                // parts of a Windows file version.
                let replaced = if release.version > [0, 9, 5] {
                    "0.8.0"
                } else {
                    "0.8.0.0"
                };
                let (action, previous, worker) = if controls_only {
                    ("Upgrade", Some(replaced), None)
                } else {
                    ("Install", None, Some(RECORD_WORKER))
                };
                assert_eq!(record.action.as_deref(), Some(action), "{what}");
                assert_eq!(record.previous_version.as_deref(), previous, "{what}");
                assert_eq!(record.worker.as_deref(), worker, "{what}");

                // And the About page shows it as the releases are named.
                let info = AboutInfo::assemble(version, None, None, Ok(Some(record)));
                let Installation::Installed(rows) = info.installation() else {
                    panic!("{what}: not shown as installed");
                };
                let row = |label: &str| {
                    rows.iter()
                        .find(|(name, _)| name == label)
                        .map(|(_, value)| value.as_str())
                };
                assert_eq!(row("Installed version"), Some(version), "{what}");
                assert_eq!(row("Replaced"), controls_only.then_some("0.8.0"), "{what}");
                assert_eq!(
                    row("Package"),
                    Some(if controls_only {
                        "Controls only"
                    } else {
                        "Full converter"
                    }),
                    "{what}"
                );
            }
        }
    }
    assert!(seen >= 56, "{seen} records");
}

/// A file from an earlier release that went through a Windows editor
/// ("UTF-8 with BOM", CR LF) is still that file, not a damaged one.
#[test]
fn an_older_file_resaved_by_a_windows_editor_is_not_damage() {
    let releases = releases();
    let oldest = &releases[0];
    let dir = tempfile::tempdir().unwrap();
    for name in ["settings.json", "ui-settings.json"] {
        let original = oldest.file("all-changed", name).unwrap();
        let text = std::fs::read_to_string(&original).unwrap();
        let edited = dir.path().join(name);
        std::fs::write(
            &edited,
            format!("\u{feff}{}\r\n", text.replace('\n', "\r\n")),
        )
        .unwrap();
        if name == "settings.json" {
            assert_eq!(
                Preferences::load_from(&edited).unwrap(),
                Preferences::load_from(&original).unwrap()
            );
        } else {
            assert_eq!(
                UiSettings::load_from(&edited),
                UiSettings::load_from(&original)
            );
        }
    }
}

// ---------------------------------------------------------------------
// What this release writes.

fn capture_status() -> DesktopCaptureStatus {
    DesktopCaptureStatus {
        pid: 4242,
        time: STATUS_TIME.into(),
        backend: "wgc".into(),
        frames: u64::MAX,
        fps: 119.88,
        average_capture_milliseconds: 1.25,
        bounds: Rect {
            x: 0,
            y: 0,
            width: 3840,
            height: 2160,
        },
        source_bounds: Rect {
            x: -1920,
            y: -1080,
            width: 1920,
            height: 1080,
        },
    }
}

fn border_status() -> CaptureBorderStatus {
    CaptureBorderStatus {
        pid: 4242,
        time: STATUS_TIME.into(),
        access: "Allowed".into(),
        border_required: false,
        error: None,
    }
}

/// The states of the corpus, written by this release's own writers into
/// `root`: the same settings every release's fixtures were made from.
fn write_states(root: &Path) {
    let all_changed = Preferences {
        mode: CaptureMode::Active,
        side_by_side: true,
        auto_convert: false,
        minimize_to_tray: false,
        recover: false,
        keep_overlay_on_top: false,
        check_updates: false,
        depth: 1.37,
        popout: -0.42,
    };
    let preferences = [
        ("default", Preferences::default()),
        ("all-changed", all_changed),
        (
            "mode-window",
            Preferences {
                mode: CaptureMode::Window,
                depth: 0.5,
                ..Default::default()
            },
        ),
        (
            "max",
            Preferences {
                side_by_side: true,
                depth: 2.0,
                popout: 1.0,
                ..Default::default()
            },
        ),
        (
            "min",
            Preferences {
                auto_convert: false,
                depth: 0.0,
                popout: -1.0,
                ..Default::default()
            },
        ),
    ];
    for (state, mut preferences) in preferences {
        preferences
            .save_to(&root.join(state).join("settings.json"))
            .unwrap();
    }
    let ui = [
        ("default", false, None),
        ("all-changed", true, Some(1_800_000_000)),
        ("max", true, Some(i64::MAX)),
        ("min", false, Some(i64::MIN)),
    ];
    for (state, dark_mode, last_update_check) in ui {
        UiSettings {
            dark_mode,
            last_update_check,
        }
        .save_to(&root.join(state).join("ui-settings.json"))
        .unwrap();
    }
    let lens = |pitch, slant, offset, bgr| Lens {
        pitch,
        slant,
        offset,
        bgr,
    };
    let lenses = [
        ("default", Lens::default()),
        ("all-changed", lens(5.4321, -0.2987, 0.75, true)),
        ("max", lens(64.0, 2.0, 0.999999, false)),
        ("min", lens(0.7, -2.0, 0.0, false)),
    ];
    for (state, lens) in lenses {
        // As crates/app/src/ui/lens.rs saves it.
        fsutil::write_json_atomic(&root.join(state).join("lens.json"), &lens.sanitized()).unwrap();
    }
    let status = |state: &str, ready| WorkerStatus {
        state: state.into(),
        time: STATUS_TIME.into(),
        pid: 4242,
        ready,
    };
    let folder = root.join("status");
    fsutil::write_json_atomic(
        &folder.join("worker-status.json"),
        &status(STATUS_TEXT, false),
    )
    .unwrap();
    fsutil::write_json_atomic(
        &folder.join("desktop-capture-status.json"),
        &capture_status(),
    )
    .unwrap();
    fsutil::write_json_atomic(&folder.join("capture-border-status.json"), &border_status())
        .unwrap();
    fsutil::write_json_atomic(
        &root.join("status-ready").join("worker-status.json"),
        &status("3D is on.", true),
    )
    .unwrap();
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                files.push(path.strip_prefix(root).unwrap().to_owned());
            }
        }
    }
    files.sort();
    files
}

/// The files this release writes are, byte for byte, the ones of the newest
/// release in the corpus. When this fails the saved format has changed:
/// decide whether older releases can still read it (the next test says what
/// they look for), then add this release's files to the corpus, as the
/// README beside it describes (`A2O_WRITE_STATE_FIXTURES`).
#[test]
fn this_release_writes_what_the_newest_release_in_the_corpus_wrote() {
    if let Some(folder) = std::env::var_os("A2O_WRITE_STATE_FIXTURES") {
        write_states(Path::new(&folder));
        return;
    }
    let releases = releases();
    let newest = releases.last().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write_states(dir.path());
    let written = files_under(dir.path());
    assert!(written.len() >= 17, "{written:?}");
    for file in written {
        let ours = std::fs::read(dir.path().join(&file)).unwrap();
        let theirs = std::fs::read(newest.dir.join(&file))
            .unwrap_or_else(|e| panic!("{}/{}: {e}", newest.tag, file.display()));
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&theirs),
            "{} is no longer written as {} wrote it",
            file.display(),
            newest.tag
        );
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "true or false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// Going back to an older release: it reads the files this one wrote. It
/// cannot be run here, but what it looks for can be told from what it
/// wrote: every key of its files must still be in the newest files, with a
/// value of the same kind (a `null` where it had a value, or text where it
/// had a number, is what an old reader calls damaged or drops), and the
/// capture mode under one of the three names every release knows.
#[test]
fn the_newest_files_still_have_what_every_older_release_looks_for() {
    let releases = releases();
    let newest = releases.last().unwrap();
    let mut compared = 0;
    for older in &releases[..releases.len() - 1] {
        for file in files_under(&older.dir) {
            let name = file.file_name().unwrap().to_string_lossy();
            let folder = file.parent().unwrap().to_string_lossy();
            // Installation records are written by the installer, and what
            // the published program wrote through its window is covered by
            // the fixed states.
            if name == "version.json" || folder == "linux-gui" {
                continue;
            }
            let (Value::Object(old), Value::Object(new)) =
                (json(&older.dir.join(&file)), json(&newest.dir.join(&file)))
            else {
                panic!("{} is not an object", file.display());
            };
            compared += 1;
            for (key, value) in &old {
                let what = format!("{} in {} of {}", key, file.display(), older.tag);
                let now = new
                    .get(key)
                    .unwrap_or_else(|| panic!("{what} is no longer written"));
                assert_eq!(kind(now), kind(value), "{what}");
                assert_eq!(now, value, "{what}: the same setting, another value");
            }
            if let Some(mode) = new.get("Mode") {
                assert!(
                    ["Desktop", "Window", "Active"].contains(&mode.as_str().unwrap()),
                    "{mode}"
                );
            }
        }
    }
    assert!(compared >= 150, "{compared} files compared");
}
