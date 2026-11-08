//! Recognize a log marker only in data written after a launch boundary.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const READ_LIMIT: u64 = 64 * 1024;
const CHECKPOINT_LEN: usize = 64;

pub struct LogReadiness {
    path: PathBuf,
    identity: Option<same_file::Handle>,
    offset: u64,
    checkpoint: Vec<u8>,
    pending: Vec<u8>,
    marker: Vec<u8>,
}

impl LogReadiness {
    /// Snapshot immediately before starting the producer. Existing contents,
    /// including a partial marker, do not count as evidence for the new run.
    pub fn snapshot(path: &Path, marker: &str) -> io::Result<Self> {
        if marker.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty log marker",
            ));
        }
        let mut probe = Self {
            path: path.to_owned(),
            identity: None,
            offset: 0,
            checkpoint: Vec::new(),
            pending: Vec::new(),
            marker: marker.as_bytes().to_vec(),
        };
        if let Some(mut file) = probe.open()? {
            probe.identity = Some(same_file::Handle::from_file(file.try_clone()?)?);
            probe.offset = file.metadata()?.len();
            probe.checkpoint = checkpoint(&mut file, probe.offset)?;
        }
        Ok(probe)
    }

    fn open(&self) -> io::Result<Option<File>> {
        match File::open(&self.path) {
            Ok(file) => Ok(Some(file)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Read a bounded increment. Replacement, truncation, and a changed
    /// checkpoint at the old cursor restart the scan at the new file's start.
    pub fn poll(&mut self) -> io::Result<bool> {
        let Some(mut file) = self.open()? else {
            return Ok(false);
        };
        let identity = same_file::Handle::from_file(file.try_clone()?)?;
        let same = self.identity.as_ref() == Some(&identity);
        let len = file.metadata()?.len();
        if !same || len < self.offset || checkpoint(&mut file, self.offset)? != self.checkpoint {
            self.offset = 0;
            self.checkpoint.clear();
            self.pending.clear();
        }
        self.identity = Some(identity);
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        file.take(READ_LIMIT).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        self.checkpoint.extend_from_slice(&bytes);
        if self.checkpoint.len() > CHECKPOINT_LEN {
            self.checkpoint
                .drain(..self.checkpoint.len() - CHECKPOINT_LEN);
        }
        self.pending.extend_from_slice(&bytes);
        let found = self
            .pending
            .windows(self.marker.len())
            .any(|w| w == self.marker);
        let keep = self.marker.len() - 1;
        if self.pending.len() > keep {
            self.pending.drain(..self.pending.len() - keep);
        }
        Ok(found)
    }
}

fn checkpoint(file: &mut File, offset: u64) -> io::Result<Vec<u8>> {
    let count = offset.min(CHECKPOINT_LEN as u64);
    file.seek(SeekFrom::Start(offset - count))?;
    let mut bytes = vec![0; count as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    const MARKER: &str = "First time MinimizeUI";

    fn append(path: &Path, text: &str) {
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    #[test]
    fn old_marker_and_new_failure_are_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        std::fs::write(&path, format!("{MARKER}\n")).unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        append(&path, "new session: model failed\n");
        assert!(!probe.poll().unwrap());
        append(&path, "First time ");
        assert!(!probe.poll().unwrap());
        append(&path, "MinimizeUI\n");
        assert!(probe.poll().unwrap());
    }

    #[test]
    fn old_partial_marker_cannot_complete_in_new_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        std::fs::write(&path, "First time ").unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        append(&path, "MinimizeUI\n");
        assert!(!probe.poll().unwrap());
    }

    #[test]
    fn missing_truncated_and_replaced_logs_are_supported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        assert!(!probe.poll().unwrap());
        std::fs::write(&path, MARKER).unwrap();
        assert!(probe.poll().unwrap());
        std::fs::write(&path, "reset\n").unwrap();
        assert!(!probe.poll().unwrap());
        append(&path, MARKER);
        assert!(probe.poll().unwrap());
        std::fs::rename(&path, dir.path().join("previous.log")).unwrap();
        std::fs::write(&path, MARKER).unwrap();
        assert!(probe.poll().unwrap());
    }

    #[test]
    fn detects_rewrite_that_regrows_past_the_cursor_between_polls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        std::fs::write(&path, "old session").unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        std::fs::write(&path, format!("new session\n{MARKER}\n")).unwrap();
        assert!(probe.poll().unwrap());
    }

    #[test]
    fn reads_in_bounded_chunks_without_losing_split_markers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        std::fs::write(
            &path,
            format!("{}{MARKER}", "x".repeat(READ_LIMIT as usize - 4)),
        )
        .unwrap();
        assert!(!probe.poll().unwrap());
        assert_eq!(probe.offset, READ_LIMIT);
        assert!(probe.poll().unwrap());
    }
}
