//! `exportMidi` + the parts of midi-writer-js 3.2.1 it uses (`Track.setTempo`, `addTrackName`,
//! `NoteEvent { pitch: [n], duration: 'T<ticks>', velocity, startTick, channel }`, `Writer`).
//!
//! midi-writer-js behaviour reproduced here (all confirmed by the golden fixture):
//! - Header: format 0 for one track, else 1; division 128.
//! - Each track: tempo meta (`FF 51 03`, `round(60e6 / bpm)`), track name meta (`FF 03`, one byte
//!   per UTF-16 code unit, truncated to its low byte), both with delta 0.
//! - `NoteEvent.tick = startTick || null`: a note whose start tick is 0 (or NaN) is NOT placed at
//!   tick 0 but appended *sequentially* (wait 0 after the previous deferred note's note-off), so
//!   several notes at tick 0 come out one after another. Notes with a non-zero start tick are
//!   merged in by tick afterwards: each note-on / note-off goes after every event whose tick is
//!   ≤ its own (a stable sort of [deferred events, then on/off pairs in note order] by tick).
//! - Every note event carries its status byte (no running status); the note-off is `0x8n` with the
//!   note-on velocity. Velocity percent → `round(v / 100 · 127)`; a falsy velocity (NaN) is 50.
//! - Pitch: `toMidi(p)` = `p` when `0 ≤ p ≤ 127` (fractions truncated by the `Uint8Array`), else
//!   `null` → 0.
//! - Deltas are tick differences to the previous event (meta events count as tick 0).
//! - End of track `00 FF 2F 00` — twice when the track had any non-zero-start note (the
//!   "hacky" rebuild in `mergeExplicitTickEvents` appends one and the outer `buildData` another).

use mdaw_model::arrangement_helpers::drum_note_to_gm;
use mdaw_model::js;
use mdaw_model::types::{Arrangement, InstrumentType};

use super::ExportError;
use super::end_seconds::drum_play_beat;

/// midi-writer-js's default division (ticks per quarter note).
pub const MIDI_TICKS_PER_BEAT: u32 = 128;

/// Largest delta midi-writer-js encodes correctly (4-byte VLQ in its 32-bit buffer).
const MAX_VLQ: u64 = 0x0FFF_FFFF;

/// `exportMidi(arrangement)`: the bytes of the Standard MIDI File the app saves.
pub fn midi_export(arrangement: &Arrangement) -> Result<Vec<u8>, ExportError> {
    let mut tracks: Vec<Vec<u8>> = Vec::new();
    for track in &arrangement.tracks {
        if track.muted || track.instrument.kind == InstrumentType::Audio {
            continue;
        }
        let is_drums = track.instrument.kind == InstrumentType::Drums;
        let channel: u8 = if is_drums { 10 } else { 1 };

        // (absolute beat, note) — drum patterns are exported swung, as they sound.
        let mut all: Vec<(f64, &mdaw_model::types::RegionNote)> = Vec::new();
        for region in &track.regions {
            for note in &region.notes {
                let beat = region.start_beat + note.start_beat;
                let abs = if is_drums {
                    drum_play_beat(arrangement, beat)
                } else {
                    beat
                };
                all.push((abs, note));
            }
        }
        // `allNotes.sort((a, b) => a.absoluteBeat - b.absoluteBeat)` (stable; NaN compares equal).
        all.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut notes = Vec::with_capacity(all.len());
        for (abs, note) in all {
            let start_tick = js::round(abs * MIDI_TICKS_PER_BEAT as f64);
            let duration = js::max(
                1.0,
                js::round(note.duration_beats * MIDI_TICKS_PER_BEAT as f64),
            );
            if !duration.is_finite() {
                return Err(ExportError::InvalidValue(format!(
                    "T{} is not a valid duration.",
                    js::number_to_string(duration)
                )));
            }
            let pitch = if is_drums {
                drum_note_to_gm(note.note, track.instrument.kit.as_ref())
            } else {
                note.note
            };
            let velocity = js::max(1.0, js::min(100.0, js::round(note.velocity * 100.0)));
            notes.push(NoteSpec {
                start_tick,
                duration,
                pitch: pitch_byte(pitch),
                velocity: convert_velocity(velocity),
            });
        }
        tracks.push(build_track(arrangement.bpm, &track.name, channel, &notes)?);
    }
    if tracks.is_empty() {
        return Err(ExportError::NothingToExport);
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"MThd");
    out.extend_from_slice(&[0, 0, 0, 6]);
    out.extend_from_slice(if tracks.len() > 1 { &[0, 1] } else { &[0, 0] });
    out.extend_from_slice(&number_to_bytes(tracks.len() as u64, 2));
    out.extend_from_slice(&number_to_bytes(MIDI_TICKS_PER_BEAT as u64, 2));
    for data in tracks {
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&number_to_bytes(data.len() as u64, 4));
        out.extend_from_slice(&data);
    }
    Ok(out)
}

struct NoteSpec {
    start_tick: f64,
    duration: f64,
    pitch: u8,
    velocity: u8,
}

#[derive(Clone, Copy)]
struct Ev {
    tick: f64,
    on: bool,
    pitch: u8,
    velocity: u8,
}

fn build_track(
    bpm: f64,
    name: &str,
    channel: u8,
    notes: &[NoteSpec],
) -> Result<Vec<u8>, ExportError> {
    // Pass 1: notes starting at a falsy tick, appended sequentially from tick 0.
    let mut events: Vec<Ev> = Vec::with_capacity(notes.len() * 2);
    let mut pointer = 0.0;
    let mut explicit: Vec<&NoteSpec> = Vec::new();
    for n in notes {
        if n.start_tick == 0.0 || n.start_tick.is_nan() {
            events.push(Ev {
                tick: pointer,
                on: true,
                pitch: n.pitch,
                velocity: n.velocity,
            });
            pointer += n.duration;
            events.push(Ev {
                tick: pointer,
                on: false,
                pitch: n.pitch,
                velocity: n.velocity,
            });
        } else {
            explicit.push(n);
        }
    }
    // Merge the explicit-tick notes (already in tick order: the caller sorted by beat, and
    // midi-writer's own sort is stable). Each event goes after every event with tick ≤ its own.
    for n in &explicit {
        if n.start_tick < 0.0 {
            return Err(ExportError::InvalidValue(format!(
                "negative MIDI tick {}",
                js::number_to_string(n.start_tick)
            )));
        }
        let off_tick = n.start_tick + n.duration;
        for (tick, on) in [(n.start_tick, true), (off_tick, false)] {
            let at = events.partition_point(|e| e.tick <= tick);
            events.insert(
                at,
                Ev {
                    tick,
                    on,
                    pitch: n.pitch,
                    velocity: n.velocity,
                },
            );
        }
    }

    let mut data = Vec::with_capacity(32 + name.len() + events.len() * 5);
    // Tempo meta.
    let tempo = js::round(60_000_000.0 / bpm);
    if !(tempo.is_finite() && tempo >= 0.0) {
        return Err(ExportError::InvalidValue(format!(
            "invalid tempo {}",
            js::number_to_string(bpm)
        )));
    }
    data.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03]);
    data.extend_from_slice(&number_to_bytes(tempo as u64, 3));
    // Track name meta: one byte per UTF-16 code unit.
    let units: Vec<u16> = name.encode_utf16().collect();
    data.extend_from_slice(&[0x00, 0xFF, 0x03]);
    vlq(units.len() as u64, &mut data);
    data.extend(units.iter().map(|u| (*u & 0xFF) as u8));
    // Note events.
    let mut prev = 0.0;
    for e in &events {
        let delta = e.tick - prev;
        if !(0.0..=MAX_VLQ as f64).contains(&delta) {
            return Err(ExportError::InvalidValue(format!(
                "MIDI delta {} out of range",
                js::number_to_string(delta)
            )));
        }
        vlq(js::round(delta) as u64, &mut data);
        let status = if e.on { 0x90 } else { 0x80 };
        data.extend_from_slice(&[status | (channel - 1), e.pitch, e.velocity]);
        prev = e.tick;
    }
    const END_OF_TRACK: [u8; 4] = [0x00, 0xFF, 0x2F, 0x00];
    data.extend_from_slice(&END_OF_TRACK);
    if !explicit.is_empty() {
        data.extend_from_slice(&END_OF_TRACK);
    }
    Ok(data)
}

/// `Utils.getPitch(p)` stored into the `Uint8Array`.
fn pitch_byte(p: f64) -> u8 {
    if (0.0..=127.0).contains(&p) {
        p as u8
    } else {
        0
    }
}

/// `Utils.convertVelocity` of the clamped 1..100 percent (NaN → midi-writer's default 50).
fn convert_velocity(v: f64) -> u8 {
    let v = if v.is_nan() || v == 0.0 { 50.0 } else { v };
    let v = if v > 100.0 { 100.0 } else { v };
    js::round(v / 100.0 * 127.0) as u8
}

/// `Utils.numberToVariableLength`.
pub(crate) fn vlq(mut value: u64, out: &mut Vec<u8>) {
    let mut buf = [0u8; 10];
    let mut i = buf.len() - 1;
    buf[i] = (value & 0x7F) as u8;
    value >>= 7;
    while value > 0 {
        i -= 1;
        buf[i] = ((value & 0x7F) as u8) | 0x80;
        value >>= 7;
    }
    out.extend_from_slice(&buf[i..]);
}

/// `Utils.numberToBytes(number, bytesNeeded)`: big-endian, minimal length, left-padded with zeros
/// to `bytes_needed` (never truncated).
pub(crate) fn number_to_bytes(value: u64, bytes_needed: usize) -> Vec<u8> {
    let mut bytes: Vec<u8> = value
        .to_be_bytes()
        .into_iter()
        .skip_while(|b| *b == 0)
        .collect();
    if bytes.is_empty() {
        bytes.push(0);
    }
    while bytes.len() < bytes_needed {
        bytes.insert(0, 0);
    }
    bytes
}
