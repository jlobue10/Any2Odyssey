//! Runs the generator into a temporary folder and validates the three assets.

use std::path::Path;
use std::process::Command;

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("png header");
    let mut buf = vec![0; reader.output_buffer_size().expect("buffer size")];
    let info = reader.next_frame(&mut buf).expect("png frame");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

fn generate(dir: &Path) {
    let status = Command::new(env!("CARGO_BIN_EXE_any2odyssey-iconmaker"))
        .arg(dir)
        .status()
        .expect("run generator");
    assert!(status.success());
}

#[test]
fn generates_icon_png_and_wordmark_with_expected_geometry() {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path());

    let (w, h, px) = decode_png(&std::fs::read(dir.path().join("Any2Odyssey.png")).unwrap());
    assert_eq!((w, h), (256, 256));
    let alpha = |x: u32, y: u32| px[((y * w + x) * 4 + 3) as usize];
    assert_eq!(alpha(0, 0), 0, "corners are transparent");
    assert_eq!(alpha(255, 0), 0);
    assert_eq!(alpha(0, 255), 0);
    assert_eq!(alpha(255, 255), 0);
    assert!(alpha(128, 128) > 240, "centre is opaque");
    assert!(alpha(128, 12) > 240, "rim is opaque");
    let opaque = px.chunks(4).filter(|p| p[3] > 0).count();
    assert!(
        opaque > 40_000 && opaque < 52_000,
        "disc covers ~pi/4 of the square, got {opaque}"
    );

    let (w, h, px) =
        decode_png(&std::fs::read(dir.path().join("Any2Odyssey-wordmark.png")).unwrap());
    assert_eq!((w, h), (1400, 250));
    let drawn = px.chunks(4).filter(|p| p[3] > 0).count();
    assert!(
        drawn > 30_000,
        "wordmark has lettering and emblem, got {drawn} drawn pixels"
    );
    // Lettering occupies the right-hand region.
    let right_drawn = (0..250u32)
        .flat_map(|y| (300..1340u32).map(move |x| (x, y)))
        .filter(|&(x, y)| px[((y * w + x) * 4 + 3) as usize] > 0)
        .count();
    assert!(right_drawn > 15_000, "{right_drawn}");
}

#[test]
fn ico_contains_all_seven_png_entries() {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path());
    let ico = std::fs::read(dir.path().join("Any2Odyssey.ico")).unwrap();
    assert_eq!(&ico[..6], &[0, 0, 1, 0, 7, 0]);
    let expected = [16u32, 24, 32, 48, 64, 128, 256];
    for (i, size) in expected.iter().enumerate() {
        let e = &ico[6 + i * 16..6 + i * 16 + 16];
        let dim = if *size == 256 { 0 } else { *size as u8 };
        assert_eq!((e[0], e[1]), (dim, dim));
        assert_eq!(u16::from_le_bytes([e[6], e[7]]), 32);
        let len = u32::from_le_bytes(e[8..12].try_into().unwrap()) as usize;
        let off = u32::from_le_bytes(e[12..16].try_into().unwrap()) as usize;
        let (w, h, px) = decode_png(&ico[off..off + len]);
        assert_eq!((w, h), (*size, *size), "entry {i}");
        assert_eq!(px[3], 0, "corner transparent in {size} px entry");
        assert!(
            px[((h / 2 * w + w / 2) * 4 + 3) as usize] > 200,
            "centre opaque in {size} px entry"
        );
    }
}
