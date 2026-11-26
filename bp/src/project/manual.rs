use super::{Aim, BEATS_PER_BAR, BUSES, Bus, DECKS, File, LaneLfo, Layer, PaletteColumn, Project, RACK_ROWS, Return, SEND_COUNT, Slope, TRACKS, Track, VELOCITY};
use crate::arp::{Articulation, MidiRow};
use fx::params::Signal;
use fx::{Kind, Slot};
use sdk::json::Json;
use sdk::schema::{Fields, bounded, either, one_of};

const ABOUT: [&str; 7] = [
    "Project schema; each list opens with its description, then item shapes.",
    "Unit suffixes: Ms, S, Hz, Db, Bpm, Beats, Seconds, Sixteenths, Semitones, Cents, Octaves, Times.",
    "Omitted keys take defaults; unknown keys are ignored; out-of-range numbers clamp.",
    "References are zero-based list indices; omit for none.",
    "Beats are song-relative, four per bar; note startBeat is clip-relative.",
    "Automation uses dotted paths, e.g. filter.cutoffHz; each row lists its lanes.",
    "Projects store racks whole; preset edits never change saved songs.",
];

const LANE_PARAM: &str = "dotted path, e.g. filter.cutoffHz; level/wet for row mix; gain without row for fader";
const SEND_LANE: &str = "whole number 0 to 15: send index instead of row/param, a track's sendLevels/S; values 0-1";

fn lane_row() -> Json {
    Json::from(format!("whole number 0 to {}: rack row index; omit for fader", RACK_ROWS - 1))
}

pub fn document() -> Json {
    let mut fields = Fields::new();
    fields.push("about", Json::Array(ABOUT.iter().map(|line| Json::from(*line)).collect()));
    fields.push("kind", Json::from("project; presets use rack, chords or a row kind"));
    Project::document(&Project::blank(), &mut fields);
    fields.push("tracks", listed(&format!("up to {TRACKS} tracks; members follow their group with inGroup"), track()));
    fields.push("palette", listed(&format!("nondefault palette columns; 7 degrees on each of {DECKS} decks"), column()));
    fields.whole("master", |fields| return_fields(fields, "master effects after tracks/returns sum; no core rows"));
    fields.push("returns", listed(&format!("{SEND_COUNT} returns A-P, fed by sendLevels; {{}} is an empty return"), return_object()));
    fields.into_json()
}

pub fn preset() -> Json {
    let mut fields = Fields::new();
    fields.push("about", Json::from("Standalone preset; projects embed its contents."));
    fields.push("kind", Json::from("rack, chords, or a row kind; single rows may include lfos"));
    fields.push("description", Json::from("text the browser shows under the name"));
    fields.push("author", Json::from("who made it; the browser shows it beside the name"));
    fields.push("website", Json::from("the author's page; the browser shows its address after the description"));
    fields.push("rack", Json::from("the rows, as a track writes them, less sourceTrack"));
    fields.push("lfos", Json::from("LFO lanes by row; a single row uses row 0"));
    fields.push("palette", Json::from("a chords preset's columns, as the project's palette without deck"));
    fields.into_json()
}

fn listed(about: &str, shape: Json) -> Json {
    Json::Array(vec![Json::from(about), shape])
}

fn track() -> Json {
    let blank = Track::blank();
    let mut fields = Fields::new();
    fields.push("name", Json::from("the track's name, any text; left out, MIDI, AUDIO or GROUP by its kind"));
    Track::words_document(&blank, &mut fields);
    fields.manual("kind", one_of(&["midi", "audio", "group"], "midi"), "midi plays notes through Cores; audio plays waves; group sums its members");
    fields.push("follows", Json::from(format!("whole number 0 to {}: also plays the notes leaving that MIDI track's last midi row, its arp's included, through this track's own rows; omit for none", TRACKS - 1)));
    fields.manual("inGroup", either(false), "joins the group above");
    fields.manual("folded", either(false), "a group hides its members");
    Track::mixer_document(&blank, &mut fields);
    fields.push("sendLevels", Json::from(format!("{SEND_COUNT} numbers 0 to 1, default 0: post-fader sends A-P")));
    fields.push("rack", rack(&format!("up to {RACK_ROWS} serial rows; a MIDI track's row 0 is a midi row, fixed, and a core follows; a midi row further down plays the midi row above into the cores below it; audio/group take effects alone")));
    fields.push("clips", listed("clips in any order", clip()));
    fields.push("curves", listed("automation curves, one per lane", curve()));
    fields.push("lfos", listed("one LFO per lane; may coexist with a curve", lfo()));
    fields.into_json()
}

fn rack(about: &str) -> Json {
    let mut rows = vec![Json::from(about), midi_row(), core_row(), bus_row()];
    rows.extend(Kind::ALL.iter().map(|kind| effect_row(*kind)));
    Json::Array(rows)
}

fn bus_row() -> Json {
    let mut fields = Fields::new();
    fields.push("kind", Json::from(Bus::KIND));
    fields.push("levels", Json::from(format!("{BUSES} numbers 0 to 1, default 1: the level of bus 1, the rows above this one, and of bus 2, the rows below; each bus runs its own Cores and effects and the two are summed; one bus row a MIDI track's rack at most")));
    fields.into_json()
}

fn midi_row() -> Json {
    let mut fields = Fields::new();
    MidiRow::manual(&mut fields);
    fields.into_json()
}

fn core_row() -> Json {
    let mut fields = Fields::new();
    fields.push("kind", Json::from("core"));
    Layer::document(&Layer::DEFAULT, &mut fields);
    fields.push("seed", Json::from("whole number 1 or more: which run of random draws its drift and scatter take; left out, the row's place gives one, and the row keeps it when it or its track moves; no two Cores share one"));
    coresynth::manual(&mut fields);
    fields.into_json()
}

fn effect_row(kind: Kind) -> Json {
    let mut fields = Fields::new();
    Slot::manual(kind, &mut fields);
    if kind.takes_signal() {
        let source = if kind == Kind::Gate { "sidechain source track, not the gate's own, which opens the gate instead of its own input, muted or not" } else { "sidechain source track, muted or not" };
        fields.push(Signal::SOURCE_TRACK, Json::from(format!("whole number 0 to {}: {source}; omit for none", TRACKS - 1)));
        fields.manual(Signal::PRE_EFFECTS, either(false), "the signal is taken before that track's effects");
    }
    fields.into_json()
}

fn clip() -> Json {
    let mut fields = Fields::new();
    fields.push("startBeat", Json::from("number 0 or more: song-relative start beat"));
    fields.push("lengthBeats", Json::from(format!("number above 0, default {BEATS_PER_BAR}: clip length")));
    fields.push("name", Json::from("the clip's name, default empty"));
    fields.push("notes", listed("MIDI notes, or a pattern object on write (see guide); not with wave", note()));
    fields.push("chords", Json::from("read only: the notes' key, the song's root and scale when the notes fit it nearly as well, any outside it counted, then chords by clip beat; with the track's arp, transposeSemitones or forceKey on, what it plays over the clip follows the same way; left out when nothing sounds"));
    fields.whole("wave", wave);
    fields.into_json()
}

fn note() -> Json {
    let mut fields = Fields::new();
    fields.push("pitch", Json::from("whole number 0 to 127: MIDI note number, 60 is C4"));
    fields.push("startBeat", Json::from("number 0 or more: beats from the clip's start"));
    fields.push("lengthBeats", Json::from("number above 0: note length"));
    fields.manual("velocity", bounded(f64::from(VELOCITY), 1.0..=127.0, true), "how hard the note is struck");
    fields.manual("cents", bounded(0.0, -f64::from(Articulation::MAX_CENTS)..=f64::from(Articulation::MAX_CENTS), false), "fine tune of the note");
    fields.push("mod", Json::from("number -1 to 1: the note's mod value, which a Core's mod keys turn into oscillator levels, pitch offsets, filter cutoff, pan and pulse width; a Core with mod/sampled true takes it as a share of its mod/sample; left out, 0, or the whole of mod/sample"));
    fields.manual("shift", bounded(0.0, -f64::from(Articulation::MAX_SHIFT)..=f64::from(Articulation::MAX_SHIFT), true), "arp shift in semitones, scaled by each oscillator's stepShare; flatten true at a midi row writes it");
    fields.manual("tie", either(false), "continue the preceding note without retriggering");
    fields.manual("slide", either(false), "as tie, gliding over the Core's mono/glideMs");
    fields.into_json()
}

fn wave(fields: &mut Fields) {
    fields.push("name", Json::from("wave-pool filename; saved projects embed the audio"));
    fields.push("path", Json::from("WAV path imported on write; replaced by name"));
    File::document(&File::new(String::new()), fields);
    let mut span = Fields::new();
    span.push("fromSeconds", Json::from("number 0 or more: span start in the file"));
    span.push("toSeconds", Json::from("number above fromSeconds: span end"));
    span.manual("levelDb", bounded(0.0, -File::DECIBELS..=File::DECIBELS, false), "the span's level");
    fields.push("spans", listed("nonoverlapping file-time spans with independent levels", span.into_json()));
    let mut rise = Fields::new();
    rise.push("fromSeconds", Json::from("number 0 to toSeconds: where in the file the level starts to rise, silent before; omit fadeIn for none"));
    rise.push("toSeconds", Json::from("number above 0: where it reaches full level, full after; never past fadeOut's fromSeconds"));
    rise.manual("bend", bounded(0.0, -1.0..=1.0, false), "bows the rise, lifting the level sooner above 0, holding it down longer below; 1 is steepest");
    fields.push(Slope::Rise.key(), rise.into_json());
    let mut fall = Fields::new();
    fall.push("fromSeconds", Json::from("number 0 or more: where in the file the level starts to fall, full before; never before fadeIn's toSeconds; omit fadeOut for none"));
    fall.push("toSeconds", Json::from("number fromSeconds or more: where it reaches silence, silent after"));
    fall.manual("bend", bounded(0.0, -1.0..=1.0, false), "bows the fall, holding the level longer above 0, dropping sooner below; 1 is steepest");
    fields.push(Slope::Fall.key(), fall.into_json());
}

fn curve() -> Json {
    let mut fields = Fields::new();
    fields.push("row", lane_row());
    fields.push("param", Json::from(LANE_PARAM));
    fields.push("lfoParam", Json::from("one of amount, startPhase, center: moves that LFO setting instead; omit for param"));
    fields.push("send", Json::from(SEND_LANE));
    let mut node = Fields::new();
    node.push("beat", Json::from("number 0 or more: beats from the song's start"));
    node.push("bar", Json::from("number 1 or more in place of beat: bars from the song's start, 16.5 halfway through bar 16"));
    node.push("value", Json::from("parameter value, overriding the written one; LFO amount/startPhase use 0-1"));
    node.push("valueDb", Json::from("decibels in place of value on a gain lane, 0 unity"));
    node.push("travel", Json::from("number 0 to 1: exact control position for value"));
    node.manual("bend", bounded(0.0, -1.0..=1.0, false), "bows the segment to the next node toward the higher value above 0, the lower below; 1 is steepest");
    fields.push("nodes", listed("beat order; holds first/last values outside the nodes", node.into_json()));
    fields.into_json()
}

fn lfo() -> Json {
    let mut fields = Fields::new();
    fields.push("row", lane_row());
    fields.push("param", Json::from(LANE_PARAM));
    fields.push("send", Json::from(SEND_LANE));
    fields.push("amount", Json::from("number 0 to 1: swing, peak to peak, as a share of the key's range around center: pan 0.35 swings ±0.35, a 0–1 key ±0.175; 0 removes the lane; read with fromBeat and untilBeat samples it"));
    fields.push("center", Json::from("swing centre, in the key's units"));
    fields.push("centerTravel", Json::from("number 0 to 1: exact control position for center"));
    LaneLfo::document(&LaneLfo::blank(Aim::Gain), &mut fields);
    fields.into_json()
}

fn column() -> Json {
    let mut fields = Fields::new();
    fields.manual("deck", bounded(0.0, 0.0..=(DECKS - 1) as f64, true), "which deck the column sits on");
    fields.push("degree", Json::from("whole number 1 to 7: scale degree"));
    PaletteColumn::document(&PaletteColumn::DEFAULT, &mut fields);
    fields.into_json()
}

fn return_object() -> Json {
    let mut fields = Fields::new();
    return_fields(&mut fields, "effect rack; no core rows");
    fields.into_json()
}

fn return_fields(fields: &mut Fields, about: &str) {
    Return::document(&Return::DEFAULT, fields);
    fields.push("rack", rack(about));
    fields.push("curves", listed("automation curves on the rack's rows", curve()));
    fields.push("lfos", listed("LFO lanes on the rack's rows", lfo()));
}
