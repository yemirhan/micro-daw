//! The Dev view's MIDI monitor (port of `src/hooks/useMidiMonitor.ts`).
//!
//! The MIDI thread only queues raw messages; the UI thread formats + buffers them
//! ([`crate::host::MidiHost::drain_raw`], called by `MidiHost::poll`, with
//! [`crate::host::MidiHost::set_monitor_active`]); the UI drains the buffer every [`FLUSH_INTERVAL_MS`] (TS `setInterval`) into
//! a list capped at [`MAX_EVENTS`]. Clock (`0xF8`) and active sensing (`0xFE`) are dropped (they
//! flood), and nothing is buffered while paused (the TS checks `pausedRef` in the listener).

use mdaw_model::notes::midi_to_note_name;
use serde::Serialize;

pub const MAX_EVENTS: usize = 500;
pub const FLUSH_INTERVAL_MS: u64 = 50;

/// `MidiMonitorEvent` (`types/midi.ts`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorEvent {
    pub id: u64,
    /// Milliseconds (TS `performance.now()`; here the host's clock).
    pub timestamp: f64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    pub status_byte: u8,
    pub data_bytes: Vec<u8>,
    pub raw_data: Vec<u8>,
    pub label: String,
    pub detail: String,
}

/// `CC_NAMES`.
fn cc_name(cc: u8) -> Option<&'static str> {
    Some(match cc {
        1 => "Mod Wheel",
        2 => "Breath",
        7 => "Volume",
        10 => "Pan",
        11 => "Expression",
        64 => "Sustain",
        65 => "Portamento",
        66 => "Sostenuto",
        67 => "Soft Pedal",
        71 => "Resonance",
        74 => "Cutoff",
        91 => "Reverb",
        93 => "Chorus",
        _ => return None,
    })
}

/// `${dataBytes[i]}` (JS prints a missing byte as `undefined`).
fn byte_str(b: Option<u8>) -> String {
    b.map_or_else(|| "undefined".to_owned(), |b| b.to_string())
}

/// `midiToNoteName(dataBytes[i])` (undefined → `undefinedNaN`).
fn note_str(b: Option<u8>) -> String {
    midi_to_note_name(b.map_or(f64::NAN, f64::from))
}

/// `formatEvent(id, statusByte, dataBytes, rawData, timestamp)`.
pub fn format_event(id: u64, raw: &[u8], timestamp: f64) -> MonitorEvent {
    let status = raw.first().copied().unwrap_or(0);
    let data: Vec<u8> = raw.iter().skip(1).copied().collect();
    let d0 = data.first().copied();
    let d1 = data.get(1).copied();
    let channel = (status & 0x0F) + 1;
    let command = status & 0xF0;
    let (kind, label, detail): (&str, &str, String) = match command {
        0x90 => {
            let velocity = d1.unwrap_or(0);
            if velocity == 0 {
                (
                    "noteoff",
                    "Note Off",
                    format!("{} ({})", note_str(d0), byte_str(d0)),
                )
            } else {
                (
                    "noteon",
                    "Note On",
                    format!("{} ({}) vel={velocity}", note_str(d0), byte_str(d0)),
                )
            }
        }
        0x80 => (
            "noteoff",
            "Note Off",
            format!("{} ({})", note_str(d0), byte_str(d0)),
        ),
        0xB0 => {
            let name = d0
                .and_then(cc_name)
                .map_or_else(|| format!("CC#{}", byte_str(d0)), str::to_owned);
            ("controlchange", "CC", format!("{name} = {}", byte_str(d1)))
        }
        0xE0 => {
            // ((d1 << 7) | d0) - 8192 with JS `undefined` → 0 in bit ops
            let v = ((i32::from(d1.unwrap_or(0)) << 7) | i32::from(d0.unwrap_or(0))) - 8192;
            let sign = if v > 0 { "+" } else { "" };
            ("pitchbend", "Pitch Bend", format!("value={sign}{v}"))
        }
        0xD0 => (
            "aftertouch",
            "Ch Pressure",
            format!("pressure={}", byte_str(d0)),
        ),
        0xA0 => (
            "aftertouch",
            "Key Pressure",
            format!(
                "{} ({}) pressure={}",
                note_str(d0),
                byte_str(d0),
                byte_str(d1)
            ),
        ),
        0xC0 => (
            "programchange",
            "Program",
            format!("program={}", byte_str(d0)),
        ),
        _ => match status {
            0xF8 => ("clock", "Clock", String::new()),
            0xFE => ("activesensing", "Active Sense", String::new()),
            0xFA => ("start", "Start", String::new()),
            0xFC => ("stop", "Stop", String::new()),
            0xFB => ("continue", "Continue", String::new()),
            0xF0 => ("sysex", "SysEx", format!("{} bytes", raw.len())),
            _ => ("unknown", "Unknown", format!("status=0x{status:X}")),
        },
    };
    MonitorEvent {
        id,
        timestamp,
        kind: kind.to_owned(),
        channel: (command < 0xF0).then_some(channel),
        status_byte: status,
        data_bytes: data,
        raw_data: raw.to_vec(),
        label: label.to_owned(),
        detail,
    }
}

/// `FilterType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MonitorFilter {
    #[default]
    All,
    NoteOn,
    NoteOff,
    ControlChange,
    PitchBend,
    Aftertouch,
    ProgramChange,
}

impl MonitorFilter {
    fn kind(self) -> Option<&'static str> {
        Some(match self {
            MonitorFilter::All => return None,
            MonitorFilter::NoteOn => "noteon",
            MonitorFilter::NoteOff => "noteoff",
            MonitorFilter::ControlChange => "controlchange",
            MonitorFilter::PitchBend => "pitchbend",
            MonitorFilter::Aftertouch => "aftertouch",
            MonitorFilter::ProgramChange => "programchange",
        })
    }
}

/// The listener side (MIDI thread): ids + the pending buffer.
#[derive(Debug, Default)]
pub struct MonitorBuffer {
    next_id: u64,
    pending: Vec<MonitorEvent>,
    paused: bool,
}

impl MonitorBuffer {
    /// The raw listener body.
    pub fn on_raw(&mut self, raw: &[u8], timestamp: f64) {
        if self.paused {
            return;
        }
        let status = raw.first().copied().unwrap_or(0);
        if status == 0xF8 || status == 0xFE {
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.pending.push(format_event(id, raw, timestamp));
    }
    pub fn take(&mut self) -> Vec<MonitorEvent> {
        std::mem::take(&mut self.pending)
    }
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

/// The UI side: the shown list, paused flag and filter (`useMidiMonitor`'s state).
#[derive(Debug, Default)]
pub struct MidiMonitor {
    events: Vec<MonitorEvent>,
    pub paused: bool,
    pub filter: MonitorFilter,
}

impl MidiMonitor {
    /// The interval's flush: append a drained batch, keep the last [`MAX_EVENTS`].
    pub fn flush(&mut self, batch: Vec<MonitorEvent>) {
        if batch.is_empty() {
            return;
        }
        self.events.extend(batch);
        if self.events.len() > MAX_EVENTS {
            let cut = self.events.len() - MAX_EVENTS;
            self.events.drain(..cut);
        }
    }
    /// `events` after the type filter.
    pub fn events(&self) -> Vec<&MonitorEvent> {
        match self.filter.kind() {
            None => self.events.iter().collect(),
            Some(k) => self.events.iter().filter(|e| e.kind == k).collect(),
        }
    }
    pub fn clear(&mut self) {
        self.events.clear();
    }
}
