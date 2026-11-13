//! Recognize a log marker only in data written after a launch boundary.
//!
//! The log belongs to another vendor and its format is not known here: it
//! may be appended to, truncated at every start, or rotated, with or without
//! timestamps. Three rules follow from that, each chosen so that it can only
//! withhold "ready" from bytes that provably predate the launch, never from
//! bytes a fresh log could legitimately contain (a missed "ready" stops a
//! working conversion after the timeout; a false one only mislabels a
//! failed start):
//!
//! 1. While the file is the same one and the bytes before the cursor are
//!    unchanged, only what is appended after the cursor counts.
//! 2. A replaced, truncated or rewritten file is scanned from its start,
//!    except for rule 3.
//! 3. If such a file begins with a copy of how the launch-time file *ended*
//!    (a logger that trims an oversized log to its tail, in place or through
//!    a new file), that copy is launch-time content and is skipped.
//!
//! What this cannot tell apart, by content or by file times (NTFS hands a
//! recreated file its predecessor's creation time):
//!
//! * A file that reproduces the launch-time file from its first byte and
//!   continues. It may be a rewrite that kept every old line, or a rotated
//!   log without timestamps that printed the same lines again. Rule 2 scans
//!   it from the start when it is a new file, so the second case becomes
//!   ready and an old marker kept by the first one is counted; when it is
//!   the same file that regrew past the cursor between two polls, rule 1
//!   treats it as an append and a marker before the cursor is missed.
//! * A carried-over tail longer than [`LAUNCH_TAIL_LIMIT`], or one seen while
//!   its copy is still being written.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Read and held in memory at a time.
const READ_LIMIT: u64 = 64 * 1024;
/// Scanned by one poll at most, so that a poll stays short on the thread
/// that makes it. It was one [`READ_LIMIT`]: at the worker's two polls a
/// second, a log restarted with more than 3.8 MB before the marker (rule 2)
/// was not read to its end within the 30 s the engine is given to become
/// ready, and a working conversion was stopped.
const POLL_LIMIT: u64 = 16 * 1024 * 1024;
const CHECKPOINT_LEN: usize = 64;
/// How much of the end of the launch-time file is remembered for rule 3.
const LAUNCH_TAIL_LIMIT: u64 = 1024 * 1024;

pub struct LogReadiness {
    path: PathBuf,
    identity: Option<same_file::Handle>,
    offset: u64,
    checkpoint: Vec<u8>,
    pending: Vec<u8>,
    marker: Vec<u8>,
    /// The end of the file as it was at the snapshot. Never updated: bytes
    /// written after the launch are new wherever a later rewrite puts them.
    launch_tail: Vec<u8>,
    /// `launch_tail` is that whole file, not only its end.
    launch_tail_is_whole: bool,
    error_reported: bool,
}

impl LogReadiness {
    /// Snapshot immediately before starting the producer. Existing contents,
    /// including a partial marker, do not count as evidence for the new run.
    pub fn snapshot(path: &Path, marker: &str) -> io::Result<Self> {
        Self::snapshot_remembering(path, marker, LAUNCH_TAIL_LIMIT)
    }

    fn snapshot_remembering(path: &Path, marker: &str, tail_limit: u64) -> io::Result<Self> {
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
            launch_tail: Vec::new(),
            launch_tail_is_whole: true,
            error_reported: false,
        };
        if let Some(mut file) = probe.open()? {
            probe.identity = Some(same_file::Handle::from_file(file.try_clone()?)?);
            probe.offset = file.metadata()?.len();
            probe.checkpoint = checkpoint(&mut file, probe.offset)?;
            let remembered = probe.offset.min(tail_limit);
            probe.launch_tail = read_at(&mut file, probe.offset - remembered, remembered)?;
            probe.launch_tail_is_whole = remembered == probe.offset;
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

    /// [`poll`](Self::poll) for a caller that keeps polling until a deadline:
    /// a failed read counts as "not ready yet", and `on_error` is told about
    /// the first one, so a log that can never be read does not end as an
    /// unexplained timeout.
    pub fn poll_or_report(&mut self, on_error: impl FnOnce(&io::Error)) -> bool {
        match self.poll() {
            Ok(found) => found,
            Err(error) => {
                if !std::mem::replace(&mut self.error_reported, true) {
                    on_error(&error);
                }
                false
            }
        }
    }

    /// Read a bounded increment. Replacement, truncation, and a changed
    /// checkpoint at the old cursor restart the scan at the new file's
    /// start, after any copy of the launch-time file's end it begins with
    /// (see the module documentation).
    pub fn poll(&mut self) -> io::Result<bool> {
        self.poll_within(POLL_LIMIT)
    }

    /// [`poll`](Self::poll) reading at most `limit` bytes, rounded up to
    /// whole chunks of [`READ_LIMIT`].
    fn poll_within(&mut self, limit: u64) -> io::Result<bool> {
        let Some(mut file) = self.open()? else {
            return Ok(false);
        };
        let identity = same_file::Handle::from_file(file.try_clone()?)?;
        let same = self.identity.as_ref() == Some(&identity);
        let len = file.metadata()?.len();
        if !same || len < self.offset || checkpoint(&mut file, self.offset)? != self.checkpoint {
            // Nothing is changed until the new start is known: a read that
            // fails here leaves the restart to be detected again.
            let carried = self.carried_over(&mut file, len)?;
            self.offset = carried.len() as u64;
            self.checkpoint = carried;
            if self.checkpoint.len() > CHECKPOINT_LEN {
                self.checkpoint
                    .drain(..self.checkpoint.len() - CHECKPOINT_LEN);
            }
            self.pending.clear();
        }
        self.identity = Some(identity);
        file.seek(SeekFrom::Start(self.offset))?;
        // One chunk at a time, so the memory held does not depend on how
        // far behind the cursor is; the state is whole after each chunk, so
        // a read that fails later loses nothing.
        let mut bytes = Vec::new();
        let mut scanned = 0;
        while scanned < limit {
            bytes.clear();
            (&mut file).take(READ_LIMIT).read_to_end(&mut bytes)?;
            if bytes.is_empty() {
                break;
            }
            scanned += bytes.len() as u64;
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
            if found {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl LogReadiness {
    /// The bytes at the start of a restarted file that are a copy of the end
    /// of the launch-time file (rule 3), or nothing.
    ///
    /// Three limits keep this from ever hiding fresh output. The copy must
    /// reach the launch-time file's last byte. It must begin strictly inside
    /// that file: a file reproducing the whole of it may equally be a
    /// rotated log printing the same lines again, and those are new. And it
    /// must be at least as long as the marker: a shorter overlap cannot hold
    /// an old marker, and overlaps of a byte or two happen by coincidence.
    fn carried_over(&self, file: &mut File, len: u64) -> io::Result<Vec<u8>> {
        let tail = &self.launch_tail;
        let available = (tail.len() as u64).min(len);
        if available < self.marker.len() as u64 {
            return Ok(Vec::new());
        }
        let mut head = read_at(file, 0, available)?;
        let overlap = suffix_prefix_overlap(tail, &head);
        let whole_file = self.launch_tail_is_whole && overlap == tail.len();
        head.truncate(if whole_file || overlap < self.marker.len() {
            0
        } else {
            overlap
        });
        Ok(head)
    }
}

/// Length of the longest prefix of `head` that is also a suffix of `tail`
/// (Knuth-Morris-Pratt: linear in both, with one table the size of `head`).
fn suffix_prefix_overlap(tail: &[u8], head: &[u8]) -> usize {
    // border[i]: length of the longest proper prefix of head[..=i] that is
    // also its suffix. u32 is enough for the megabyte remembered here and
    // halves the table.
    let mut border = vec![0u32; head.len()];
    let mut matched = 0usize;
    for i in 1..head.len() {
        while matched > 0 && head[i] != head[matched] {
            matched = border[matched - 1] as usize;
        }
        if head[i] == head[matched] {
            matched += 1;
        }
        border[i] = matched as u32;
    }
    let mut matched = 0usize;
    for &byte in tail {
        if matched == head.len() {
            matched = border.last().map_or(0, |&b| b as usize);
        }
        while matched > 0 && byte != head[matched] {
            matched = border[matched - 1] as usize;
        }
        if head.get(matched) == Some(&byte) {
            matched += 1;
        }
    }
    matched
}

fn checkpoint(file: &mut File, offset: u64) -> io::Result<Vec<u8>> {
    let count = offset.min(CHECKPOINT_LEN as u64);
    read_at(file, offset - count, count)
}

fn read_at(file: &mut File, start: u64, count: u64) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(start))?;
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
        assert!(!probe.poll_within(READ_LIMIT).unwrap());
        assert_eq!(probe.offset, READ_LIMIT);
        assert!(probe.poll_within(READ_LIMIT).unwrap());
        // The same split between two chunks of one poll.
        let text = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        std::fs::write(&path, &text).unwrap();
        assert!(probe.poll().unwrap());
        assert_eq!(probe.offset, text.len() as u64);
    }

    /// The worker polls twice a second for the 30 s of
    /// `engine::ENGINE_READY_TIMEOUT_SECS`: sixty polls.
    const POLLS_BEFORE_THE_TIMEOUT: usize = 60;

    #[test]
    fn a_marker_behind_megabytes_of_new_log_is_found_before_the_timeout() {
        // The log was replaced by one that already holds 5 MB of this run's
        // lines when it is first looked at (rule 2: all of it is new), and
        // the marker comes after them. With one 64 KiB read per poll the
        // sixty polls ended 1.2 MB short of it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        std::fs::write(&path, "previous run\n").unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        let line = "2026-11-10 09:00:01 frame converted, nothing to see\n";
        let backlog = line.repeat(5 * 1024 * 1024 / line.len());
        std::fs::write(&path, format!("{backlog}{NEW_READY}")).unwrap();
        let polls = (1..=POLLS_BEFORE_THE_TIMEOUT).find(|_| probe.poll().unwrap());
        assert_eq!(polls, Some(1));
        assert_eq!(probe.offset as usize, backlog.len() + NEW_READY.len());
        // Appended to since the last poll by more than one chunk: the same.
        append(&path, &format!("{backlog}{NEW_READY}"));
        assert!(probe.poll().unwrap());
    }

    #[test]
    fn a_backlog_larger_than_one_poll_is_resumed_without_a_gap() {
        // Three chunks and a marker across the end of the second one, read
        // two chunks at a time: the cursor neither skips nor re-reads.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        let chunk = READ_LIMIT as usize;
        let text = format!("{}{MARKER}{}", "x".repeat(2 * chunk - 7), "y".repeat(chunk));
        std::fs::write(&path, &text).unwrap();
        assert!(!probe.poll_within(2 * READ_LIMIT).unwrap());
        assert_eq!(probe.offset, 2 * READ_LIMIT);
        assert!(probe.poll_within(2 * READ_LIMIT).unwrap());
        assert_eq!(probe.offset, 3 * READ_LIMIT);
        // Found once: what follows the marker holds no second one.
        assert!(!probe.poll().unwrap());
        assert_eq!(probe.offset as usize, text.len());
        // Truncated and rewritten while the cursor was mid-file.
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        append(&path, &"z".repeat(3 * chunk));
        assert!(!probe.poll_within(READ_LIMIT).unwrap());
        std::fs::write(&path, format!("{}{MARKER}", "w".repeat(chunk + 5))).unwrap();
        assert!(probe.poll().unwrap());
    }

    const OLD_READY: &str = "2026-11-09 10:00:05 First time MinimizeUI\n2026-11-09 11:00:00 Stop\n";
    const NEW_FAILURE: &str = "2026-11-10 09:00:01 model failed\n";
    const NEW_READY: &str = "2026-11-10 09:00:09 First time MinimizeUI\n";

    fn old_log() -> String {
        format!("{}{OLD_READY}", "2026-11-09 09:00:00 noise\n".repeat(100))
    }

    #[test]
    fn a_log_trimmed_to_its_tail_in_place_does_not_bring_the_old_marker_back() {
        // The logger cuts an oversized log down to its last lines in the
        // same file, then the new run fails. The file shrank, so the scan
        // restarts; the old "ready" line at its start is not this run's.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        std::fs::write(&path, old_log()).unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        std::fs::write(&path, format!("{OLD_READY}{NEW_FAILURE}")).unwrap();
        assert!(!probe.poll().unwrap());
        assert_eq!(probe.offset as usize, OLD_READY.len() + NEW_FAILURE.len());
        assert!(!probe.poll().unwrap());
        append(&path, NEW_READY);
        assert!(probe.poll().unwrap(), "this run's own marker still counts");
    }

    #[test]
    fn a_new_file_holding_the_old_tail_does_not_bring_the_old_marker_back() {
        // The same trim through a new file that takes the log's place.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        std::fs::write(&path, old_log()).unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        let trimmed = dir.path().join("conversion.log.tmp");
        // Seen first with nothing after the kept tail, then with new lines.
        std::fs::write(&trimmed, OLD_READY).unwrap();
        std::fs::rename(&path, dir.path().join("conversion.log.old")).unwrap();
        std::fs::rename(&trimmed, &path).unwrap();
        assert!(!probe.poll().unwrap());
        append(&path, NEW_FAILURE);
        assert!(!probe.poll().unwrap());
        append(&path, NEW_READY);
        assert!(probe.poll().unwrap());
    }

    #[test]
    fn only_the_remembered_end_of_a_large_launch_file_is_needed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        std::fs::write(&path, old_log()).unwrap();
        // Remember exactly the two last lines of a much longer file.
        let mut probe =
            LogReadiness::snapshot_remembering(&path, MARKER, OLD_READY.len() as u64).unwrap();
        assert!(!probe.launch_tail_is_whole);
        std::fs::write(&path, format!("{OLD_READY}{NEW_FAILURE}")).unwrap();
        assert!(!probe.poll().unwrap());
        // A kept tail longer than what was remembered is not recognized.
        std::fs::write(&path, old_log()).unwrap();
        let mut probe = LogReadiness::snapshot_remembering(&path, MARKER, 30).unwrap();
        std::fs::write(&path, OLD_READY).unwrap();
        assert!(probe.poll().unwrap(), "documented limit of rule 3");
    }

    #[test]
    fn a_rotated_log_that_prints_the_same_lines_again_is_ready() {
        // No timestamps: every run writes these bytes. The previous run's
        // file was renamed away and this run's file already holds all of it
        // and more. That the old content is reproduced from the first byte
        // must not withhold "ready"; it is why a rewrite that kept *every*
        // old line cannot be told apart and is scanned as new.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        let run = format!("Init engine\nLoading model\n{MARKER}\n");
        std::fs::write(&path, &run).unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        std::fs::rename(&path, dir.path().join("conversion-prev.log")).unwrap();
        std::fs::write(&path, format!("{run}Rendering\n")).unwrap();
        assert!(probe.poll().unwrap());
        // Truncated in place and only partly rewritten: the same.
        std::fs::write(&path, format!("{run}Rendering\nStop\n")).unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        std::fs::write(&path, &run).unwrap();
        assert!(probe.poll().unwrap());
    }

    #[test]
    fn data_written_after_the_launch_stays_new_when_it_is_carried_over() {
        // The launch-time file is what rule 3 compares with, not whatever
        // was scanned since: lines this run wrote count wherever they go.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        std::fs::write(&path, "2026-11-09 11:00:00 Stop\n").unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        append(&path, "2026-11-10 09:00:00 Init\n");
        assert!(!probe.poll().unwrap());
        std::fs::write(&path, format!("2026-11-10 09:00:00 Init\n{NEW_READY}")).unwrap();
        assert!(probe.poll().unwrap());
    }

    #[test]
    fn overlap_is_the_longest_prefix_of_the_head_that_ends_the_tail() {
        assert_eq!(suffix_prefix_overlap(b"abcdef", b"defxyz"), 3);
        assert_eq!(suffix_prefix_overlap(b"abcdef", b"abcdef"), 6);
        assert_eq!(suffix_prefix_overlap(b"abcdef", b"abcdefgh"), 6);
        assert_eq!(suffix_prefix_overlap(b"abcdef", b"ef"), 2);
        assert_eq!(suffix_prefix_overlap(b"abcdef", b"xyz"), 0);
        assert_eq!(suffix_prefix_overlap(b"abcdef", b""), 0);
        assert_eq!(suffix_prefix_overlap(b"", b"abc"), 0);
        // Repetition: the longest, not the first, match; a full match of a
        // short head in the middle of the tail does not end the search.
        assert_eq!(suffix_prefix_overlap(b"xaaaa", b"aaab"), 3);
        assert_eq!(suffix_prefix_overlap(b"abababab", b"abab"), 4);
        assert_eq!(suffix_prefix_overlap(b"abcabcab", b"abcabd"), 5);
        assert_eq!(suffix_prefix_overlap(b"abcabcabd", b"abcabd"), 6);
        assert_eq!(suffix_prefix_overlap(b"abcabcabx", b"abcabd"), 0);
        assert_eq!(suffix_prefix_overlap(b"aabaab", b"aabaabaab"), 6);
        // Checked against the definition for every small case.
        let alphabet = *b"ab";
        let words: Vec<Vec<u8>> = (0..=5usize)
            .flat_map(|len| {
                (0..1usize << len).map(move |bits| {
                    (0..len)
                        .map(|i| alphabet[bits >> i & 1])
                        .collect::<Vec<u8>>()
                })
            })
            .collect();
        for tail in &words {
            for head in &words {
                let expected = (0..=tail.len().min(head.len()))
                    .rev()
                    .find(|&j| tail[tail.len() - j..] == head[..j])
                    .unwrap();
                assert_eq!(
                    suffix_prefix_overlap(tail, head),
                    expected,
                    "{tail:?} {head:?}"
                );
            }
        }
    }

    #[test]
    fn short_or_coincidental_overlaps_hide_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        // The old file ends with the bytes the new one starts with, but
        // fewer than a marker's worth: the new file is scanned whole.
        std::fs::write(&path, "previous run\nFirst").unwrap();
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        std::fs::write(&path, MARKER).unwrap();
        assert!(probe.poll().unwrap());
    }

    #[test]
    fn the_first_read_error_is_reported_once_and_polling_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversion.log");
        let mut probe = LogReadiness::snapshot(&path, MARKER).unwrap();
        // A directory where the log should be cannot be read as a file.
        std::fs::create_dir(&path).unwrap();
        let mut reports = Vec::new();
        for _ in 0..3 {
            assert!(!probe.poll_or_report(|e| reports.push(e.to_string())));
        }
        assert_eq!(reports.len(), 1, "{reports:?}");
        std::fs::remove_dir(&path).unwrap();
        std::fs::write(&path, MARKER).unwrap();
        assert!(probe.poll_or_report(|e| reports.push(e.to_string())));
        assert_eq!(reports.len(), 1);
    }
}
