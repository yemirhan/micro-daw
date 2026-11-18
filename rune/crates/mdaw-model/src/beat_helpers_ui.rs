//! Small pure helpers that live inside the Beat view's `.tsx` components.

use crate::js::{round_string as js_round_string, to_fixed as js_to_fixed};
use crate::types::{PadIndex, SwingGrid};

/// `swingGridLabel(grid)` (BeatTransport.tsx:125): "8ths" / "16ths".
pub fn swing_grid_label(grid: SwingGrid) -> &'static str {
    match grid {
        SwingGrid::S8 => "8ths",
        SwingGrid::S16 => "16ths",
    }
}

/// `fmtDb(v)` (PadEditor.tsx:49): "+3.0dB", "-6.5dB", "0.0dB".
pub fn fmt_db(v: f64) -> String {
    format!("{}{}dB", if v > 0.0 { "+" } else { "" }, js_to_fixed(v, 1))
}

/// `fmtPan(v)` (PadEditor.tsx:50): "C" near the centre, else "L40" / "R25".
pub fn fmt_pan(v: f64) -> String {
    if v.abs() < 0.005 {
        return "C".to_string();
    }
    format!(
        "{}{}",
        if v < 0.0 { "L" } else { "R" },
        js_round_string(v.abs() * 100.0)
    )
}

/// `fmtPitch(v)` (PadEditor.tsx:51): "+12st", "-3st", "0st".
pub fn fmt_pitch(v: f64) -> String {
    format!("{}{}st", if v > 0.0 { "+" } else { "" }, js_round_string(v))
}

/// `padCount(n)` (MissingPacks.tsx:47): "1 pad" / "3 pads".
pub fn pad_count_label(n: usize) -> String {
    if n == 1 {
        "1 pad".to_string()
    } else {
        format!("{n} pads")
    }
}

/// `bankRows(bank)` (PadGrid.tsx:163): one bank as two rows of four, pad 1 bottom-left.
pub fn bank_rows(bank: char) -> Vec<Vec<PadIndex>> {
    let o = if bank == 'A' { 0 } else { 8 };
    vec![
        vec![o + 4, o + 5, o + 6, o + 7],
        vec![o, o + 1, o + 2, o + 3],
    ]
}

/// Result of `pendingLoadText` (PendingLoadChip.tsx:15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLoadText {
    pub prefix: String,
    pub name: String,
    pub text: String,
    pub title: String,
}

/// `pendingLoadText(info)`: chip text + tooltip for a pending "Get & load".
pub fn pending_load_text(pack_name: &str, waiting_for_recording: bool) -> PendingLoadText {
    if waiting_for_recording {
        let prefix = "After take: ";
        return PendingLoadText {
            prefix: prefix.to_string(),
            name: pack_name.to_string(),
            text: format!("{prefix}{pack_name}"),
            title: format!(
                "{pack_name} is downloaded: its kit loads on these pads when recording stops"
            ),
        };
    }
    let prefix = "Getting ";
    PendingLoadText {
        prefix: prefix.to_string(),
        name: format!("{pack_name}…"),
        text: format!("{prefix}{pack_name}…"),
        title: format!(
            "Getting {pack_name}: its kit loads on these pads when the download is done (choosing another kit cancels it)"
        ),
    }
}
