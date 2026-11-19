//! The instrument bank, verbatim from `src/audio/presets.ts` (`SYNTH_PRESETS`, `FALLBACK_PIANO`)
//! with the same builders (`sub`, `fm`, `pluck`, `sampler`, `osc`, `drawbars`): every builder
//! fills the engine defaults, then the preset overrides fields one by one (the TS spreads).
//!
//! BACKWARD COMPATIBILITY: projects store `presetIndex`; slot i keeps the musical role of the old
//! preset i, new sounds start at 36, and every preset has a stable `id`.

use std::sync::OnceLock;

use super::bank_builders::*;
use super::presets::*;

use OscWave::{Pulse, Sawtooth as Saw, Sine, Square, Triangle};
use PresetCategory as C;
use PresetVibe as V;

fn build_bank() -> Vec<SynthPreset> {
    vec![
        // 0 (was "Classic")
        sampler(("grand-piano", "Grand Piano", C::Keys, V::Organic), |p| {
            desc(&mut p.meta, "Sampled concert grand (Salamander)");
            p.meta.gain = 3.5;
            p.release = 0.5;
            p.velocity_curve = 1.5;
            p.meta.character.tone = 16000.0;
        }),
        // 1 (was "Bright")
        sub(("saw-lead", "Saw Lead", C::Lead, V::Bright), |p| {
            p.meta.polyphony = 8.0;
            p.meta.gain = 2.5;
            p.osc_a = osc(Saw, 1.0).unison(3.0, 14.0);
            p.osc_b = osc(Saw, 0.55).fine(7.0);
            p.sub.level = 0.25;
            p.sub.wave = SubWave::Square;
            filt(&mut p.filter, 2400.0, 3.0, 0.6, 2.2, 0.5);
            p.filter_env = adsr(0.005, 0.35, 0.45, 0.3);
            p.amp = amp(0.003, 0.2, 0.85, 0.25, 0.5);
            p.lfo = lfo(5.5, 12.0, 0.0, 0.45);
            p.meta.character.chorus = 0.15;
        }),
        // 2 (was "FM")
        SynthPreset::Fm(fm(("fm-keys", "FM Keys", C::Keys, V::Bright), |p| {
            desc(&mut p.meta, "Glassy DX-style electric piano");
            fm_core(
                p,
                -2.5,
                1.0,
                3.2,
                0.85,
                0.6,
                adsr(0.001, 1.1, 0.12, 0.3),
                amp(0.002, 3.2, 0.0, 0.35, 0.7),
            );
            p.ping = ping(14.0, 0.22, 0.07);
            p.detune = 3.0;
            p.meta.character.chorus = 0.3;
        })),
        // 3 (was "Warm")
        sub(("warm-pad", "Warm Pad", C::Pad, V::Chill), |p| {
            p.meta.polyphony = 10.0;
            p.meta.gain = 2.0;
            p.osc_a = osc(Saw, 1.0).unison(3.0, 18.0);
            p.osc_b = osc(Triangle, 0.5).semi(-12.0);
            filt(&mut p.filter, 850.0, 1.5, 0.3, 1.2, 0.3);
            p.filter_env = adsr(0.8, 1.5, 0.6, 1.5);
            p.amp = amp(0.6, 1.0, 0.85, 1.6, 0.35);
            p.lfo = lfo(0.25, 0.0, 300.0, 0.0);
            p.meta.character.chorus = 0.5;
        }),
        // 4 (was "AM")
        sub(("sync-lead", "Sync Lead", C::Lead, V::Aggressive), |p| {
            p.meta.gain = -2.0;
            desc(&mut p.meta, "Screaming resonant sweep");
            p.osc_a = osc(Saw, 1.0);
            p.osc_b = osc(Saw, 0.6).semi(12.0).fine(5.0);
            filt(&mut p.filter, 1100.0, 9.0, 0.7, 3.5, 0.5);
            p.filter.rolloff = -12.0;
            p.filter_env = adsr(0.002, 0.5, 0.3, 0.25);
            p.amp = amp(0.003, 0.2, 0.9, 0.2, 0.4);
            p.lfo = lfo(5.8, 14.0, 0.0, 0.35);
            p.meta.polyphony = 8.0;
            p.meta.character.drive = 0.35;
        }),
        // 5 (was "Pluck")
        sub(("synth-pluck", "Synth Pluck", C::Pluck, V::Bright), |p| {
            p.meta.polyphony = 12.0;
            p.meta.gain = 5.0;
            p.osc_a = osc(Saw, 1.0).unison(2.0, 10.0);
            p.osc_b = osc(Square, 0.35).semi(12.0);
            filt(&mut p.filter, 480.0, 3.0, 0.6, 4.0, 0.7);
            p.filter_env = adsr(0.001, 0.22, 0.0, 0.2);
            p.amp = amp(0.001, 0.55, 0.0, 0.3, 0.7);
            p.meta.character.chorus = 0.25;
        }),
        // 6 (was "Pad")
        sub(("supersaw-pad", "Supersaw Pad", C::Pad, V::Dreamy), |p| {
            p.meta.polyphony = 8.0;
            p.meta.gain = 1.0;
            p.osc_a = osc(Saw, 1.0).unison(7.0, 36.0);
            p.osc_b = osc(Saw, 0.35).semi(12.0).unison(5.0, 24.0);
            filt(&mut p.filter, 2000.0, 1.0, 0.3, 0.8, 0.3);
            p.filter_env = adsr(1.0, 2.0, 0.7, 2.0);
            p.amp = amp(0.5, 1.0, 0.9, 2.2, 0.3);
            p.lfo = lfo(0.15, 0.0, 200.0, 0.0);
            p.meta.character.chorus = 0.35;
        }),
        // 7 (was "Bass")
        sub(("analog-bass", "Analog Bass", C::Bass, V::Punchy), |p| {
            p.meta.gain = -2.0;
            p.osc_a = osc(Saw, 1.0);
            p.osc_b = osc(Square, 0.5).fine(-8.0);
            p.sub.level = 0.6;
            filt(&mut p.filter, 340.0, 3.0, 0.4, 2.6, 0.6);
            p.filter_env = adsr(0.002, 0.25, 0.25, 0.15);
            p.amp = amp(0.002, 0.3, 0.9, 0.12, 0.5);
            p.meta.polyphony = 8.0;
            p.meta.character.drive = 0.2;
        }),
        // 8 (was "Sub Bass")
        sub(("sub-bass", "Sub Bass", C::Bass, V::Dark), |p| {
            p.meta.gain = -5.0;
            p.osc_a = osc(Sine, 1.0);
            p.osc_b = osc(Triangle, 0.25);
            filt(&mut p.filter, 320.0, 0.5, 0.5, 0.5, 0.3);
            p.filter_env = adsr(0.003, 0.2, 0.3, 0.1);
            p.amp = amp(0.004, 0.2, 1.0, 0.12, 0.4);
            p.meta.polyphony = 8.0;
            p.meta.character.drive = 0.15;
        }),
        // 9 (was "Organ")
        sub(("drawbar-organ", "Drawbar Organ", C::Keys, V::Chill), |p| {
            p.meta.polyphony = 16.0;
            p.meta.gain = 4.0;
            desc(
                &mut p.meta,
                "Tonewheel organ, 888000000-ish with click and Leslie",
            );
            p.osc_a = osc(Sine, 1.0).partials(drawbars(0.9, 0.7, 0.45, 0.4, 0.0, 0.2, 0.25));
            p.sub.level = 0.55;
            p.noise = NoiseParams {
                level: 0.25,
                kind: NoiseType::White,
                decay: 0.006,
            };
            filt(&mut p.filter, 9000.0, 0.3, 0.0, 0.0, 0.0);
            p.filter.rolloff = -12.0;
            p.amp = amp(0.004, 0.05, 1.0, 0.06, 0.1);
            p.lfo = lfo(6.4, 7.0, 0.0, 0.0);
            ch(&mut p.meta.character, 0.12, 0.45, 0.18);
            p.meta.character.tremolo_rate = 6.4;
        }),
        // 10 (was "Lead")
        sub(("square-lead", "Square Lead", C::Lead, V::Punchy), |p| {
            p.meta.gain = -2.5;
            p.osc_a = osc(Square, 1.0).unison(2.0, 10.0);
            p.osc_b = osc(Square, 0.4).semi(-12.0);
            filt(&mut p.filter, 2800.0, 2.0, 0.5, 1.5, 0.5);
            p.filter_env = adsr(0.004, 0.3, 0.5, 0.25);
            p.amp = amp(0.003, 0.2, 0.85, 0.2, 0.5);
            p.lfo = lfo(5.5, 15.0, 0.0, 0.3);
            p.meta.polyphony = 8.0;
        }),
        // 11 (was "Bell")
        SynthPreset::Fm(fm(
            ("tubular-bell", "Tubular Bell", C::Bell, V::Bright),
            |p| {
                fm_core(
                    p,
                    -2.0,
                    3.5,
                    4.0,
                    0.7,
                    0.4,
                    adsr(0.001, 2.5, 0.1, 1.5),
                    amp(0.001, 4.5, 0.0, 2.5, 0.7),
                );
                p.ping = ping(2.76, 0.3, 0.6);
            },
        )),
        // 12 (was "Strings")
        sub(
            ("string-ensemble", "String Ensemble", C::Pad, V::Dreamy),
            |p| {
                p.meta.polyphony = 10.0;
                p.meta.gain = 1.0;
                p.osc_a = osc(Saw, 1.0).unison(5.0, 22.0);
                p.osc_b = osc(Pulse, 0.3).semi(12.0).width(0.3);
                filt(&mut p.filter, 3000.0, 0.5, 0.5, 0.6, 0.4);
                p.filter_env = adsr(0.4, 1.0, 0.7, 0.8);
                p.amp = amp(0.35, 0.5, 0.9, 0.9, 0.5);
                p.lfo = lfo(5.0, 7.0, 0.0, 0.6);
                p.meta.character.chorus = 0.6;
            },
        ),
        // 13 (was "Electric Piano")
        SynthPreset::Fm(fm(("rhodes", "Rhodes EP", C::Keys, V::Chill), |p| {
            desc(
                &mut p.meta,
                "Tine electric piano with stereo tremolo; play harder for bark",
            );
            fm_core(
                p,
                6.0,
                1.0,
                2.2,
                1.0,
                0.6,
                adsr(0.001, 0.9, 0.12, 0.4),
                amp(0.002, 3.5, 0.0, 0.4, 0.75),
            );
            p.ping = ping(14.0, 0.2, 0.05);
            p.detune = 2.0;
            ch(&mut p.meta.character, 0.08, 0.15, 0.35);
            p.meta.character.tremolo_rate = 4.5;
        })),
        // 14 (was "Brass")
        sub(("synth-brass", "Synth Brass", C::Brass, V::Punchy), |p| {
            p.meta.polyphony = 10.0;
            p.meta.gain = 1.5;
            p.osc_a = osc(Saw, 1.0).unison(3.0, 12.0);
            p.osc_b = osc(Saw, 0.7).fine(-10.0);
            filt(&mut p.filter, 700.0, 1.5, 0.5, 2.2, 0.6);
            p.filter_env = adsr(0.06, 0.5, 0.45, 0.3);
            p.amp = amp(0.03, 0.3, 0.85, 0.25, 0.6);
            p.lfo = lfo(5.5, 6.0, 0.0, 0.5);
            p.meta.character.chorus = 0.2;
        }),
        // 15 (was "DX Piano")
        sampler(("bright-piano", "Bright Piano", C::Keys, V::Bright), |p| {
            desc(&mut p.meta, "Grand piano voiced for pop/rock mixes");
            p.meta.gain = 3.0;
            p.release = 0.4;
            p.velocity_curve = 1.3;
            p.meta.character.presence = 5.0;
            p.meta.character.drive = 0.05;
        }),
        // 16 (was "Marimba")
        SynthPreset::Fm(fm(("marimba", "Marimba", C::Bell, V::Organic), |p| {
            fm_core(
                p,
                1.5,
                4.0,
                1.6,
                0.8,
                0.5,
                adsr(0.001, 0.12, 0.0, 0.1),
                amp(0.001, 0.8, 0.0, 0.35, 0.7),
            );
            p.ping = ping(10.0, 0.1, 0.02);
        })),
        // 17 (was "FM Bell")
        SynthPreset::Fm(fm(("music-box", "Music Box", C::Bell, V::Bright), |p| {
            fm_core(
                p,
                -1.0,
                7.0,
                1.2,
                0.6,
                0.3,
                adsr(0.001, 0.15, 0.0, 0.2),
                amp(0.001, 2.2, 0.0, 1.2, 0.6),
            );
            p.ping = ping(4.2, 0.25, 0.25);
        })),
        // 18 (was "Glass Bell")
        SynthPreset::Fm(fm(("vibraphone", "Vibraphone", C::Bell, V::Dreamy), |p| {
            fm_core(
                p,
                3.5,
                4.0,
                0.8,
                0.7,
                0.5,
                adsr(0.001, 0.3, 0.0, 0.3),
                amp(0.001, 3.5, 0.0, 1.2, 0.7),
            );
            p.ping = ping(10.0, 0.08, 0.03);
            p.meta.character.tremolo = 0.45;
            p.meta.character.tremolo_rate = 5.2;
        })),
        // 19 (was "FM Bass")
        SynthPreset::Fm(fm(("fm-bass", "FM Bass", C::Bass, V::Dark), |p| {
            p.meta.polyphony = 4.0;
            fm_core(
                p,
                -3.5,
                1.0,
                3.0,
                0.8,
                0.0,
                adsr(0.001, 0.25, 0.2, 0.1),
                amp(0.002, 0.8, 0.7, 0.1, 0.5),
            );
            p.meta.character.drive = 0.1;
        })),
        // 20 (was "Acid Bass")
        sub(("acid-bass", "Acid Bass", C::Bass, V::Aggressive), |p| {
            p.meta.gain = -2.5;
            desc(&mut p.meta, "303-style squelch: velocity = accent");
            p.osc_a = osc(Saw, 1.0);
            filt(&mut p.filter, 300.0, 10.0, 0.5, 3.5, 0.9);
            p.filter_env = adsr(0.002, 0.28, 0.05, 0.12);
            p.amp = amp(0.002, 0.3, 0.8, 0.08, 0.5);
            p.meta.polyphony = 8.0;
            p.meta.character.drive = 0.45;
        }),
        // 21 (was "AM Lead")
        sub(("pulse-lead", "Pulse Lead", C::Lead, V::Punchy), |p| {
            p.meta.polyphony = 6.0;
            p.meta.gain = -10.0;
            p.osc_a = osc(Pulse, 1.0).width(0.25);
            p.osc_b = osc(Pulse, 0.7).width(0.6).fine(8.0);
            filt(&mut p.filter, 2600.0, 2.0, 0.5, 1.2, 0.5);
            p.filter_env = adsr(0.003, 0.3, 0.5, 0.25);
            p.amp = amp(0.004, 0.2, 0.85, 0.25, 0.5);
            p.lfo = lfo(5.8, 10.0, 0.0, 0.35);
            p.meta.character.chorus = 0.2;
        }),
        // 22 (was "Tremolo Lead")
        sub(("theremin", "Theremin", C::Lead, V::Experimental), |p| {
            p.meta.gain = -8.5;
            p.osc_a = osc(Sine, 1.0);
            p.osc_b = osc(Triangle, 0.15).semi(12.0);
            filt(&mut p.filter, 6000.0, 0.3, 0.3, 0.0, 0.0);
            p.amp = amp(0.08, 0.1, 1.0, 0.35, 0.3);
            p.lfo = lfo(5.8, 28.0, 0.0, 0.25);
            p.meta.polyphony = 8.0;
        }),
        // 23 (was "AM Pad")
        sub(("airy-pad", "Airy Pad", C::Pad, V::Dreamy), |p| {
            p.meta.polyphony = 10.0;
            p.meta.gain = 3.5;
            p.osc_a = osc(Triangle, 0.8).unison(3.0, 15.0);
            p.osc_b = osc(Saw, 0.35).semi(12.0).unison(3.0, 20.0);
            p.noise = NoiseParams {
                level: 0.25,
                kind: NoiseType::Pink,
                decay: 0.0,
            };
            filt(&mut p.filter, 3200.0, 1.0, 0.4, 0.5, 0.2);
            p.filter_env = adsr(1.5, 2.0, 0.8, 2.5);
            p.amp = amp(1.2, 1.0, 0.9, 2.8, 0.25);
            p.lfo = lfo(0.12, 0.0, 500.0, 0.0);
            p.meta.character.chorus = 0.55;
        }),
        // 24 (was "Mono Bass")
        sub(("reese-bass", "Reese Bass", C::Bass, V::Dark), |p| {
            p.meta.gain = -1.0;
            p.osc_a = osc(Saw, 1.0).fine(-9.0);
            p.osc_b = osc(Saw, 1.0).fine(9.0);
            p.sub.level = 0.6;
            filt(&mut p.filter, 600.0, 2.0, 0.3, 0.6, 0.4);
            p.filter_env = adsr(0.02, 0.6, 0.6, 0.3);
            p.amp = amp(0.005, 0.3, 1.0, 0.15, 0.4);
            p.lfo = lfo(0.2, 0.0, 250.0, 0.0);
            p.mono = true;
            p.legato = true;
            p.glide = 0.03;
            p.meta.character.drive = 0.3;
        }),
        // 25 (was "Squelch Bass")
        sub(("pluck-bass", "Pluck Bass", C::Bass, V::Aggressive), |p| {
            p.meta.gain = 0.5;
            p.osc_a = osc(Saw, 1.0);
            p.osc_b = osc(Square, 0.5).semi(-12.0);
            filt(&mut p.filter, 220.0, 6.0, 0.4, 3.8, 0.8);
            p.filter_env = adsr(0.001, 0.14, 0.05, 0.1);
            p.amp = amp(0.001, 0.35, 0.25, 0.1, 0.6);
            p.mono = true;
            p.legato = false;
            p.glide = 0.0;
            p.meta.character.drive = 0.25;
        }),
        // 26 (was "Mono Lead")
        sub(("mono-lead", "Mono Lead", C::Lead, V::Aggressive), |p| {
            p.meta.gain = -1.0;
            p.osc_a = osc(Saw, 1.0).unison(3.0, 14.0);
            p.osc_b = osc(Square, 0.5).semi(-12.0);
            filt(&mut p.filter, 1500.0, 4.0, 0.6, 2.8, 0.5);
            p.filter_env = adsr(0.003, 0.4, 0.35, 0.25);
            p.amp = amp(0.003, 0.2, 0.9, 0.2, 0.4);
            p.lfo = lfo(5.6, 14.0, 0.0, 0.35);
            p.mono = true;
            p.legato = true;
            p.glide = 0.04;
            p.meta.character.drive = 0.3;
        }),
        // 27 (was "Portamento Lead")
        sub(("glide-lead", "Glide Lead", C::Lead, V::Chill), |p| {
            p.meta.gain = -8.5;
            p.osc_a = osc(Square, 1.0);
            p.osc_b = osc(Triangle, 0.4).semi(12.0);
            filt(&mut p.filter, 2000.0, 2.0, 0.5, 1.5, 0.4);
            p.filter_env = adsr(0.02, 0.3, 0.5, 0.3);
            p.amp = amp(0.01, 0.2, 0.85, 0.3, 0.4);
            p.lfo = lfo(5.2, 12.0, 0.0, 0.4);
            p.mono = true;
            p.legato = true;
            p.glide = 0.12;
            p.meta.character.chorus = 0.15;
        }),
        // 28 (was "Nylon Guitar")
        pluck(
            ("nylon-guitar", "Nylon Guitar", C::Pluck, V::Organic),
            |p| {
                p.meta.gain = -2.5;
                p.attack_noise = 1.2;
                p.dampening = 3200.0;
                p.resonance = 0.975;
                p.release = 1.0;
                p.velocity = 0.7;
                p.meta.character.presence = -2.0;
            },
        ),
        // 29 (was "Harp")
        pluck(("harp", "Harp", C::Pluck, V::Dreamy), |p| {
            p.meta.gain = -1.5;
            p.attack_noise = 0.6;
            p.dampening = 5200.0;
            p.resonance = 0.985;
            p.release = 2.2;
            p.velocity = 0.5;
            p.meta.character.chorus = 0.15;
        }),
        // 30 (was "Kalimba")
        SynthPreset::Fm(fm(("kalimba", "Kalimba", C::Pluck, V::Organic), |p| {
            fm_core(
                p,
                0.5,
                5.4,
                0.9,
                0.7,
                0.4,
                adsr(0.001, 0.06, 0.0, 0.05),
                amp(0.001, 1.4, 0.0, 0.6, 0.7),
            );
            p.ping = ping(3.1, 0.2, 0.09);
        })),
        // 31 (was "Timpani")
        sub(("timpani", "Timpani", C::Percussion, V::Punchy), |p| {
            p.meta.polyphony = 6.0;
            p.meta.gain = 0.5;
            p.osc_a = osc(Sine, 1.0);
            p.osc_b = osc(Sine, 0.35).semi(7.0).fine(2.0);
            p.sub.level = 0.3;
            p.noise = NoiseParams {
                level: 0.4,
                kind: NoiseType::Brown,
                decay: 0.06,
            };
            filt(&mut p.filter, 1200.0, 0.5, 0.5, 1.2, 0.6);
            p.filter_env = adsr(0.001, 0.4, 0.2, 1.2);
            p.amp = amp(0.002, 1.8, 0.0, 1.0, 0.8);
            p.pitch_env = PitchEnvParams {
                amount: 1.0,
                decay: 0.12,
            };
        }),
        // 32 (was "Steel Drum")
        SynthPreset::Fm(fm(
            ("steel-drum", "Steel Drum", C::Percussion, V::Bright),
            |p| {
                fm_core(
                    p,
                    1.0,
                    1.0,
                    1.5,
                    0.7,
                    0.4,
                    adsr(0.001, 0.25, 0.1, 0.2),
                    amp(0.001, 1.2, 0.0, 0.5, 0.7),
                );
                p.ping = ping(3.0, 0.25, 0.2);
            },
        )),
        // 33 (was "Wobble")
        sub(
            ("wobble-bass", "Wobble Bass", C::Fx, V::Experimental),
            |p| {
                p.meta.gain = -1.0;
                p.osc_a = osc(Saw, 1.0).unison(3.0, 20.0);
                p.osc_b = osc(Square, 0.6).semi(-12.0);
                p.sub.level = 0.5;
                filt(&mut p.filter, 400.0, 5.0, 0.3, 0.0, 0.2);
                p.amp = amp(0.005, 0.2, 1.0, 0.2, 0.4);
                p.lfo = lfo(2.5, 0.0, 3000.0, 0.0);
                p.meta.polyphony = 8.0;
                p.meta.character.drive = 0.4;
            },
        ),
        // 34 (was "Sweep")
        sub(("sweep-pad", "Sweep Pad", C::Fx, V::Dreamy), |p| {
            p.meta.polyphony = 8.0;
            p.meta.gain = -0.5;
            p.osc_a = osc(Saw, 1.0).unison(5.0, 30.0);
            p.osc_b = osc(Saw, 0.4).semi(7.0).unison(3.0, 20.0);
            p.noise = NoiseParams {
                level: 0.2,
                kind: NoiseType::White,
                decay: 0.0,
            };
            filt(&mut p.filter, 300.0, 6.0, 0.3, 5.0, 0.2);
            p.filter_env = adsr(3.5, 3.0, 0.5, 3.0);
            p.amp = amp(1.5, 1.0, 0.9, 3.0, 0.3);
            p.lfo = lfo(0.1, 0.0, 400.0, 0.0);
            p.meta.character.chorus = 0.4;
        }),
        // 35 (was "Noise Hit")
        sub(("laser-zap", "Laser Zap", C::Fx, V::Experimental), |p| {
            p.meta.gain = -5.0;
            p.osc_a = osc(Square, 1.0);
            p.osc_b = osc(Saw, 0.5).semi(7.0);
            p.noise = NoiseParams {
                level: 0.5,
                kind: NoiseType::White,
                decay: 0.05,
            };
            filt(&mut p.filter, 4000.0, 6.0, 0.3, -2.0, 0.4);
            p.filter_env = adsr(0.001, 0.2, 0.0, 0.1);
            p.amp = amp(0.001, 0.25, 0.0, 0.1, 0.6);
            p.pitch_env = PitchEnvParams {
                amount: 24.0,
                decay: 0.18,
            };
        }),
        // ─── New sounds (36+) ───
        SynthPreset::Fm(fm(("wurli", "Wurlitzer", C::Keys, V::Chill), |p| {
            desc(&mut p.meta, "Reedy EP with tremolo; bites when played hard");
            fm_core(
                p,
                6.0,
                1.0,
                2.6,
                0.9,
                0.5,
                adsr(0.001, 0.5, 0.25, 0.2),
                amp(0.002, 2.4, 0.1, 0.25, 0.75),
            );
            p.ping = ping(7.0, 0.1, 0.04);
            ch(&mut p.meta.character, 0.2, 0.0, 0.5);
            p.meta.character.tremolo_rate = 5.5;
        })),
        sub(("rock-organ", "Rock Organ", C::Keys, V::Aggressive), |p| {
            p.meta.polyphony = 16.0;
            p.meta.gain = 5.5;
            p.osc_a = osc(Sine, 1.0).partials(drawbars(1.0, 1.0, 0.9, 0.8, 0.0, 0.6, 0.7));
            p.sub.level = 0.8;
            p.noise = NoiseParams {
                level: 0.3,
                kind: NoiseType::White,
                decay: 0.005,
            };
            filt(&mut p.filter, 10000.0, 0.3, 0.0, 0.0, 0.0);
            p.filter.rolloff = -12.0;
            p.amp = amp(0.003, 0.05, 1.0, 0.05, 0.1);
            p.lfo = lfo(6.8, 6.0, 0.0, 0.0);
            ch(&mut p.meta.character, 0.45, 0.5, 0.3);
            p.meta.character.tremolo_rate = 6.8;
        }),
        sub(("trance-lead", "Supersaw Lead", C::Lead, V::Bright), |p| {
            p.meta.polyphony = 6.0;
            p.meta.gain = -1.5;
            p.osc_a = osc(Saw, 1.0).unison(7.0, 40.0);
            p.osc_b = osc(Saw, 0.4).semi(12.0).unison(5.0, 25.0);
            filt(&mut p.filter, 5000.0, 1.0, 0.4, 0.8, 0.4);
            p.filter_env = adsr(0.005, 0.4, 0.7, 0.3);
            p.amp = amp(0.005, 0.3, 0.9, 0.35, 0.4);
            p.meta.character.chorus = 0.25;
        }),
        sub(
            ("brass-section", "Brass Section", C::Brass, V::Organic),
            |p| {
                p.meta.polyphony = 10.0;
                p.meta.gain = 3.5;
                p.osc_a = osc(Saw, 1.0).unison(3.0, 8.0);
                p.osc_b = osc(Saw, 0.25).semi(12.0);
                p.noise = NoiseParams {
                    level: 0.12,
                    kind: NoiseType::Pink,
                    decay: 0.08,
                };
                filt(&mut p.filter, 600.0, 1.0, 0.6, 2.6, 0.8);
                p.filter_env = adsr(0.09, 0.6, 0.5, 0.25);
                p.amp = amp(0.05, 0.4, 0.85, 0.3, 0.7);
                p.lfo = lfo(5.2, 10.0, 0.0, 0.5);
                p.meta.character.chorus = 0.2;
            },
        ),
        SynthPreset::Fm(fm(("glass-pad", "Glass Pad", C::Pad, V::Dreamy), |p| {
            p.meta.polyphony = 10.0;
            fm_core(
                p,
                -7.0,
                3.0,
                1.0,
                0.4,
                0.4,
                adsr(1.5, 2.0, 0.6, 2.0),
                amp(0.9, 1.0, 0.85, 2.5, 0.3),
            );
            p.lfo = lfo(4.5, 4.0, 0.0, 0.0);
            p.meta.character.chorus = 0.5;
        })),
    ]
}

/// `SYNTH_PRESETS`.
pub fn synth_presets() -> &'static [SynthPreset] {
    static BANK: OnceLock<Vec<SynthPreset>> = OnceLock::new();
    BANK.get_or_init(build_bank)
}

/// `FALLBACK_PIANO`: the synthesized piano a sampler preset plays until its samples decode.
pub fn fallback_piano() -> &'static FmPreset {
    static P: OnceLock<FmPreset> = OnceLock::new();
    P.get_or_init(|| {
        fm(
            ("fallback-piano", "Piano (synth)", C::Keys, V::Organic),
            |p| {
                fm_core(
                    p,
                    -11.0,
                    1.0,
                    1.4,
                    0.9,
                    0.7,
                    adsr(0.001, 0.6, 0.05, 0.3),
                    amp(0.001, 3.0, 0.0, 0.4, 0.8),
                );
                p.ping = ping(4.0, 0.12, 0.03);
                p.detune = 3.0;
            },
        )
    })
}
