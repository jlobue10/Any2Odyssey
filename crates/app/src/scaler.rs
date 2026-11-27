//! The choice of the scaler's 3D mode for the Odyssey 3D output (Linux,
//! experimental; `docs/LENS-SWITCH.md`, "The scaler's 3D mode").
//!
//! Nothing here sends the monitor anything. `any2odyssey-weaver` does that
//! at the start of a picture, when the file `scaler.on` is beside it; the
//! box on the viewer's page makes that file and removes it, which is what
//! `install.sh --scaler` and `--no-scaler` do. So the page, the installer
//! and a `touch` in a terminal are one switch, and each sees what the
//! others did. What the scaler answered at the last start is in
//! `scaler.log` in the same folder.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use any2odyssey_core::fsutil;

/// The file whose presence is the choice.
const CHOSEN: &str = "scaler.on";
/// What the program that sends the command said at the last start.
const LOG: &str = "scaler.log";
/// There while a mode that this picture's start switched on is on: the
/// helper makes it before the command and takes it away when the command
/// was refused or the mode was switched off again.
const SENT: &str = "scaler.sent";
/// The program that sends the command, built by the set-up where there is a
/// C compiler.
const PROGRAM: &str = "build/any2odyssey-scaler";

/// The switch beside one `any2odyssey-weaver`.
pub struct Scaler {
    /// The folder the helper itself is in (it is started through a link in
    /// the data folder, and looks beside itself, not beside the link).
    side: Option<PathBuf>,
}

impl Scaler {
    pub fn beside(helper: Option<&Path>) -> Scaler {
        let side = helper
            .and_then(|helper| helper.canonicalize().ok())
            .and_then(|helper| helper.parent().map(Path::to_owned));
        Scaler { side }
    }

    fn file(&self, name: &str) -> Option<PathBuf> {
        self.side.as_ref().map(|side| side.join(name))
    }

    /// Whether the set-up built the program that sends the command.
    pub fn installed(&self) -> bool {
        self.file(PROGRAM).is_some_and(|program| program.is_file())
    }

    /// Whether the output is to switch the mode.
    pub fn chosen(&self) -> bool {
        // (as the helper asks: anything of that name)
        self.file(CHOSEN)
            .is_some_and(|chosen| chosen.symlink_metadata().is_ok())
    }

    /// Make the choice. It counts from the next start of the output.
    pub fn choose(&self, on: bool) -> std::io::Result<()> {
        let Some(chosen) = self.file(CHOSEN) else {
            return Err(std::io::Error::new(
                ErrorKind::NotFound,
                "the Odyssey 3D output is not set up",
            ));
        };
        if on {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(chosen)
                .map(drop)
        } else {
            match std::fs::remove_file(chosen) {
                Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        }
    }

    /// What there is to say about the scaler beside a picture that is
    /// showing: that its 3D mode is on, or why it is not although it was
    /// chosen. Nothing where it was not asked for.
    pub fn with_the_picture(&self) -> Option<String> {
        if self
            .file(SENT)
            .is_some_and(|sent| sent.symlink_metadata().is_ok())
        {
            return Some("Scaler 3D mode: on.".to_owned());
        }
        let reason = self.chosen().then(|| self.refused()).flatten()?;
        Some(format!("Scaler 3D mode: not on ({reason})."))
    }

    /// Why the scaler did not take its command at the last start that sent
    /// one, if it did not.
    pub fn refused(&self) -> Option<String> {
        let bytes = fsutil::read_limited(&self.file(LOG)?, fsutil::SMALL_FILE_LIMIT).ok()?;
        let said = String::from_utf8_lossy(&bytes);
        // The first line is the answer to "on"; a line "leaving: …" after
        // it is the answer to "off" when the picture ended.
        let reason = said.lines().next()?.trim().strip_prefix("error:")?.trim();
        Some(crate::lens_switch::tidy(reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let helper = dir.path().join("any2odyssey-weaver");
        std::fs::write(&helper, "").unwrap();
        (dir, helper)
    }

    #[test]
    fn the_choice_is_the_file_beside_the_helper_and_is_made_and_unmade() {
        let (dir, helper) = side();
        let scaler = Scaler::beside(Some(&helper));
        assert!(!scaler.chosen() && !scaler.installed());
        scaler.choose(true).unwrap();
        assert!(dir.path().join("scaler.on").is_file() && scaler.chosen());
        // Chosen twice, and what a `touch` left there, is still chosen.
        scaler.choose(true).unwrap();
        assert!(scaler.chosen());
        scaler.choose(false).unwrap();
        assert!(!dir.path().join("scaler.on").exists() && !scaler.chosen());
        scaler.choose(false).unwrap();
        std::fs::create_dir(dir.path().join("build")).unwrap();
        std::fs::write(dir.path().join("build/any2odyssey-scaler"), "").unwrap();
        assert!(scaler.installed());
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_started_through_a_link_has_its_switch_where_it_is() {
        let (dir, helper) = side();
        let elsewhere = tempfile::tempdir().unwrap();
        let link = elsewhere.path().join("any2odyssey-weaver");
        std::os::unix::fs::symlink(&helper, &link).unwrap();
        Scaler::beside(Some(&link)).choose(true).unwrap();
        assert!(dir.path().join("scaler.on").exists());
        assert!(!elsewhere.path().join("scaler.on").exists());
    }

    #[test]
    fn without_a_helper_there_is_no_switch() {
        for scaler in [
            Scaler::beside(None),
            Scaler::beside(Some(Path::new("/nonexistent/any2odyssey-weaver"))),
        ] {
            assert!(!scaler.chosen() && !scaler.installed());
            assert!(scaler.choose(true).is_err());
            assert_eq!(scaler.refused(), None);
        }
    }

    #[test]
    fn beside_a_picture_the_scaler_is_on_refused_or_not_spoken_of() {
        let (dir, helper) = side();
        let scaler = Scaler::beside(Some(&helper));
        assert_eq!(scaler.with_the_picture(), None);
        // Chosen after the picture started: nothing was sent for it.
        scaler.choose(true).unwrap();
        std::fs::write(dir.path().join("scaler.log"), "on\nleaving: off\n").unwrap();
        assert_eq!(scaler.with_the_picture(), None);
        std::fs::write(dir.path().join("scaler.sent"), "").unwrap();
        assert_eq!(
            scaler.with_the_picture().as_deref(),
            Some("Scaler 3D mode: on.")
        );
        // Cleared while the picture shows: the mode stays on until it ends.
        scaler.choose(false).unwrap();
        assert_eq!(
            scaler.with_the_picture().as_deref(),
            Some("Scaler 3D mode: on.")
        );
        std::fs::remove_file(dir.path().join("scaler.sent")).unwrap();
        std::fs::write(dir.path().join("scaler.log"), "error: no scaler\n").unwrap();
        assert_eq!(scaler.with_the_picture(), None, "not chosen");
        scaler.choose(true).unwrap();
        assert_eq!(
            scaler.with_the_picture().as_deref(),
            Some("Scaler 3D mode: not on (no scaler).")
        );
    }

    #[test]
    fn a_refusal_is_the_first_line_of_the_log_when_it_is_an_error() {
        let (dir, helper) = side();
        let scaler = Scaler::beside(Some(&helper));
        let log = dir.path().join("scaler.log");
        assert_eq!(scaler.refused(), None);
        std::fs::write(&log, "on\nleaving: off\n").unwrap();
        assert_eq!(scaler.refused(), None);
        std::fs::write(&log, "error: the monitor's scaler was not found\n").unwrap();
        assert_eq!(
            scaler.refused().as_deref(),
            Some("the monitor's scaler was not found")
        );
        // An error when leaving is not a refusal of this picture's mode.
        std::fs::write(&log, "on\nleaving: error: gone\n").unwrap();
        assert_eq!(scaler.refused(), None);
    }
}
