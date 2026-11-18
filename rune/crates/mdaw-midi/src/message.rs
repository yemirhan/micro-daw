//! Raw MIDI bytes → the events `MidiService` dispatches (webmidi 3.3.1 semantics).
//!
//! - **Note-on with velocity 0** is a note-off (webmidi `_parseEventForStandardMessages`;
//!   `MidiService.dispatchNoteOn` repeats the rule for simulated input).
//! - **Velocity** = `velocityToNormalized(rawAttack)` = `clamp(raw / 127, 0, 1)`.
//! - **Channels** are 1..16 (`e.message.channel`).
//! - **CC**: every control change, channel-mode CCs 120..127 included, as
//!   `(controller.number, rawValue, channel)`.
//! - Everything else (aftertouch, pitch bend, program change, system messages) reaches only the
//!   raw listeners (the MIDI monitor).

/// A parsed channel message the routing core acts on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MidiEvent {
    /// `velocity` 0..1 (never 0: a zero velocity is a [`MidiEvent::NoteOff`]).
    NoteOn {
        note: u8,
        velocity: f64,
        channel: u8,
    },
    NoteOff {
        note: u8,
        channel: u8,
    },
    ControlChange {
        cc: u8,
        value: u8,
        channel: u8,
    },
}

/// `velocityToNormalized(raw)` (`noteHelpers.ts:28-30`).
pub fn velocity_to_normalized(raw: f64) -> f64 {
    (raw / 127.0).clamp(0.0, 1.0)
}

/// The event `MidiService` dispatches for these bytes (None: raw listeners only).
pub fn parse(bytes: &[u8]) -> Option<MidiEvent> {
    let status = *bytes.first()?;
    if !(0x80..0xF0).contains(&status) {
        return None;
    }
    let channel = (status & 0x0F) + 1;
    let d1 = *bytes.get(1)? & 0x7F;
    let d2 = bytes.get(2).map(|b| b & 0x7F);
    match status & 0xF0 {
        0x80 => {
            d2?;
            Some(MidiEvent::NoteOff { note: d1, channel })
        }
        0x90 => {
            let v = d2?;
            if v == 0 {
                Some(MidiEvent::NoteOff { note: d1, channel })
            } else {
                Some(MidiEvent::NoteOn {
                    note: d1,
                    velocity: velocity_to_normalized(f64::from(v)),
                    channel,
                })
            }
        }
        0xB0 => Some(MidiEvent::ControlChange {
            cc: d1,
            value: d2?,
            channel,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_off_and_zero_velocity() {
        assert_eq!(
            parse(&[0x99, 38, 127]),
            Some(MidiEvent::NoteOn {
                note: 38,
                velocity: 1.0,
                channel: 10
            })
        );
        assert_eq!(
            parse(&[0x90, 60, 0]),
            Some(MidiEvent::NoteOff {
                note: 60,
                channel: 1
            })
        );
        assert_eq!(
            parse(&[0x8F, 60, 64]),
            Some(MidiEvent::NoteOff {
                note: 60,
                channel: 16
            })
        );
        assert_eq!(
            parse(&[0xB0, 123, 0]),
            Some(MidiEvent::ControlChange {
                cc: 123,
                value: 0,
                channel: 1
            })
        );
        assert_eq!(parse(&[0xE0, 0, 64]), None);
        assert_eq!(parse(&[0xF8]), None);
        assert_eq!(parse(&[0x90, 60]), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn velocity_is_raw_over_127() {
        for raw in 0..=127u8 {
            let Some(MidiEvent::NoteOn { velocity, .. }) = parse(&[0x90, 1, raw]) else {
                assert_eq!(raw, 0);
                continue;
            };
            assert_eq!(velocity, f64::from(raw) / 127.0);
        }
    }
}
