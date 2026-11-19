//! Phase 9's views: the Samples (Sound Designer), Packs, Settings and Developer pages, the dialogs, the instrument
//! rack (Live FX strip, synth editor, instrument selector). State and actions: `crate::views`.

pub mod designer;
pub mod dev;
pub mod dialogs;
pub mod kit;
pub mod packs;
pub mod rack;
pub mod samples;
pub mod settings;

pub use rack::knob_value;

use rune::{Cx, El};

use crate::app::{App, AppMode};
use crate::views::{Dialog, P9Menu};

/// The page of a non-DAW mode.
pub fn page(s: &App, cx: &mut Cx<App>) -> El {
    match s.mode {
        AppMode::Samples => designer::view(s, cx),
        AppMode::Packs => packs::view(s, cx),
        AppMode::Settings => settings::view(s),
        AppMode::Dev => dev::view(s),
        AppMode::Daw => rune::el(),
    }
}

/// An open phase-9 select / popover.
pub fn menu(s: &App, m: P9Menu) -> El {
    match m {
        P9Menu::SendToPad => designer::send_menu(s),
        P9Menu::DevFilter => dev::filter_menu(s),
        P9Menu::BufferSize => settings::buffer_menu(s),
        P9Menu::LiveFilterType => rack::filter_type_menu(s),
        P9Menu::LiveLfoWave => rack::lfo_wave_menu(s),
        P9Menu::PadChannel => dialogs::pad_channel_menu(s),
        P9Menu::KnobTarget(i) => dialogs::knob_target_menu(s, i),
        P9Menu::Instrument => rack::instrument_menu(s),
    }
}

/// The open dialog (drawn above everything; the template picker and export dialog only in the
/// DAW mode, like `App.tsx`).
pub fn dialog(s: &App, win: [f32; 2]) -> Option<El> {
    let d = s.p9.dialog?;
    match d {
        Dialog::Palette => Some(dialogs::palette(s, win)),
        Dialog::Export if s.mode == AppMode::Daw => Some(dialogs::export(s)),
        Dialog::Templates if s.mode == AppMode::Daw => Some(dialogs::template_picker(s, win)),
        Dialog::Controller => Some(dialogs::controller(s, win)),
        _ => None,
    }
}
