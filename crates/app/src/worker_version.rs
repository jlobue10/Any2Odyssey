//! Agreement between the running controls and the installed worker.
//!
//! The worker is a second copy of this executable, placed beside Samsung's
//! engine by the installer. An update that replaced only one of the two (a
//! controls-only install over a full one, a copy made by hand, an install
//! under another Windows account) leaves two versions talking through a
//! command line and status files that may have changed between them. The
//! controls therefore compare the worker's file version with their own
//! before every start and refuse a pair that does not match.
//!
//! The comparison is kept apart from the Win32 read so it is tested on every
//! platform; the read is `any2odyssey_windows::file_version`.

use any2odyssey_core::update::Version;

/// `(major, minor, build, revision)` as a version resource stores it.
pub type FileVersion = (u16, u16, u16, u16);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Agreement {
    /// The worker is the controls' own version.
    Same,
    /// The worker's version could not be read: allowed, and logged by the
    /// caller. A build without a version resource (a cross-compiled or
    /// hand-made one) must not be locked out of its own worker.
    Unknown,
    /// The versions differ; the text is the status line to show.
    Mismatch(String),
}

/// Compare the installed worker's file version with the running controls.
/// The build script stamps `major.minor.patch.0`, so the first three parts
/// decide; the fourth is not part of a release version and is only shown
/// when it is not zero.
pub fn check(installed: Option<FileVersion>, running: Version) -> Agreement {
    let Some((major, minor, patch, revision)) = installed else {
        return Agreement::Unknown;
    };
    if (u32::from(major), u32::from(minor), u32::from(patch))
        == (running.major, running.minor, running.patch)
    {
        return Agreement::Same;
    }
    let installed = if revision == 0 {
        format!("{major}.{minor}.{patch}")
    } else {
        format!("{major}.{minor}.{patch}.{revision}")
    };
    Agreement::Mismatch(format!(
        "The installed conversion worker is version {installed}; these controls are {running}. Run Install.cmd from the same package to update both."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_readable_different_version_blocks_a_start() {
        let running = Version::new(0, 8, 6);
        assert_eq!(check(Some((0, 8, 6, 0)), running), Agreement::Same);
        // The fourth part is not a release version component.
        assert_eq!(check(Some((0, 8, 6, 7)), running), Agreement::Same);
        assert_eq!(check(None, running), Agreement::Unknown);
        for other in [
            (0, 8, 5, 0),
            (0, 8, 7, 0),
            (0, 9, 6, 0),
            (1, 8, 6, 0),
            (0, 0, 0, 0),
        ] {
            assert!(
                matches!(check(Some(other), running), Agreement::Mismatch(_)),
                "{other:?}"
            );
        }
        // A component beyond what a version resource can hold never matches.
        assert!(matches!(
            check(Some((0, 8, u16::MAX, 0)), Version::new(0, 8, 70_000)),
            Agreement::Mismatch(_)
        ));
    }

    #[test]
    fn the_mismatch_names_both_versions_and_the_remedy() {
        assert_eq!(
            check(Some((0, 8, 5, 0)), Version::new(0, 8, 6)),
            Agreement::Mismatch(
                "The installed conversion worker is version 0.8.5; these controls are 0.8.6. Run Install.cmd from the same package to update both."
                    .to_owned()
            )
        );
        let Agreement::Mismatch(text) = check(Some((0, 9, 0, 3)), Version::new(0, 8, 6)) else {
            panic!("a newer worker is a mismatch too");
        };
        assert!(text.contains("version 0.9.0.3;"), "{text}");
    }

    #[test]
    fn this_build_agrees_with_the_version_its_resource_would_carry() {
        // build.rs stamps CARGO_PKG_VERSION with a trailing ".0".
        let v = Version::current();
        let stamped = (v.major as u16, v.minor as u16, v.patch as u16, 0);
        assert_eq!(check(Some(stamped), v), Agreement::Same);
    }
}
