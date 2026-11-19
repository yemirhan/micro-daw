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

    /// (track id, region id) of every Song audio region playing sound `id` (a region added
    /// with "Add to song": the sound id with no file path).
    pub fn sound_regions(&self, id: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for t in &self.arrangement.tracks {
            for r in &t.regions {
                if r.audio
                    .as_ref()
                    .is_some_and(|a| a.sample_id == id && a.sample_path.is_empty())
                {
                    out.push((t.id.clone(), r.id.clone()));
                }
            }
        }
        out
    }

    /// Ids of sounds that pads or Song regions play but the project doesn't define (a kit or a
    /// pad brought in from elsewhere: the app copies the recipe from the user library).
    pub fn missing_sounds(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in &self.arrangement.tracks {
            if let Some(kit) = t.instrument.kit.as_ref() {
                for pad in &kit.pads {
                    if let Some(PadSampleSource::Sound { sound_id }) =
                        pad.sample.as_ref().map(|s| &s.source)
                        && self.get_sound(sound_id).is_none()
                        && !out.contains(sound_id)
                    {
                        out.push(sound_id.clone());
                    }
                }
            }
        }
        out
    }

    /// Adds sounds the project's pads already play (see [`Self::missing_sounds`]) without an
    /// undo step of their own: they complete the edit that brought the pads in (undoing that
    /// edit leaves no pad playing them). Sounds already defined are left alone.
    pub fn adopt_sounds(&mut self, sounds: &[DesignedSound]) {
        let mut hit = false;
        for s in sounds {
            if s.id.is_empty() || self.get_sound(&s.id).is_some() {
                continue;
            }
            let mut s = s.clone();
            s.recipe = s.recipe.sanitized();
            self.arrangement.sounds.push(s);
            hit = true;
        }
        if hit {
            self.emit_state_change();
        }
    }

    /// Deletes a sound — undo 'Delete Sound' — clears the pads that played it and removes the
    /// Song regions playing it (no region is left pointing at a sound that's gone).
    pub fn remove_sound(&mut self, id: &str) {
        if self.get_sound(id).is_none()
            && self.sound_users(id).is_empty()
            && self.sound_regions(id).is_empty()
        {
            return;
        }
        self.push_undo_snapshot("Delete Sound");
        self.remove_sound_now(id);
    }

    /// Delete from the Sound Designer: like [`Self::remove_sound`], but `sound` is put in the
    /// undo snapshot even when the project didn't have it (a sound only in the user library),
    /// so undo always brings the sound back (the app then restores its library entry).
    pub fn delete_sound(&mut self, sound: &DesignedSound) {
        if sound.id.is_empty() {
            return;
        }
        if self.get_sound(&sound.id).is_none() {
            let mut s = sound.clone();
            s.recipe = s.recipe.sanitized();
            self.arrangement.sounds.push(s);
        }
        self.push_undo_snapshot("Delete Sound");
        self.remove_sound_now(&sound.id);
    }

    fn remove_sound_now(&mut self, id: &str) {
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
        for t in &mut self.arrangement.tracks {
            t.regions.retain(|r| {
                !r.audio
                    .as_ref()
                    .is_some_and(|a| a.sample_id == id && a.sample_path.is_empty())
            });
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
