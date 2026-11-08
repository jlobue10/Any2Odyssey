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
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
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

    #[test]
    fn atomic_write_replaces_existing_file_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("file.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert!(!dir.path().join("nested").join("file.json.tmp").exists());
    }

    #[test]
    fn predictable_temp_file_is_not_touched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let old_temp = dir.path().join("settings.json.tmp");
        fs::write(&old_temp, b"belongs to someone else").unwrap();
        write_atomic(&path, b"new settings").unwrap();
        assert_eq!(fs::read(&old_temp).unwrap(), b"belongs to someone else");
        assert_eq!(fs::read(&path).unwrap(), b"new settings");
    }

    #[cfg(unix)]
    #[test]
    fn predictable_temp_symlink_cannot_clobber_another_file() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("unrelated");
        fs::write(&victim, b"preserve me").unwrap();
        let path = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&victim, dir.path().join("settings.json.tmp")).unwrap();
        write_atomic(&path, b"new settings").unwrap();
        assert_eq!(fs::read(victim).unwrap(), b"preserve me");
        assert!(!fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
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
                        let observed = fs::read(path).unwrap();
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
