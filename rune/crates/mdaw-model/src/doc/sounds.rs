//! The Sound Designer's sounds in the project (`Arrangement::sounds`): save, rename, delete and
//! send to a pad. Each is one undo step; the app re-renders a sound whose recipe changed (an
//! edit, an undo) and the pads that point at it reload.

use crate::sound_design::DesignedSound;
use crate::types::{PAD_COUNT, PadIndex, PadSample, PadSampleSource};

use super::ArrangementDoc;

fn uses(source: &PadSampleSource, id: &str) -> bool {
    matches!(source, PadSampleSource::Sound { sound_id } if sound_id == id)
}

impl ArrangementDoc {
    pub fn get_sounds(&self) -> &[DesignedSound] {
        &self.arrangement.sounds
    }

    pub fn get_sound(&self, id: &str) -> Option<&DesignedSound> {
        self.arrangement.sounds.iter().find(|s| s.id == id)
    }

    /// (drum track id, pad) of every pad playing sound `id`.
    pub fn sound_users(&self, id: &str) -> Vec<(String, PadIndex)> {
        let mut out = Vec::new();
        for t in &self.arrangement.tracks {
            let Some(kit) = t.instrument.kit.as_ref() else {
                continue;
            };
            for (p, pad) in kit.pads.iter().enumerate() {
                if pad.sample.as_ref().is_some_and(|s| uses(&s.source, id)) {
                    out.push((t.id.clone(), p));
                }
            }
        }
        out
    }

    /// Adds or replaces (by id) a sound — undo 'Save Sound'. An unchanged sound writes nothing.
    pub fn save_sound(&mut self, sound: &DesignedSound) {
        if self.get_sound(&sound.id) == Some(sound) || sound.id.is_empty() {
            return;
        }
        self.push_undo_snapshot("Save Sound");
        self.put_sound(sound);
    }

    fn put_sound(&mut self, sound: &DesignedSound) {
        let mut s = sound.clone();
        s.recipe = s.recipe.sanitized();
        match self.arrangement.sounds.iter_mut().find(|x| x.id == s.id) {
            Some(x) => *x = s,
            None => self.arrangement.sounds.push(s),
        }
        self.emit_state_change();
    }

    /// Renames a sound — undo 'Rename Sound'. Pads named after the old name follow.
    pub fn rename_sound(&mut self, id: &str, name: &str) {
        let name = name.trim();
        let Some(old) = self.get_sound(id).map(|s| s.name.clone()) else {
            return;
        };
        if name.is_empty() || name == old {
            return;
        }
        self.push_undo_snapshot("Rename Sound");
        if let Some(s) = self.arrangement.sounds.iter_mut().find(|s| s.id == id) {
            s.name = name.to_owned();
        }
        for i in 0..self.arrangement.tracks.len() {
            let Some(mut kit) = self.arrangement.tracks[i].instrument.kit.clone() else {
                continue;
            };
            let mut hit = false;
            for pad in &mut kit.pads {
                if let Some(smp) = &mut pad.sample
                    && uses(&smp.source, id)
                    && smp.name == old
                {
                    smp.name = name.to_owned();
                    hit = true;
                }
            }
            if hit {
                self.apply_kit(i, kit);
            }
        }
        self.emit_state_change();
    }

    /// Deletes a sound — undo 'Delete Sound' — and clears the pads that played it.
    pub fn remove_sound(&mut self, id: &str) {
        if self.get_sound(id).is_none() && self.sound_users(id).is_empty() {
            return;
        }
        self.push_undo_snapshot("Delete Sound");
        self.arrangement.sounds.retain(|s| s.id != id);
        for i in 0..self.arrangement.tracks.len() {
            let Some(mut kit) = self.arrangement.tracks[i].instrument.kit.clone() else {
                continue;
            };
            let mut hit = false;
            for pad in &mut kit.pads {
                if pad.sample.as_ref().is_some_and(|s| uses(&s.source, id)) {
                    pad.sample = None;
                    hit = true;
                }
            }
            if hit {
                self.apply_kit(i, kit);
            }
        }
        self.emit_state_change();
    }

    /// Saves `sound` and puts it on pad `pad` of drum track `track_id` — one undo step,
    /// 'Send to Pad'. The pad keeps its other settings (gain, pitch...).
    pub fn send_sound_to_pad(&mut self, track_id: &str, pad: PadIndex, sound: &DesignedSound) {
        if self.drum_track_index(track_id).is_none() || pad >= PAD_COUNT || sound.id.is_empty() {
            return;
        }
        self.begin_undo_group("Send to Pad");
        if self.get_sound(&sound.id) != Some(sound) {
            self.push_undo_snapshot("Save Sound");
            self.put_sound(sound);
        }
        let sample = PadSample {
            source: PadSampleSource::Sound {
                sound_id: sound.id.clone(),
            },
            name: sound.name.clone(),
        };
        self.set_pad_sample(track_id, pad, Some(&sample));
        self.end_undo_group();
    }
}
