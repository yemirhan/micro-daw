//! The app's design tokens, ported 1:1 from `src/index.css` (the "studio instrument" theme):
//! OKLCH surfaces, shadcn tokens, signal colours, radii and the fonts (Manrope, JetBrains Mono,
//! Syne: the same OFL faces the Electron app bundles through @fontsource, converted from its
//! WOFF files to TTF by `tools/woff2ttf.py`).

use std::sync::LazyLock;

use rune::tok::{Family, TextStyle};
use rune::{Color, oklch, oklcha};

/// Every colour token of `:root` in `src/index.css`.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub surface0: Color,
    pub surface1: Color,
    pub surface2: Color,
    pub surface3: Color,
    pub surface4: Color,
    pub fg: Color,
    pub popover: Color,
    pub primary: Color,
    pub primary_fg: Color,
    pub muted_fg: Color,
    pub destructive: Color,
    pub border: Color,
    pub input: Color,
    pub rec: Color,
    pub play: Color,
    pub select: Color,
    pub warn: Color,
    pub hairline: Color,
    pub hairline_strong: Color,
    pub sidebar: Color,
    pub sidebar_border: Color,
    pub intro_bg: Color,
    pub white: Color,
    pub black: Color,
}

pub static T: LazyLock<Theme> = LazyLock::new(|| Theme {
    surface0: oklch(0.180, 0.005, 65.0),
    surface1: oklch(0.212, 0.005, 65.0),
    surface2: oklch(0.245, 0.005, 65.0),
    surface3: oklch(0.285, 0.006, 65.0),
    surface4: oklch(0.330, 0.007, 65.0),
    fg: oklch(0.935, 0.004, 80.0),
    popover: oklch(0.235, 0.005, 65.0),
    primary: oklch(0.74, 0.17, 55.0),
    primary_fg: oklch(0.19, 0.03, 55.0),
    muted_fg: oklch(0.68, 0.008, 70.0),
    destructive: oklch(0.64, 0.21, 27.0),
    border: oklch(0.290, 0.005, 65.0),
    input: oklch(0.320, 0.006, 65.0),
    rec: oklch(0.64, 0.22, 27.0),
    play: oklch(0.76, 0.17, 150.0),
    select: oklch(0.74, 0.12, 230.0),
    warn: oklch(0.83, 0.15, 85.0),
    hairline: oklcha(1.0, 0.0, 0.0, 0.06),
    hairline_strong: oklcha(1.0, 0.0, 0.0, 0.10),
    sidebar: oklch(0.165, 0.005, 65.0),
    sidebar_border: oklch(0.255, 0.005, 65.0),
    intro_bg: oklch(0.165, 0.005, 65.0),
    white: oklch(1.0, 0.0, 0.0),
    black: oklch(0.0, 0.0, 0.0),
});

/// The theme (one dark appearance, like the Electron app's `color-scheme: dark`).
pub fn t() -> &'static Theme {
    &T
}

/// `--radius-sm` / `-md` / `-lg` / `-xl` (4 / 6 / 8 / 10 px).
pub const R_SM: f32 = 4.0;
pub const R_MD: f32 = 6.0;
pub const R_LG: f32 = 8.0;
pub const R_XL: f32 = 10.0;

/// Fonts embedded in the binary (sans slot first: Manrope replaces Rune's Inter).
pub static FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Manrope-400.ttf"),
    include_bytes!("../assets/fonts/Manrope-500.ttf"),
    include_bytes!("../assets/fonts/Manrope-600.ttf"),
    include_bytes!("../assets/fonts/Manrope-700.ttf"),
    include_bytes!("../assets/fonts/Manrope-800.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-400.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-500.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-600.ttf"),
    include_bytes!("../assets/fonts/Syne-700.ttf"),
    include_bytes!("../assets/fonts/Syne-800.ttf"),
];

/// `--font-ui` / `--font-mono`.
pub const FAMILIES: (&str, &str) = ("Manrope", "JetBrains Mono");

/// The line height Chromium gives text of `size` px in the app: Tailwind's named sizes carry
/// their own (`text-xs` 12/16, `text-sm` 14/20, `text-base` 16/24, `text-lg` 18/28, `text-xl`
/// 20/28, `text-2xl` 24/32); arbitrary sizes (`text-[11px]`) inherit `body { line-height: 1.4 }`.
/// Rune's own default (1.25 × size) made every text box shorter than the Electron one.
pub const fn line_height(size: f32) -> f32 {
    if size == 12.0 {
        16.0
    } else if size == 14.0 {
        20.0
    } else if size == 16.0 {
        24.0
    } else if size == 18.0 || size == 20.0 {
        28.0
    } else if size == 24.0 {
        32.0
    } else {
        size * 1.4
    }
}

/// `font-ui` text at `size` px and `weight` (Tailwind / body line height; `.lh` overrides,
/// `leading-none` = `.lh(size)`).
pub const fn ui(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).lh(line_height(size))
}

/// `font-mono` text (`tabular` digits are the font's own: JetBrains Mono is monospaced).
pub const fn mono(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).mono().lh(line_height(size))
}

/// `font-display` (Syne, the wordmark).
pub const fn display(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
        .family(Family::Named("Syne"))
        .lh(line_height(size))
}

/// `.daw-label`: 10.5px, bold, 0.08em tracking, upper case, muted.
pub const fn daw_label(size: f32) -> TextStyle {
    TextStyle::new(size, 700.0)
        .tracking(size * 0.08)
        .lh(line_height(size))
}

/// Parses the CSS colours the model stores (pad colours `oklch(L C H)`, classic-machine accents,
/// pack colours `#rrggbb`): `oklch(...)` with optional `/ alpha`, `#rgb` / `#rrggbb`, else the
/// fallback.
pub fn css_color(s: &str, fallback: Color) -> Color {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("oklch(").and_then(|r| r.strip_suffix(')')) {
        let (main, alpha) = match inner.split_once('/') {
            Some((m, a)) => (m, a.trim().parse::<f32>().ok()),
            None => (inner, None),
        };
        let parts: Vec<f32> = main
            .split_whitespace()
            .filter_map(|p| {
                if let Some(pct) = p.strip_suffix('%') {
                    pct.parse::<f32>().ok().map(|v| v / 100.0)
                } else {
                    p.parse::<f32>().ok()
                }
            })
            .collect();
        if parts.len() == 3 {
            return oklcha(parts[0], parts[1], parts[2], alpha.unwrap_or(1.0));
        }
        return fallback;
    }
    if s.starts_with('#') {
        return rune::color::hex(s).unwrap_or(fallback);
    }
    fallback
}

/// `color-mix(in oklab, c a%, transparent)`-style alpha.
pub fn with_alpha(c: Color, a: f32) -> Color {
    c.alpha(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review finding 1: every embedded weight is its own face of one family (the converted
    /// TTFs carry typographic names 16 / 17), so 400 / 500 / 600 / 700 / 800 all measure
    /// differently; before, 500 drew as 400 and 600 / 800 as 700.
    #[test]
    fn every_weight_resolves_to_its_own_face() {
        let mut ts = rune::text::TextSystem::new(FONTS, FAMILIES);
        let s = "Kick Snare Open HH Low Tom";
        let ui_w: Vec<f32> = [400.0, 500.0, 600.0, 700.0, 800.0]
            .iter()
            .map(|w| ts.measure(s, ui(12.0, *w), 1.0).0)
            .collect();
        for pair in ui_w.windows(2) {
            assert!(pair[1] > pair[0] + 0.3, "Manrope widths {ui_w:?}");
        }
        // JetBrains Mono advances are the same in every weight, so its faces are checked by
        // their names: typographic family 16 = the CSS family, 17 = the weight's style.
        for (font, family, style) in [
            (FONTS[5], "JetBrains Mono", "Regular"),
            (FONTS[6], "JetBrains Mono", "Medium"),
            (FONTS[7], "JetBrains Mono", "SemiBold"),
            (FONTS[1], "Manrope", "Medium"),
            (FONTS[9], "Syne", "ExtraBold"),
        ] {
            assert_eq!(name(font, 16).as_deref(), Some(family));
            assert_eq!(name(font, 17).as_deref(), Some(style));
        }
        // Syne 800 "micro" at 40 px: Chromium draws 184 px (before the fix 700's 172 px).
        let syne: Vec<f32> = [700.0, 800.0]
            .iter()
            .map(|w| ts.measure("micro", display(40.0, *w), 1.0).0)
            .collect();
        assert!(
            (syne[1] - 184.0).abs() < 3.0,
            "Syne 800 micro = {}",
            syne[1]
        );
        assert!(syne[1] > syne[0] + 4.0, "{syne:?}");
    }

    /// A Windows-platform name record of an sfnt.
    fn name(font: &[u8], id: u16) -> Option<String> {
        let be16 = |o: usize| u16::from_be_bytes([font[o], font[o + 1]]) as usize;
        let be32 = |o: usize| u32::from_be_bytes(font[o..o + 4].try_into().unwrap()) as usize;
        let tables = be16(4);
        let name = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&o| &font[o..o + 4] == b"name")
            .map(|o| be32(o + 8))?;
        let (count, strings) = (be16(name + 2), name + be16(name + 4));
        (0..count).map(|i| name + 6 + 12 * i).find_map(|r| {
            (be16(r) == 3 && be16(r + 6) == id as usize).then(|| {
                let (len, off) = (be16(r + 8), be16(r + 10));
                let units: Vec<u16> = font[strings + off..strings + off + len]
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&units)
            })
        })
    }

    #[test]
    fn line_heights_follow_tailwind_and_the_body() {
        assert_eq!(ui(12.0, 400.0).line_height, 16.0); // text-xs
        assert_eq!(ui(14.0, 400.0).line_height, 20.0); // text-sm
        assert_eq!(mono(16.0, 400.0).line_height, 24.0); // text-base
        assert!((ui(11.0, 400.0).line_height - 15.4).abs() < 1e-4); // body 1.4
        assert_eq!(ui(10.0, 400.0).lh(10.0).line_height, 10.0); // leading-none
    }

    #[test]
    fn parses_pad_colours() {
        let c = css_color("oklch(0.62 0.22 25)", Color::WHITE);
        assert_eq!(c, oklch(0.62, 0.22, 25.0));
        let c = css_color("oklch(1 0 0 / 0.16)", Color::WHITE);
        assert_eq!(c, oklcha(1.0, 0.0, 0.0, 0.16));
        assert_eq!(css_color("nonsense", Color::WHITE), Color::WHITE);
        assert_ne!(css_color("#ff8800", Color::WHITE), Color::WHITE);
    }
}
