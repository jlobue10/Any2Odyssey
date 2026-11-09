//! Any2Odyssey brand asset generator.
//!
//! Port of Any2Odd's `IconMaker.cs` (GPL-3.0-only, Zhandragon): an engraved
//! "2n + 1" seal on a graphite disc with a gold rim, turquoise accents, and a
//! wordmark. Everything is vector-drawn here; no bitmap assets are read.
//! Letters use the bundled Libre Baskerville (SIL OFL 1.1, see `fonts/OFL.txt`).
//!
//! Usage: `any2odyssey-iconmaker <output-folder>` writes
//! `Any2Odyssey.ico`, `Any2Odyssey.png` (256 px) and `Any2Odyssey-wordmark.png`.

mod draw;
mod ico;

use std::path::Path;

pub const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

fn main() {
    let folder = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: any2odyssey-iconmaker <output-folder>");
        std::process::exit(2);
    });
    // `cargo run -p any2odyssey-iconmaker -- --list` once created a folder named
    // `--list` that was then committed; an option-shaped name is never a folder.
    if folder.starts_with('-') {
        eprintln!("usage: any2odyssey-iconmaker <output-folder> (got option-like {folder:?})");
        std::process::exit(2);
    }
    if let Err(e) = generate(Path::new(&folder)) {
        eprintln!("asset generation failed: {e}");
        std::process::exit(1);
    }
}

pub fn generate(folder: &Path) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
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
