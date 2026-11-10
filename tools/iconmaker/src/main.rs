//! Any2Odyssey brand asset generator.
//!
//! Port of Any2Odd's `IconMaker.cs` (GPL-3.0-only, Zhandragon): an engraved
//! "2n + 1" seal on a graphite disc with a gold rim, turquoise accents, and a
//! wordmark. Everything is vector-drawn here; no bitmap assets are read.
//! Letters use the bundled Libre Baskerville (SIL OFL 1.1, see `fonts/OFL.txt`).
//!
//! Usage: `any2odyssey-iconmaker <existing-output-folder>` writes
//! `Any2Odyssey.ico`, `Any2Odyssey.png` (256 px) and `Any2Odyssey-wordmark.png`.

mod draw;
mod ico;

use std::path::Path;

pub const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

const USAGE: &str = "usage: any2odyssey-iconmaker <existing-output-folder>";

/// The single output folder this run was asked to write into.
///
/// Exactly one argument, and it must name a folder that already exists:
/// `cargo run -p any2odyssey-iconmaker -- --list` once created a folder named
/// `--list` that was then committed, an empty argument (an unset shell
/// variable) wrote the three files into the current directory, and a
/// misspelt or misplaced `assets` silently became a new folder while the
/// real assets stayed stale. Extra words are refused rather than ignored, so
/// `assets --check` cannot overwrite the assets while looking like a check.
fn output_folder(args: &[std::ffi::OsString]) -> Result<&Path, String> {
    let [folder] = args else {
        return Err(format!("{USAGE} (got {} arguments)", args.len()));
    };
    if folder.is_empty() {
        return Err(format!("{USAGE} (the folder name is empty)"));
    }
    if folder.to_string_lossy().starts_with('-') {
        return Err(format!("{USAGE} (got option-like {folder:?})"));
    }
    let folder = Path::new(folder);
    if !folder.is_dir() {
        return Err(format!(
            "{USAGE} ({} is not an existing folder; run from the repository root or create it first)",
            folder.display()
        ));
    }
    Ok(folder)
}

fn main() {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let folder = match output_folder(&args) {
        Ok(folder) => folder,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if let Err(e) = generate(folder) {
        eprintln!("asset generation failed: {e}");
        std::process::exit(1);
    }
}

pub fn generate(folder: &Path) -> Result<(), String> {
    let mut entries = Vec::new();
    for &size in &ICON_SIZES {
        let rgba = draw::icon_rgba(size);
        let png = ico::encode_png(size, size, &rgba)?;
        if size == 256 {
            std::fs::write(folder.join("Any2Odyssey.png"), &png).map_err(|e| e.to_string())?;
        }
        entries.push((size, png));
    }
    std::fs::write(folder.join("Any2Odyssey.ico"), ico::encode_ico(&entries))
        .map_err(|e| e.to_string())?;
    let (w, h, rgba) = draw::wordmark_rgba();
    std::fs::write(
        folder.join("Any2Odyssey-wordmark.png"),
        ico::encode_png(w, h, &rgba)?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn exactly_one_existing_folder_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let name = dir.path().as_os_str().to_owned();
        assert_eq!(output_folder(std::slice::from_ref(&name)), Ok(dir.path()));
        // Nothing, too much, empty, option-like, missing, or a file.
        assert!(output_folder(&[]).is_err());
        assert!(output_folder(&[name, OsString::from("--check")]).is_err());
        assert!(output_folder(&args(&[""])).is_err());
        assert!(output_folder(&args(&["--list"])).is_err());
        assert!(output_folder(&args(&["-o"])).is_err());
        let missing = dir.path().join("asets");
        assert!(output_folder(&[missing.as_os_str().to_owned()]).is_err());
        assert!(!missing.exists(), "a refused folder is not created");
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(output_folder(&[file.into_os_string()]).is_err());
    }
}
