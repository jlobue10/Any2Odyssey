//! Small file helpers shared by every crate.

use std::fs;
use std::io;
use std::path::Path;

/// Write `bytes` to `path` atomically: the content goes to a sibling temporary
/// file first and is renamed into place, so readers never observe a partial
/// file. Parent directories are created as needed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    // Exclusive creation prevents collisions and following a pre-planted
    // sibling symlink. Each writer owns its temporary file until publication.
    let mut tmp = tempfile::Builder::new()
        .prefix(".any2odyssey-")
        .tempfile_in(parent)?;
    tmp.write_all(bytes)?;
    // Replacing a file that another process or thread has open can fail
    // transiently on Windows (sharing violation / access denied) even though
    // the opener used FILE_SHARE_DELETE. Status files are read while they are
    // rewritten, so retry briefly before giving up.
    let mut tmp = tmp;
    let mut attempt = 0;
    loop {
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(e) if attempt < 25 && is_transient_replace_error(&e.error) => {
                attempt += 1;
                tmp = e.file;
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
            Err(e) => return Err(e.error),
        }
    }
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

/// Read at most the last `chars` bytes of a (possibly growing) text file, as
/// lossy UTF-8. Missing or unreadable files yield an empty string, matching the
/// tolerant behaviour needed when tailing Samsung's conversion log.
pub fn tail(path: &Path, chars: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return String::new();
    };
    let start = len.saturating_sub(chars);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
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
    fn tail_returns_suffix_and_tolerates_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.txt");
        fs::write(&path, "0123456789").unwrap();
        assert_eq!(tail(&path, 4), "6789");
        assert_eq!(tail(&path, 100), "0123456789");
        assert_eq!(tail(&dir.path().join("missing"), 10), "");
    }
}
