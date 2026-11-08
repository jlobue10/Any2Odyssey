//! Platform-neutral core of Any2Odyssey.
//!
//! Everything here compiles and is tested on every supported OS. Windows-only
//! capture, the Samsung engine binding and Win32 window management live in the
//! `any2odyssey-windows` crate; the desktop GUI lives in `any2odyssey`.

pub mod engine;
pub mod frame_wake;
pub mod fsutil;
pub mod geometry;
pub mod help;
pub mod ipc;
pub mod log;
pub mod log_readiness;
pub mod overlay_settings;
pub mod paths;
pub mod prefs;
pub mod slider;
pub mod status;
pub mod stereo;
pub mod supervisor;
pub mod theme;

/// Product name used in window titles, tray tooltips and the data directory.
pub const APP_NAME: &str = "Any2Odyssey";
/// Package version, kept in sync with the repository `VERSION` file by a test.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The display this program targets. Detection only checks resolution and camera.
pub const TARGET_DISPLAY: &str = "Samsung Odyssey G90XF";

#[cfg(test)]
mod version_tests {
    #[test]
    fn version_file_matches_package_version() {
        let file = include_str!("../../../VERSION").trim();
        assert_eq!(
            file,
            super::VERSION,
            "VERSION file and Cargo version disagree"
        );
    }
}
