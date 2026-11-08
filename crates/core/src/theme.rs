//! The ivory/graphite, gold and turquoise palette used by the desktop controls.
//! Colour values are identical to Any2Odd's light and night palettes.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn hex(v: u32) -> Rgb {
        Rgb(
            ((v >> 16) & 0xff) as u8,
            ((v >> 8) & 0xff) as u8,
            (v & 0xff) as u8,
        )
    }
    /// Linear blend towards `other` by `amount` in 0..=1.
    pub fn blend(self, other: Rgb, amount: f32) -> Rgb {
        let mix = |a: u8, b: u8| {
            (a as f32 + (b as f32 - a as f32) * amount)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Rgb(
            mix(self.0, other.0),
            mix(self.1, other.1),
            mix(self.2, other.2),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub background: Rgb,
    pub panel: Rgb,
    pub header: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub gold: Rgb,
    pub shine: Rgb,
    pub border: Rgb,
    pub button: Rgb,
    pub hover: Rgb,
}

pub const LIGHT: Palette = Palette {
    dark: false,
    background: Rgb::hex(0xEEE8DC),
    panel: Rgb::hex(0xF8F4EA),
    header: Rgb::hex(0xDED0B4),
    text: Rgb::hex(0x232727),
    muted: Rgb::hex(0x625E54),
    accent: Rgb::hex(0x236F7C),
    gold: Rgb::hex(0x967028),
    shine: Rgb::hex(0xE7CE90),
    border: Rgb::hex(0x654A19),
    button: Rgb::hex(0xE6DCC9),
    hover: Rgb::hex(0xD7C7A8),
};

pub const NIGHT: Palette = Palette {
    dark: true,
    background: Rgb::hex(0x111315),
    panel: Rgb::hex(0x1C1E20),
    header: Rgb::hex(0x302D27),
    text: Rgb::hex(0xF2ECE2),
    muted: Rgb::hex(0xB7AFA4),
    accent: Rgb::hex(0x7CC4D3),
    gold: Rgb::hex(0xB0863A),
    shine: Rgb::hex(0xD6BA70),
    border: Rgb::hex(0xBC9451),
    button: Rgb::hex(0x2A2D2F),
    hover: Rgb::hex(0x363A3D),
};

/// The primary-action button fill, the same in both themes.
pub const PRIMARY_BUTTON: Rgb = Rgb(29, 104, 119);
pub const PRIMARY_BUTTON_HOVER: Rgb = Rgb(62, 145, 162);
pub const PRIMARY_BUTTON_TEXT: Rgb = Rgb(250, 245, 230);

impl Palette {
    pub const fn for_dark(dark: bool) -> Palette {
        if dark {
            NIGHT
        } else {
            LIGHT
        }
    }
    /// Shadow colour drawn under ornamental borders.
    pub const fn frame_shadow(&self) -> Rgb {
        if self.dark {
            Rgb(10, 12, 13)
        } else {
            Rgb(51, 43, 29)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_decoding_and_blend() {
        assert_eq!(Rgb::hex(0xEEE8DC), Rgb(0xEE, 0xE8, 0xDC));
        assert_eq!(
            Rgb(0, 0, 0).blend(Rgb(255, 255, 255), 0.5),
            Rgb(128, 128, 128)
        );
        assert_eq!(Rgb(10, 20, 30).blend(Rgb(0, 0, 0), 0.0), Rgb(10, 20, 30));
    }

    #[test]
    fn palettes_are_distinct_and_selectable() {
        assert_ne!(LIGHT, NIGHT);
        assert!(Palette::for_dark(true).dark);
        assert_eq!(Palette::for_dark(false), LIGHT);
        assert_eq!(NIGHT.gold, Rgb::hex(0xB0863A));
    }
}
