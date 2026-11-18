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

/// `font-ui` text at `size` px and `weight`.
pub const fn ui(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

/// `font-mono` text (`tabular` digits are the font's own: JetBrains Mono is monospaced).
pub const fn mono(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).mono()
}

/// `font-display` (Syne, the wordmark).
pub const fn display(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Syne"))
}

/// `.daw-label`: 10.5px, bold, 0.08em tracking, upper case, muted.
pub const fn daw_label(size: f32) -> TextStyle {
    TextStyle::new(size, 700.0).tracking(size * 0.08)
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
