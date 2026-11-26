use super::*;
use sdk::division::{self, Division};
use crate::project::warp::{ENVELOPE_MAX, Loop};
use sdk::face::Format::{Decibels, Degrees, GlideTime, Integer, LfoHertz, MillisWhole, Percent, Semitones, SignedCents, SignedMillis};
use sdk::face::Kind::{Centre, Rise};
use sdk::face::{Param, Cell, UNIT, choose, desc, field, flag, linear, log, pick, switch, tempo, toggle, unless, value};
use sdk::param::{Scale, skewed};
use crate::project::{LfoClock, LfoField, LfoShape};

fn lfo_off(p: &LaneLfo) -> bool {
    p.amount <= 0.0
}

fn lfo_off_or_free(p: &LaneLfo) -> bool {
    lfo_off(p) || !p.sync
}

fn lfo_off_or_synced(p: &LaneLfo) -> bool {
    lfo_off(p) || p.sync
}

fn restart_of(p: &LaneLfo) -> f64 {
    match (p.retrigger, p.legato) {
        (false, _) => 0.0,
        (true, false) => 1.0,
        (true, true) => 2.0,
    }
}

fn set_restart(p: &mut LaneLfo, v: f64) {
    p.retrigger = v >= 0.5;
    p.legato = v >= 1.5;
}

fn lfo_off_or_unpitched(p: &LaneLfo) -> bool {
    lfo_off(p) || p.pitch_knob().is_none()
}

fn lfo_off_or_audio(p: &LaneLfo) -> bool {
    lfo_off(p) || p.audio_rate().is_some()
}

const DIVISIONS: &[&str] = &division::LABELS;
const TIME_SKEW: f64 = 4.0;
const BAND_RATE: Scale = log(LaneLfo::MIN_HZ, LaneLfo::MAX_HZ);
const AUDIO_RATE: Scale = log(LaneLfo::MIN_HZ, LaneLfo::AUDIO_MAX_HZ);

macro_rules! lfo_band {
    ($rate:expr, $rate_desc:literal) => {
        [
            desc(value(Rise, 0, 0, "AMT", Param::new("LFO", LfoField::Amount.label(), field!(amount), UNIT), Percent), "Wave height; it replaces the lane's nodes"),
            desc(unless(switch(0, 1, &["FREE", "SYNC"], "LFO", "SYNC", flag!(sync)), lfo_off), "SYNC runs at a note value, FREE at RATE"),
            desc(unless(pick(0, 2, 2, "LFO", "DIVISION", choose!(&Division::ALL, division), DIVISIONS), lfo_off_or_free), "Note value of one cycle when synced"),
            desc(unless(value(Rise, 1, 0, "RAT", Param::new("LFO", "RATE", field!(rate_hz), $rate), LfoHertz), lfo_off_or_synced), $rate_desc),
            desc(unless(value(Rise, 1, 1, "PHS", Param::new("LFO", LfoField::Phase.label(), field!(phase), UNIT), Degrees), lfo_off), "Where in its cycle the wave starts"),
            desc(unless(switch(1, 2, &["RUN", "RTG", "LEG"], "LFO", "RESTART", field!(restart_of, set_restart)), lfo_off), "RUN runs on; RTG starts over on each new note from the MIDI row above, LEG only when no other note still sounds"),
            desc(unless(value(Centre, 1, 3, "CENT", Param::new("LFO", LfoField::Center.label(), field!(centre), UNIT), Percent), lfo_off), "Level the wave centres on; it clips at the lane's top and bottom"),
            desc(unless(pick(2, 0, 2, "LFO", "SHAPE", choose!(LfoShape::ALL, shape), LfoShape::LABELS), lfo_off), "The wave; Drawn plays the lane's own points, 0 bottom to 1 top"),
            desc(unless(toggle(2, 2, "AUD", "LFO", "AUDIO RATE", field!(|p| if p.audio { 1.0 } else { 0.0 }, |p, v| p.set_audio(v >= 0.5))), lfo_off_or_unpitched), "Bends pitch smoothly up to 2 kHz; off, up to 32 Hz"),
            desc(unless(switch(2, 3, &["SONG", "NOTE"], "LFO", "CLOCK", choose!(LfoClock::ALL, clock)), lfo_off), "SONG follows the song; NOTE starts over on each new note from the MIDI row above, once for a chord, never for a tie or slide"),
            desc(unless(toggle(3, 2, "ONCE", "LFO", "ONCE", flag!(once)), lfo_off), "Plays one cycle per restart and holds: a one-shot envelope"),
            desc(unless(value(Rise, 3, 0, "FADE", Param::new("LFO", "FADE", field!(fade_ms), skewed(0.0, LaneLfo::MAX_FADE_MS, TIME_SKEW)), GlideTime), lfo_off), "Fade-in time after each restart"),
            desc(unless(value(Rise, 3, 1, "LAG", Param::new("LFO", "LAG", field!(lag_ms), skewed(0.0, LaneLfo::MAX_LAG_MS, TIME_SKEW)), GlideTime), lfo_off_or_audio), "Glide time to each new value"),
        ]
    };
}

pub(super) const LFO_BAND: [Cell<LaneLfo>; 13] = lfo_band!(BAND_RATE, "Cycles per second when running free");
const AUDIO_LFO_BAND: [Cell<LaneLfo>; 13] = lfo_band!(AUDIO_RATE, "Cycles per second when running free, to 2 kHz at audio rate");

pub(super) fn lfo_band(lfo: &LaneLfo) -> &'static [Cell<LaneLfo>] {
    if lfo.audio_rate().is_some() { &AUDIO_LFO_BAND } else { &LFO_BAND }
}

const START_MS: f64 = 500.0;

fn unwarped(f: &File) -> bool {
    !f.warp
}

fn unwarped_or_filled(f: &File) -> bool {
    !f.warp || f.gap != Loop::Rest
}

fn adopt_tempo(f: &mut File, bpm: f64) {
    if f.warp && f.tempo <= 0.0 {
        f.tempo = bpm;
    }
}

pub(super) const FILE_BAND: [Cell<File>; 8] = [
    desc(value(Centre, 0, 0, "LVL", Param::new("AUDIO", "LEVEL", field!(level), linear(-File::DECIBELS, File::DECIBELS)), Decibels), "Level of the file"),
    desc(value(Centre, 0, 1, "STRT", Param::new("AUDIO", "START", field!(|f| f.offset * 1000.0, |f, v| f.offset = v / 1000.0), linear(-START_MS, START_MS)), SignedMillis), "Start point in the file; below 0 delays the clip"),
    desc(value(Rise, 0, 2, "END", Param::new("AUDIO", "END", field!(|f| f.tail * 1000.0, |f, v| f.tail = v / 1000.0), linear(0.0, START_MS)), MillisWhole), "Trims the file's tail, fading at the cut"),
    desc(value(Centre, 0, 3, "PTCH", Param::new("AUDIO", "PITCH", field!(|f| f.pitch, |f, v| f.pitch = v.round()), linear(-File::SEMITONES, File::SEMITONES)), Semitones), "Speed in whole semitones; up is faster and shorter"),
    desc(value(Centre, 0, 4, "FIN", Param::new("AUDIO", "FINE", field!(fine), linear(-File::CENTS, File::CENTS)), SignedCents), "Fine pitch of the file, in cents"),
    desc(tempo(toggle(0, 5, "WARP", "AUDIO", "WARP", flag!(warp)), adopt_tempo), "Plays the file at the song's tempo, transient by transient; PTCH then keeps time"),
    desc(unless(switch(0, 6, &Loop::WORDS, "AUDIO", "GAP", choose!(&Loop::ALL, gap)), unwarped), "What fills a gap when the song runs slower: REST silence, FWD the segment's second half again, BAF that half back and forth"),
    desc(unless(value(Rise, 0, 7, "ENV", Param::new("AUDIO", "ENVELOPE", field!(envelope), linear(0.0, ENVELOPE_MAX as f64)), Integer), unwarped_or_filled), "How long each transient holds with GAP at REST; 100 to the next"),
];
