//! PNG encoding and the ICO container (PNG-compressed entries, 32 bpp).

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Build an ICO file from `(size, png_bytes)` entries. A 256 px entry is
/// recorded with width/height bytes of 0, as the format requires.
pub fn encode_ico(entries: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let count = entries.len() as u16;
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6u32 + 16 * entries.len() as u32;
    for (size, png) in entries {
        let dim = if *size >= 256 { 0u8 } else { *size as u8 };
        out.push(dim);
        out.push(dim);
        out.push(0); // colour count
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bit count
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in entries {
        out.extend_from_slice(png);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ico_directory_layout() {
        let ico = encode_ico(&[(16, vec![1, 2, 3]), (256, vec![9; 10])]);
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(ico[6], 16);
        assert_eq!(ico[22], 0, "256 px entry is encoded as 0");
        let first_size = u32::from_le_bytes(ico[14..18].try_into().unwrap());
        let first_offset = u32::from_le_bytes(ico[18..22].try_into().unwrap());
        assert_eq!(first_size, 3);
        assert_eq!(first_offset, 6 + 32);
        assert_eq!(ico.len(), 6 + 32 + 3 + 10);
    }
}
