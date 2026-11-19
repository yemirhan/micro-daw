//! The Sound Designer's live preview: while the Samples page shows, a small polyphonic synth on
//! the engine's preview slot plays the sound being edited from MIDI keys and pads (the drum and
//! synth outputs are muted meanwhile, like the old Generate tab). Each voice is the same
//! [`mdaw_dsp::sound_design::Voice`] the offline render uses, so what a key plays is exactly
//! the rendered sound, transposed: C4 (60) plays it as designed; pads (MIDI channel 10) always
//! play it as designed. Sounds are one-shots: a key's release doesn't cut them.
//!
//! The recipe reaches the audio thread through a mutex the voice side only `try_lock`s
//! (allocation- and wait-free there); a note starts with the recipe it last saw.

use std::sync::{Arc, Mutex};

use mdaw_dsp::sound_design::Voice;
use mdaw_engine::contract::Instrument;
use mdaw_engine::core::{SynthInstrument, SynthNoteOp};
use mdaw_model::sound_design::SoundRecipe;

use crate::app::App;

const VOICES: usize = 8;
/// The key that plays a sound as designed.
pub const ROOT: f64 = 60.0;

/// (revision, recipe) shared with the audio thread.
pub type Shared = Arc<Mutex<(u64, SoundRecipe)>>;

/// The preview synth (audio side).
pub struct SoundPreviewSynth {
    voices: Vec<Voice>,
    /// Age of each voice's note (the oldest is stolen).
    ages: Vec<u64>,
    clock: u64,
    recipe: SoundRecipe,
    seen: u64,
    shared: Shared,
}

impl SoundPreviewSynth {
    pub fn new(sample_rate: u32, shared: Shared) -> Self {
        let recipe = shared.lock().map(|g| g.1).unwrap_or_default();
        Self {
            voices: (0..VOICES)
                .map(|_| Voice::new(sample_rate as f64))
                .collect(),
            ages: vec![0; VOICES],
            clock: 0,
            recipe,
            seen: 0,
            shared,
        }
    }

    fn refresh(&mut self) {
        if let Ok(g) = self.shared.try_lock()
            && g.0 != self.seen
        {
            self.seen = g.0;
            self.recipe = g.1;
        }
    }
}

impl Instrument for SoundPreviewSynth {
    fn process(&mut self, _frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        for v in &mut self.voices {
            if !v.is_active() {
                continue;
            }
            for k in 0..n {
                let y = v.next_sample();
                left[k] += y;
                right[k] += y;
            }
        }
    }
}

impl SynthInstrument for SoundPreviewSynth {
    fn note_on(&mut self, note: f64, velocity: f64, _time: f64) {
        self.refresh();
        self.clock += 1;
        let i = self
            .voices
            .iter()
            .position(|v| !v.is_active())
            .unwrap_or_else(|| {
                (0..VOICES)
                    .min_by_key(|&i| self.ages[i])
                    .unwrap_or_default()
            });
        self.ages[i] = self.clock;
        self.voices[i].start(&self.recipe, note - ROOT, velocity);
    }
    /// One-shots: a release doesn't cut the sound.
    fn note_off(&mut self, _note: f64, _time: f64) {}
    fn release_all(&mut self, _time: f64) {
        for v in &mut self.voices {
            v.stop();
        }
    }
}

/// The UI side while mounted.
struct Preview {
    shared: Shared,
    /// MIDI rows from the raw listener: (status, data1, data2).
    midi: Arc<Mutex<Vec<[u8; 3]>>>,
    listener: u64,
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<Preview>> = const { std::cell::RefCell::new(None) };
}

/// Mount: mute the live synth and drums (the preview's own mute), put the synth on the preview
/// slot, listen to MIDI.
pub fn mount(s: &mut App, recipe: SoundRecipe) {
    let shared: Shared = Arc::new(Mutex::new((1, recipe)));
    let synth = SoundPreviewSynth::new(s.svc.engine.sample_rate(), shared.clone());
    s.svc.engine.set_preview_instrument(Some(Box::new(synth)));
    s.svc.with_router(|r, out| r.set_preview_muted(true, out));
    let midi: Arc<Mutex<Vec<[u8; 3]>>> = Arc::default();
    let q = midi.clone();
    let listener = s
        .svc
        .midi
        .host
        .add_raw_listener(Box::new(move |b: &[u8], _| {
            if b.len() >= 3 {
                q.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push([b[0], b[1], b[2]]);
            }
        }));
    CURRENT.with(|c| {
        *c.borrow_mut() = Some(Preview {
            shared,
            midi,
            listener,
        })
    });
}

/// Unmount: remove the synth, lift the preview mute (the user's own mute is untouched).
pub fn unmount(s: &mut App) {
    let Some(p) = CURRENT.with(|c| c.borrow_mut().take()) else {
        return;
    };
    s.svc.midi.host.remove_raw_listener(p.listener);
    s.svc.engine.set_preview_instrument(None);
    s.svc.with_router(|r, out| r.set_preview_muted(false, out));
}

/// The recipe new notes play.
pub fn set_recipe(recipe: &SoundRecipe) {
    CURRENT.with(|c| {
        if let Some(p) = c.borrow().as_ref() {
            let mut g = p.shared.lock().unwrap_or_else(|e| e.into_inner());
            if g.1 != *recipe {
                g.0 += 1;
                g.1 = *recipe;
            }
        }
    });
}

/// A note-on: (key, velocity 0..1). Pads (channel 10) play at [`ROOT`].
pub type NoteOn = (f64, f64);

/// The MIDI note-ons queued since the last call (note-offs are dropped: one-shots).
pub fn take_note_ons() -> Vec<NoteOn> {
    let rows = CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .map(|p| std::mem::take(&mut *p.midi.lock().unwrap_or_else(|e| e.into_inner())))
            .unwrap_or_default()
    });
    rows.into_iter()
        .filter(|[st, _, v]| st & 0xF0 == 0x90 && *v > 0)
        .map(|[st, n, v]| {
            let key = if st & 0x0F == 9 { ROOT } else { n as f64 };
            (key, v as f64 / 127.0)
        })
        .collect()
}

/// Plays the synth now.
pub fn play(s: &mut App, note: f64, velocity: f64) {
    s.svc
        .engine
        .preview_synth_note(SynthNoteOp::On { note, velocity });
}

/// Queues a raw MIDI row as if the device sent it (tests).
pub fn feed(bytes: [u8; 3]) {
    CURRENT.with(|c| {
        if let Some(p) = c.borrow().as_ref() {
            p.midi.lock().unwrap_or_else(|e| e.into_inner()).push(bytes);
        }
    });
}

/// The preview is mounted (tests).
pub fn mounted() -> bool {
    CURRENT.with(|c| c.borrow().is_some())
}

/// The recipe the synth would play next (tests).
pub fn current_recipe() -> Option<SoundRecipe> {
    CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .map(|p| p.shared.lock().map(|g| g.1).unwrap_or_default())
    })
}
