//! Small file helpers shared by every crate.

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

/// The most this program reads of a settings, status or record file it
/// loads whole: 1 MiB. The real ones are a few hundred bytes to a few
/// kilobytes.
pub const SMALL_FILE_LIMIT: u64 = 1024 * 1024;

/// Read a whole file that should be small, refusing one over `limit` bytes.
///
/// The files read this way can be written by other programs: Samsung's
/// `OverlaySettings.json` lives under `LocalLow`, which low-integrity
/// processes may write, and the data folder is open to everything the user
/// runs. `fs::read` would load whatever size it finds, some of them four
/// times a second. An oversized file is an `InvalidData` error naming the
/// file; a missing one keeps its `NotFound`.
pub fn read_limited(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is larger than {limit} bytes and was not read",
                path.display()
            ),
        ));
    }
    Ok(bytes)
}

/// [`read_limited`] for UTF-8 text.
pub fn read_to_string_limited(path: &Path, limit: u64) -> io::Result<String> {
    String::from_utf8(read_limited(path, limit)?).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })
}

/// Write `bytes` to `path` atomically: the content goes to a sibling temporary
/// file first and is renamed into place, so readers never observe a partial
/// file. Parent directories are created as needed.
///
/// The published file is the temporary one, so it has that file's
/// permissions, not those of a file it replaces: on Unix it is readable and
/// writable by the owner only (0600). That suits everything kept in the data
/// folder (preferences, status and diagnostics files, which name window
/// titles); a file written for other readers goes through
/// [`write_atomic_document`].
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_inner(path, bytes, false, Audience::Owner)
}

/// How often the rename that publishes a file is tried again after a
/// transient failure, and how long each wait is: up to 100 ms in all.
const REPLACE_RETRIES: u32 = 25;
const REPLACE_RETRY_PAUSE: std::time::Duration = std::time::Duration::from_millis(4);

#[cfg(test)]
thread_local! {
    /// Retries of the publishing rename made on this thread, so a test can
    /// count them instead of timing them.
    static REPLACE_RETRIES_MADE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// [`write_atomic`] that also flushes the content to disk before the rename,
/// for the preference files: a power loss right after Apply must not leave
/// an empty `settings.json` that the next launch treats as damaged. Status
/// files are rewritten every second and stay unsynced.
pub fn write_atomic_durable(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_inner(path, bytes, true, Audience::Owner)
}

/// [`write_atomic`] for a document the user asked for at a path of their
/// choosing (the exported guide). On Unix it gets the permissions any newly
/// saved file gets, 0666 less the process umask (normally 0644), instead of
/// 0600: a guide exported into a shared or served folder must be readable
/// there. Elsewhere the two functions are the same.
pub fn write_atomic_document(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_inner(path, bytes, false, Audience::Anyone)
}

/// Stream a document into a sibling temporary file, flush its buffered
/// bytes, then publish it atomically. Encoding or flush failures leave the
/// previous destination intact and remove the temporary. New files have the
/// permissions of [`write_atomic_document`]; existing regular files keep
/// their Unix permissions. The writer supports seeking as well as writing.
pub fn write_atomic_document_with(
    path: &Path,
    write: impl FnOnce(&mut io::BufWriter<&mut fs::File>) -> io::Result<()>,
) -> io::Result<()> {
    write_atomic_with(path, false, Audience::Anyone, |file| {
        // A private export must stay private. Do not follow a symlink to
        // copy its target's permissions; the link itself will be replaced.
        #[cfg(unix)]
        if let Ok(metadata) = fs::symlink_metadata(path) {
            if metadata.is_file() {
                file.set_permissions(metadata.permissions())?;
            }
        }
        let mut buffered = io::BufWriter::new(file);
        write(&mut buffered)?;
        buffered.flush()
    })
}

/// Who may read the published file, where the platform lets us choose.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Audience {
    Owner,
    Anyone,
}

fn write_atomic_inner(path: &Path, bytes: &[u8], sync: bool, audience: Audience) -> io::Result<()> {
    write_atomic_with(path, sync, audience, |file| file.write_all(bytes))
}

fn write_atomic_with(
    path: &Path,
    sync: bool,
    audience: Audience,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    write_atomic_retrying(path, sync, audience, REPLACE_RETRIES, write)
}

fn write_atomic_retrying(
    path: &Path,
    sync: bool,
    audience: Audience,
    retries: u32,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let long = path.file_name().and_then(|name| {
        let folder = long_form(parent, name.len().max(TEMPORARY_NAME_LENGTH))?;
        let path = folder.join(name);
        Some((folder, path))
    });
    let (parent, path) = match &long {
        Some((parent, path)) => (parent.as_path(), path.as_path()),
        None => (parent, path),
    };
    // Exclusive creation prevents collisions and following a pre-planted
    // sibling symlink. Each writer owns its temporary file until publication.
    let mut builder = tempfile::Builder::new();
    builder.prefix(TEMPORARY_PREFIX);
    // The mode is given to the creating `open`, so the umask applies as it
    // does to any other new file. Not on Windows: access there is inherited
    // from the folder either way, and the builder rejects the setting.
    #[cfg(unix)]
    if audience == Audience::Anyone {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    #[cfg(not(unix))]
    let _ = audience;
    let mut tmp = builder.tempfile_in(parent)?;
    write(tmp.as_file_mut())?;
    if sync {
        tmp.as_file().sync_all()?;
    }
    // Replacing a file that another process or thread has open can fail
    // transiently on Windows (sharing violation / access denied) even though
    // the opener used FILE_SHARE_DELETE. Status files are read while they are
    // rewritten, so retry briefly before giving up.
    let mut tmp = tmp;
    let mut attempt = 0;
    loop {
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(e) if attempt < retries && is_transient_replace_error(&e.error) => {
                attempt += 1;
                tmp = e.file;
                #[cfg(test)]
                REPLACE_RETRIES_MADE.with(|made| made.set(made.get() + 1));
                std::thread::sleep(REPLACE_RETRY_PAUSE);
            }
            Err(e) => return Err(e.error),
        }
    }
}

/// How the temporary file beside a published one is named, and how long
/// that name is: the prefix and the six characters the builder adds.
const TEMPORARY_PREFIX: &str = ".any2odyssey-";
const TEMPORARY_NAME_LENGTH: usize = TEMPORARY_PREFIX.len() + 6;

/// The most characters Windows takes in a file's full name written the
/// ordinary way (`MAX_PATH`, less its terminator).
const ORDINARY_PATH_LIMIT: usize = 259;

/// `folder`, which exists, in the form Windows takes at any length
/// (`\\?\C:\...`) when a file in it whose name has `longest_name` characters
/// is past what the ordinary form may have: for a temporary file and the
/// name it is published under, whichever is longer. `None` otherwise, and
/// always off Windows.
///
/// The standard library makes this change by itself for whatever it opens,
/// renames or removes. The temporary-file crate publishes with calls of its
/// own that do not, so in such a folder a file could be read and written in
/// place but never replaced: "The system cannot find the path specified".
/// In a data folder of 240 to 245 characters that was because of the
/// temporary's name alone, which is longer than `settings.json`; the same
/// went for a guide or a picture exported into a deep folder.
pub(crate) fn long_form(folder: &Path, longest_name: usize) -> Option<std::path::PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    // Counted in bytes, which are never fewer than Windows's characters.
    let ordinary = std::path::absolute(folder).ok()?;
    if ordinary.as_os_str().len() + 1 + longest_name <= ORDINARY_PATH_LIMIT {
        return None;
    }
    fs::canonicalize(folder).ok()
}

fn is_transient_replace_error(e: &io::Error) -> bool {
    // ERROR_ACCESS_DENIED (5) and ERROR_SHARING_VIOLATION (32) on Windows.
    cfg!(windows)
        && (e.kind() == io::ErrorKind::PermissionDenied
            || matches!(e.raw_os_error(), Some(5) | Some(32)))
}

/// `bytes` without a leading UTF-8 byte-order mark. Notepad's "UTF-8 with BOM"
/// and Windows PowerShell 5.1's `-Encoding UTF8` both write one, and
/// `serde_json` refuses to parse it.
pub fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// [`strip_utf8_bom`] for text.
pub fn strip_utf8_bom_str(text: &str) -> &str {
    text.strip_prefix('\u{FEFF}').unwrap_or(text)
}

/// Serialize `value` as pretty JSON and write it atomically.
pub fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    write_atomic(path, text.as_bytes())
}

/// [`write_json_atomic`] for telemetry written from a thread that must not
/// wait: one attempt at the rename, no retries. A reader or a virus scanner
/// holding the old file open makes [`write_atomic`] sleep for up to 100 ms;
/// on the capture thread that is several lost frames for a file whose next
/// version is due seconds later. The caller skips a failed write, and the
/// previous file stays in place. Never for preferences or for a status the
/// controls act on.
pub fn write_json_atomic_once<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    write_atomic_retrying(path, false, Audience::Owner, 0, |file| {
        file.write_all(text.as_bytes())
    })
}

/// [`write_json_atomic`] through [`write_atomic_durable`].
pub fn write_json_atomic_durable<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    write_atomic_durable(path, text.as_bytes())
}

/// Remove `.any2odyssey-*` temporaries in `dir` that a writer killed between
/// creating and publishing left behind. Only files older than an hour go, so
/// a worker that is mid-write right now is never touched; errors are ignored
/// because the sweep is housekeeping. Returns how many were removed.
pub fn remove_stale_temporaries(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    // `SystemTime - Duration` panics on underflow; a clock near the platform's
    // earliest representable time then sweeps nothing instead of aborting.
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(60 * 60))
        .unwrap_or(std::time::UNIX_EPOCH);
    entries
        .flatten()
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                name.starts_with(".any2odyssey-") || name.starts_with(".overlay-backup-")
            })
        })
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|modified| modified < cutoff)
        })
        .filter(|entry| fs::remove_file(entry.path()).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn atomic_write_replaces_existing_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");
        let path = nested.join("file.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        // Whatever name the temporary had, nothing but the published file remains.
        assert_eq!(entries(&nested), ["file.json"]);
    }

    #[cfg(windows)]
    #[test]
    fn a_single_attempt_write_gives_up_at_once_when_the_file_is_held() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop-capture-status.json");
        write_json_atomic_once(&path, &serde_json::json!({"frames": 1})).unwrap();
        // Opened without sharing: the rename over it is refused the way it
        // is while a scanner or a reader without FILE_SHARE_DELETE has it.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        let retries = || REPLACE_RETRIES_MADE.with(|made| made.replace(0));
        retries();
        let once = write_json_atomic_once(&path, &serde_json::json!({"frames": 2}));
        assert!(once.is_err(), "the held file was replaced");
        assert_eq!(retries(), 0, "the single attempt waited and tried again");
        // The ordinary write does retry this very failure, to the limit.
        assert!(write_json_atomic(&path, &serde_json::json!({"frames": 3})).is_err());
        assert_eq!(retries(), REPLACE_RETRIES);
        drop(held);
        // Neither failure left a temporary behind or touched the content.
        assert_eq!(entries(dir.path()), ["desktop-capture-status.json"]);
        assert!(fs::read_to_string(&path).unwrap().contains("\"frames\": 1"));
        write_json_atomic_once(&path, &serde_json::json!({"frames": 4})).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("\"frames\": 4"));
    }

    #[test]
    fn a_limited_read_takes_a_file_up_to_the_limit_and_refuses_a_larger_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status.json");
        assert_eq!(
            read_limited(&path, 8).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        fs::write(&path, b"").unwrap();
        assert_eq!(read_limited(&path, 8).unwrap(), b"");
        fs::write(&path, b"12345678").unwrap();
        assert_eq!(read_limited(&path, 8).unwrap(), b"12345678");
        assert_eq!(read_to_string_limited(&path, 8).unwrap(), "12345678");
        fs::write(&path, b"123456789").unwrap();
        for error in [
            read_limited(&path, 8).unwrap_err(),
            read_to_string_limited(&path, 8).unwrap_err(),
        ] {
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            let text = error.to_string();
            assert!(
                text.contains("status.json") && text.contains("larger than 8 bytes"),
                "{text}"
            );
        }
        // No overflow at the top of the range.
        assert_eq!(read_limited(&path, u64::MAX).unwrap().len(), 9);
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert_eq!(
            read_to_string_limited(&path, 8).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn streamed_document_failure_preserves_the_destination_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("document");
        fs::write(&path, b"original").unwrap();
        let failed = write_atomic_document_with(&path, |writer| {
            writer.write_all(&[7; 16384])?;
            Err(io::Error::other("encoding failed"))
        });
        assert_eq!(failed.unwrap_err().to_string(), "encoding failed");
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(entries(dir.path()), ["document"]);
        write_atomic_document_with(&path, |writer| writer.write_all(b"new")).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(entries(dir.path()), ["document"]);
    }

    #[cfg(unix)]
    #[test]
    fn replacing_a_private_document_preserves_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private-document");
        fs::write(&path, b"original").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic_document_with(&path, |writer| writer.write_all(b"new")).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        write_atomic_document_with(&path, |writer| writer.write_all(b"new again")).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn temporaries_are_unique_per_writer() {
        // Two writers of the same path must never share a temporary name, so an
        // interrupted writer cannot leave a file the next one appends to or
        // publishes. The exclusive-create temp file is what provides that; a
        // fixed `<name>.tmp` sibling would make this test fail.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let parent = dir.path();
        let first = tempfile::Builder::new()
            .prefix(".any2odyssey-")
            .tempfile_in(parent)
            .unwrap();
        write_atomic(&path, b"new settings").unwrap();
        assert!(
            first.path().exists(),
            "another writer's temporary is untouched"
        );
        assert_eq!(fs::read(&path).unwrap(), b"new settings");
        assert_eq!(entries(parent).len(), 2);
    }

    /// A folder under `base` whose whole path is `length` characters long.
    #[cfg(windows)]
    fn folder_of_length(base: &Path, length: usize) -> std::path::PathBuf {
        let mut folder = base.to_path_buf();
        loop {
            let room = length - folder.as_os_str().len();
            if room > 102 {
                folder.push("d".repeat(100));
            } else {
                folder.push("d".repeat(room - 1));
                return folder;
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_file_is_replaced_where_its_name_or_the_temporarys_passes_the_path_limit() {
        let dir = tempfile::tempdir().unwrap();
        assert!(dir.path().as_os_str().len() < 200, "{:?}", dir.path());
        // In a folder of 239 characters both names fit in the 259 Windows
        // takes. From 240 to 245 "settings.json" fits and the temporary
        // beside it does not; beyond that neither does.
        for length in [239, 240, 245, 246, 300] {
            let folder = folder_of_length(dir.path(), length);
            assert_eq!(folder.as_os_str().len(), length);
            let path = folder.join("settings.json");
            for content in ["one", "two"] {
                write_atomic_durable(&path, content.as_bytes())
                    .unwrap_or_else(|e| panic!("in a folder of {length} characters: {e}"));
                assert_eq!(fs::read_to_string(&path).unwrap(), content);
            }
            write_atomic_document_with(&path, |writer| writer.write_all(b"three")).unwrap();
            write_json_atomic_once(&path, &serde_json::json!({"frames": 4})).unwrap();
            assert!(fs::read_to_string(&path).unwrap().contains("\"frames\": 4"));
            assert_eq!(entries(&folder), ["settings.json"]);
        }
        // The published name can be the longer of the two.
        let folder = folder_of_length(dir.path(), 230);
        let path = folder.join("Samsung-OverlaySettings-before.json");
        write_atomic(&path, b"kept").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"kept");
        assert_eq!(entries(&folder), ["Samsung-OverlaySettings-before.json"]);
        // The other files that are published through a temporary one: the
        // copy kept of unreadable settings and the backup of Samsung's file.
        let folder = folder_of_length(dir.path(), 235);
        fs::create_dir_all(&folder).unwrap();
        let settings = folder.join("settings.json");
        fs::write(&settings, b"not settings").unwrap();
        let kept = crate::prefs::Preferences::default()
            .save_recovering_to(&settings)
            .unwrap()
            .unwrap();
        assert_eq!(fs::read(kept).unwrap(), b"not settings");
        let samsung = dir.path().join("OverlaySettings.json");
        fs::write(&samsung, b"{}").unwrap();
        let backup = folder.join("Samsung-OverlaySettings-before.json");
        crate::overlay_settings::update_file(&samsung, &backup, false, 0.5, 0.5).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), b"{}");
        assert_eq!(entries(&folder).len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_destination_is_replaced_not_followed() {
        // A planted link at the published path must not redirect the write into
        // another file: the rename replaces the directory entry itself.
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("unrelated");
        fs::write(&victim, b"preserve me").unwrap();
        let path = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        write_atomic(&path, b"new settings").unwrap();
        assert_eq!(fs::read(&victim).unwrap(), b"preserve me");
        assert_eq!(fs::read(&path).unwrap(), b"new settings");
        assert!(!fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(entries(dir.path()), ["settings.json", "unrelated"]);
    }

    #[cfg(unix)]
    #[test]
    fn data_files_are_private_and_documents_get_the_default_mode() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        // Whatever this process's umask is, a plainly created file shows it.
        let plain = dir.path().join("plain");
        fs::write(&plain, b"x").unwrap();
        let settings = dir.path().join("settings.json");
        write_atomic_durable(&settings, b"{}").unwrap();
        assert_eq!(mode(&settings), 0o600);
        let status = dir.path().join("worker-status.json");
        write_atomic(&status, b"{}").unwrap();
        assert_eq!(mode(&status), 0o600);
        let guide = dir.path().join("guide.html");
        write_atomic_document(&guide, b"<!doctype html>").unwrap();
        assert_eq!(mode(&guide), mode(&plain));
        // Replacing keeps the rule, whatever the replaced file had.
        fs::set_permissions(&guide, fs::Permissions::from_mode(0o600)).unwrap();
        write_atomic_document(&guide, b"again").unwrap();
        assert_eq!(mode(&guide), mode(&plain));
        assert_eq!(fs::read(&guide).unwrap(), b"again");
    }

    #[test]
    fn a_document_is_published_like_any_other_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("guide.html");
        write_atomic_document(&path, b"one").unwrap();
        write_atomic_document(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(entries(path.parent().unwrap()), ["guide.html"]);
    }

    #[test]
    fn byte_order_marks_are_stripped_once() {
        assert_eq!(strip_utf8_bom(b"\xEF\xBB\xBF{}"), b"{}");
        assert_eq!(strip_utf8_bom(b"{}"), b"{}");
        assert_eq!(strip_utf8_bom(b"\xEF\xBB\xBF\xEF\xBB\xBF"), b"\xEF\xBB\xBF");
        assert_eq!(strip_utf8_bom_str("\u{FEFF}{}"), "{}");
        assert_eq!(strip_utf8_bom_str("{}"), "{}");
    }

    #[test]
    fn concurrent_writers_publish_only_complete_payloads() {
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.json");
        let barrier = Arc::new(Barrier::new(8));
        std::thread::scope(|scope| {
            for id in 0..8u8 {
                let path = &path;
                let barrier = barrier.clone();
                scope.spawn(move || {
                    let payload = vec![id; 64 * 1024];
                    barrier.wait();
                    for _ in 0..20 {
                        write_atomic(path, &payload).unwrap();
                        // A reader may race another writer's replace; on Windows that
                        // can surface as a transient open error. Retry the read, but
                        // whatever is read must always be one complete payload.
                        let observed = loop {
                            match fs::read(path) {
                                Ok(bytes) => break bytes,
                                Err(e) if is_transient_replace_error(&e) => {
                                    std::thread::yield_now()
                                }
                                Err(e) => panic!("read failed: {e}"),
                            }
                        };
                        assert_eq!(observed.len(), payload.len());
                        assert!(observed.iter().all(|&b| b == observed[0]));
                    }
                });
            }
        });
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn durable_write_publishes_like_the_plain_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        write_atomic_durable(&path, b"one").unwrap();
        write_atomic_durable(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(entries(dir.path()), ["settings.json"]);
    }

    #[test]
    fn stale_temporaries_are_swept_but_fresh_and_foreign_files_stay() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join(".any2odyssey-stale");
        let fresh = dir.path().join(".any2odyssey-fresh");
        let foreign = dir.path().join("settings.json");
        for p in [&stale, &fresh, &foreign] {
            fs::write(p, b"x").unwrap();
        }
        let two_hours_ago =
            std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60);
        fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
        assert_eq!(remove_stale_temporaries(dir.path()), 1);
        assert_eq!(entries(dir.path()), [".any2odyssey-fresh", "settings.json"]);
        assert_eq!(remove_stale_temporaries(&dir.path().join("missing")), 0);
    }

    fn age(path: &Path, minutes: u64) {
        let then = std::time::SystemTime::now() - std::time::Duration::from_secs(minutes * 60);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(then)
            .unwrap();
    }

    #[test]
    fn the_sweep_spares_a_temporary_under_an_hour_old_and_old_files_of_other_names() {
        let dir = tempfile::tempdir().unwrap();
        // A writer may still own a temporary half an hour old; settings two
        // hours old are not a temporary at all.
        for (name, minutes) in [
            (".any2odyssey-half-an-hour", 30),
            (".overlay-backup-old", 120),
            (".any2odyssey", 120),
            ("settings.json", 120),
            ("settings-recovery-old.json", 120),
        ] {
            let path = dir.path().join(name);
            fs::write(&path, b"x").unwrap();
            age(&path, minutes);
        }
        assert_eq!(remove_stale_temporaries(dir.path()), 1);
        assert_eq!(
            entries(dir.path()),
            [
                ".any2odyssey",
                ".any2odyssey-half-an-hour",
                "settings-recovery-old.json",
                "settings.json"
            ]
        );
    }

    #[test]
    fn every_json_writer_publishes_the_value_it_was_given() {
        let dir = tempfile::tempdir().unwrap();
        let value = serde_json::json!({"frames": 7, "backend": "test"});
        type Writer = fn(&Path, &serde_json::Value) -> io::Result<()>;
        let writers: [(&str, Writer); 3] = [
            ("retrying.json", write_json_atomic),
            ("once.json", write_json_atomic_once),
            ("durable.json", write_json_atomic_durable),
        ];
        for (name, write) in writers {
            let path = dir.path().join(name);
            write(&path, &value).unwrap();
            let back: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            assert_eq!(back, value, "{name}");
        }
        assert_eq!(
            entries(dir.path()),
            ["durable.json", "once.json", "retrying.json"]
        );
    }

    #[test]
    fn the_small_file_limit_is_a_mebibyte() {
        // The tests of its users name it by the constant.
        assert_eq!(SMALL_FILE_LIMIT, 1_048_576);
    }
}
