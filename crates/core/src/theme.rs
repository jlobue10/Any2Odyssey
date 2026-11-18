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
pub const PRIMARY_BUTTON_TEXT: Rgb = Rgb(250, 245, 230);

impl Palette {
    pub const fn for_dark(dark: bool) -> Palette {
        if dark {
            NIGHT
        } else {
            LIGHT
        }
    }
    /// The primary button's fill under the pointer. Its text has to stay
    /// readable on it (4.5:1, also on the lighter band across the button's
    /// upper half): the lighter teal used before, (62, 145, 162) in both
    /// themes, left 3.3:1. No lighter fill passes on the light theme, where
    /// the band is strongest, so it darkens there, as the other buttons do.
    pub const fn primary_hover(&self) -> Rgb {
        if self.dark {
            Rgb(40, 117, 133)
        } else {
            Rgb(25, 88, 101)
        }
    }
    /// The fill behind a selected list entry and behind selected text. The
    /// text on it keeps the theme's text colour, so it is the accent taken
    /// most of the way to the panel: on the accent itself the text had a
    /// contrast of 2.6:1 (light) and 1.7:1 (dark).
    pub fn selection(&self) -> Rgb {
        self.accent.blend(self.panel, 0.55)
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

/// Contrast of two colours as WCAG 2 defines it: 1 (none) to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    fn luminance(c: Rgb) -> f64 {
        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
    }
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every pair of text and background the controls paint, in both themes,
    /// meets WCAG's 4.5:1 for body text.
    #[test]
    fn text_is_readable_on_every_background_it_is_painted_on() {
        for p in [LIGHT, NIGHT] {
            let theme = if p.dark { "dark" } else { "light" };
            // The upper half of a button carries a lighter band (`gold_button`).
            let band = |fill: Rgb| {
                let top = fill.blend(Rgb(255, 255, 255), if p.dark { 0.03 } else { 0.18 });
                fill.blend(top, 0.55)
            };
            let pairs = [
                ("text on the page", p.text, p.background),
                ("text on a card", p.text, p.panel),
                ("text on the title bar", p.text, p.header),
                ("text on a button", p.text, band(p.button)),
                ("text on a button under the pointer", p.text, band(p.hover)),
                ("small print on the page", p.muted, p.background),
                ("small print on a card", p.muted, p.panel),
                ("a disabled button", p.muted, band(p.button)),
                ("headings and links", p.accent, p.panel),
                ("capital headings", p.border, p.panel),
                (
                    "the primary button",
                    PRIMARY_BUTTON_TEXT,
                    band(PRIMARY_BUTTON),
                ),
                (
                    "the primary button under the pointer",
                    PRIMARY_BUTTON_TEXT,
                    band(p.primary_hover()),
                ),
                ("a selected list entry", p.text, p.selection()),
            ];
            for (what, text, background) in pairs {
                let ratio = contrast(text, background);
                assert!(ratio >= 4.5, "{theme}: {what} has a contrast of {ratio:.2}");
            }
            // What these two replaced, so the measure is known to measure.
            assert!(contrast(p.text, p.accent) < 3.0, "{theme}");
            assert!(contrast(PRIMARY_BUTTON_TEXT, Rgb(62, 145, 162)) < 3.5);
            // The hover fill is still a different fill.
            assert_ne!(p.primary_hover(), PRIMARY_BUTTON);
        }
        assert!((contrast(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
        assert!((contrast(Rgb(119, 119, 119), Rgb(255, 255, 255)) - 4.48).abs() < 0.01);
    }

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
