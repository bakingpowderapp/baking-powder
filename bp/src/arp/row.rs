use super::{Arp, RATE_LABELS, RATES, Snap, Style, TOP_VELOCITY, Ties, order};
use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{self, Act, Cell, Face, Faced, Format::*, ICON_NOTES, ICON_PITCH, ICON_STEPS, Kind::*, READOUT_ROWS, View, button, choose, desc, field, flag, held, icon, inert, pick, refuses, switch, tempo, title, toggle, value, wide};
use sdk::json::Json;
use sdk::keyboard::Key;
use sdk::param::{self, Knob, Param, Patch, UNIT, int, linear, log, table};
use sdk::schema::{Fields, MAX_TIMES, params};
use std::sync::OnceLock;

const ARP_MODES: [&str; 3] = ["OFF", "ARP", "STEP"];
const STEPS: [&str; 4] = ["1×", "2×", "3×", "4×"];

params!(MidiRow (fields, read, document) {
    flag bypass = false, "bypass" => "the row's arp, transposeSemitones and forceKey are passed over; row 0 still scales by velocity";
    whole times: usize = 1, "times", 1.0..=MAX_TIMES as f64 => "the row plays as this many copies of itself one under the other, each feeding the next, so its arp, transposeSemitones and forceKey are applied that many times; on row 0 velocity still scales the notes once; every copy counts toward the rack's rows";
    whole velocity: u8 = MidiRow::VELOCITY, "velocity", 1.0..=f64::from(TOP_VELOCITY) => "on row 0, scales the track's notes as they enter, velocity / 127; 127 passes them as struck; a fixedVelocity step ignores it, and so does a midi row further down";
    whole transpose: i8 = 0, "transposeSemitones", -f64::from(MidiRow::TRANSPOSE)..=f64::from(MidiRow::TRANSPOSE) => "moves every note entering the row up or down, before its arp and forceKey; a note moved past 0 or 127 is left out";
    flag force_key = false, "forceKey" => "moves each note the row sends on to the nearest note of the song's root and scale, after the arp; a note in the key passes unchanged";
    choice ties: Ties = Ties::Up, "ties" => "under forceKey, where a note goes when the key notes above and below are equally near, as every note outside a seven-note scale is: up, down, alternate, which takes turns from up at each such note and starts over whenever playback starts or loops, or smart, which keeps the note's interval above the lowest held key as the key spells it, or above the song's root for that key itself";
    own arp: Arp = Arp::DEFAULT, "arp", |p, d, key, f| f.nested(key, |fields| p.arp.fields(&d.arp, fields)), |json, d, key| Arp::read(json.and_then(|json| json.get(key)), d.arp), |d, key, _, f| f.whole(key, |fields| Arp::document(&d.arp, fields));
});

impl MidiRow {
    pub const VELOCITY: u8 = 100;
    pub const TRANSPOSE: i8 = 24;
    pub const KIND: &'static str = "midi";
    pub const LABEL: &'static str = "MIDI";

    pub fn struck(&self, velocity: u8) -> u8 {
        let scaled = (u16::from(velocity) * u16::from(self.velocity) * 2 + u16::from(TOP_VELOCITY)) / (u16::from(TOP_VELOCITY) * 2);
        scaled.clamp(1, u16::from(TOP_VELOCITY)) as u8
    }

    pub fn as_written(self) -> MidiRow {
        MidiRow { velocity: TOP_VELOCITY, transpose: 0, ..self }
    }

    pub fn shift(&self) -> i8 {
        if self.bypass { 0 } else { self.transpose }
    }

    pub fn leaving(&self) -> bool {
        self.times == 0
    }

    pub fn heard(&self) -> Arp {
        Arp { on: self.arp.on && !self.bypass, ..self.arp }
    }

    pub fn snap(&self, key: u16) -> Option<Snap> {
        (self.force_key && !self.bypass).then_some(Snap { key, ties: self.ties })
    }

    pub fn to_json(&self) -> Json {
        let mut fields = Fields::new();
        fields.push("kind", Json::from(MidiRow::KIND));
        self.fields(&MidiRow::DEFAULT, &mut fields);
        fields.into_json()
    }

    pub fn from_json(json: &Json) -> MidiRow {
        MidiRow::read(Some(json), MidiRow::DEFAULT)
    }

    pub fn manual(fields: &mut Fields) {
        fields.push("kind", Json::from(MidiRow::KIND));
        MidiRow::document(&MidiRow::DEFAULT, fields);
        fields.push("lanes", param::lanes(MidiRow::knobs()));
    }

    pub fn knobs() -> &'static [Knob] {
        static KNOBS: OnceLock<Vec<Knob>> = OnceLock::new();
        KNOBS.get_or_init(|| param::knobs(&listed(), MidiRow::DEFAULT, spelled))
    }

    pub fn automatable(band: &str, name: &str) -> bool {
        param_of(band, name).is_some()
    }

    pub fn lane_share(band: &str, name: &str) -> f64 {
        param_of(band, name).map_or(1.0, Param::lane_share)
    }

    pub fn lane_seed(band: &str, name: &str) -> Option<u64> {
        param_of(band, name).map(|param| param.seed)
    }

    pub fn travel_of(&self, band: &str, name: &str) -> Option<f64> {
        param_of(band, name).map(|param| param.travel_of(self))
    }

    pub fn set_travel(&mut self, band: &str, name: &str, travel: f64) -> bool {
        match param_of(band, name) {
            Some(param) => {
                param.set_travel(self, travel);
                true
            }
            None => false,
        }
    }

    pub fn draw(&self, face: &Face, list: &mut DrawList, idle: impl FnOnce(&mut DrawList, Rect)) {
        face.draw(list, READOUT_ROWS, &CELLS, self, idle);
    }
}

impl Patch for MidiRow {
    fn factory(&self) -> MidiRow {
        MidiRow::DEFAULT
    }
}

fn spelled(row: &MidiRow, against: &MidiRow) -> Json {
    let mut fields = Fields::new();
    row.fields(against, &mut fields);
    fields.into_json()
}

fn listed() -> Vec<Param<MidiRow>> {
    PARAMS.iter().copied().chain(face::switches(&CELLS)).collect()
}

fn param_of(band: &str, name: &str) -> Option<&'static Param<MidiRow>> {
    param::find(PARAMS, band, name).or_else(|| face::switched(&CELLS, band, name).map(|cell| &cell.param))
}

table! {
    PARAMS: MidiRow {
        TIME = "ARP", "TIME", field!(arp.time_ms), log(Arp::MIN_TIME, Arp::MAX_TIME);
        GATE = "ARP", "GATE", field!(arp.gate), linear(Arp::MIN_GATE, Arp::MAX_GATE);
        SWING = "ARP", "SWING", field!(arp.swing), UNIT;
        DISTANCE = "ARP", "DISTANCE", field!(|m| m.arp.distance(), |m, v| m.arp.set_distance(v)), linear(-Arp::MAX_DISTANCE, Arp::MAX_DISTANCE);
        TRANSPOSE = "MIDI", "TRANSPOSE", int!(transpose), linear(-(MidiRow::TRANSPOSE as f64), MidiRow::TRANSPOSE as f64);
    }
}

const VEL: Param<MidiRow> = Param::new("MIDI", "VELOCITY", int!(velocity), linear(1.0, TOP_VELOCITY as f64));

fn arp_off(m: &MidiRow) -> bool {
    !m.arp.on
}

fn arp_free(m: &MidiRow) -> bool {
    !m.arp.sync
}

fn arp_synced(m: &MidiRow) -> bool {
    m.arp.sync
}

fn arp_off_or_free(m: &MidiRow) -> bool {
    arp_off(m) || arp_free(m)
}

fn arp_off_or_synced(m: &MidiRow) -> bool {
    arp_off(m) || arp_synced(m)
}

fn arp_off_or_stepped(m: &MidiRow) -> bool {
    arp_off(m) || m.arp.stepped
}

fn arp_off_or_unstepped(m: &MidiRow) -> bool {
    arp_off(m) || !m.arp.stepped
}

fn unforced(m: &MidiRow) -> bool {
    !m.force_key
}

fn arp_mode(m: &MidiRow) -> f64 {
    match (m.arp.on, m.arp.stepped) {
        (false, _) => 0.0,
        (true, false) => 1.0,
        (true, true) => 2.0,
    }
}

fn set_arp_mode(m: &mut MidiRow, v: f64) {
    m.arp.on = v >= 0.5;
    if m.arp.on {
        m.arp.stepped = v >= 1.5;
    }
}

fn arp_retrigger(m: &MidiRow) -> f64 {
    match (m.arp.retrigger, m.arp.retrigger_each_note) {
        (false, _) => 0.0,
        (true, false) => 1.0,
        (true, true) => 2.0,
    }
}

fn set_arp_retrigger(m: &mut MidiRow, v: f64) {
    m.arp.retrigger = v >= 0.5;
    m.arp.retrigger_each_note = v >= 1.5;
}

fn arp_steps(m: &MidiRow) -> f64 {
    (m.arp.steps.max(1) - 1) as f64
}

fn set_arp_steps(m: &mut MidiRow, v: f64) {
    m.arp.steps = (v.round().max(0.0) as usize + 1).min(Arp::MAX_STEPS);
}

fn arp_tempo(m: &mut MidiRow, bpm: f64) {
    m.arp.tempo(bpm);
}

fn draw_style_icon(list: &mut DrawList, index: usize, x: i32, y: i32, ink: Tone) {
    let style = Style::ALL[index.min(Style::ALL.len() - 1)];
    for step in 0..ICON_STEPS {
        let note = order(style, i64::from(step), ICON_NOTES as usize) as i32;
        list.fill(Rect::new(x + step * ICON_PITCH, y + (ICON_NOTES - 1 - note) * ICON_PITCH, 1, 1), ink);
    }
}

static CELLS: [Cell<MidiRow>; 27] = [
    title(0, 0, "ARP"),
    desc(wide(switch(1, 0, &ARP_MODES, "ARP", "MODE", field!(arp_mode, set_arp_mode)), 2), "ARP plays the notes you hold as a pattern; STEP plays its own line of notes from the key you hold"),
    desc(icon(inert(pick(1, 2, 4, "ARP", "STYLE", choose!(Style::ALL, arp.style), Style::LABELS), arp_off_or_stepped), draw_style_icon), "Order the held notes play in"),
    title(0, 7, "TIMING"),
    desc(tempo(inert(wide(switch(1, 7, &["FREE", "SYNC"], "ARP", "SYNC", flag!(arp.sync)), 2), arp_off), arp_tempo), "SYNC steps at a note value, FREE at TIME"),
    desc(inert(refuses(pick(1, 9, 2, "ARP", "RATE", choose!(RATES, arp.rate), RATE_LABELS), arp_free), arp_off_or_free), "How fast the steps fire, as a note value"),
    desc(inert(refuses(wide(value(Rise, 1, 9, "TIME", TIME, Millis), 2), arp_synced), arp_off_or_synced), "How fast the steps fire, in milliseconds"),
    desc(inert(wide(value(Rise, 1, 11, "SWING", SWING, Percent), 2), arp_off), "Delays every second step of the line; at 100% a step and a half then a half, 67% the triplet feel"),
    desc(inert(wide(switch(1, 13, &["RUN", "RTG", "KEY"], "ARP", "RETRIGGER", field!(arp_retrigger, set_arp_retrigger)), 3), arp_off), "RUN plays on, RTG restarts on a fresh chord, KEY restarts on every key press"),
    title(2, 0, "NOTES"),
    desc(held(wide(value(Rise, 3, 0, "VELOCITY", VEL, Integer), 3)), "How hard the track's notes strike this row; 127 passes them as struck"),
    desc(inert(wide(value(Rise, 3, 3, "GATE", GATE, Percent), 3), arp_off_or_stepped), "How much of the step each note sounds; STEP mode sets it per step"),
    title(2, 7, "STACK"),
    desc(inert(pick(3, 7, 2, "ARP", "STEPS", field!(arp_steps, set_arp_steps), &STEPS), arp_off), "Copies of the chord, or of each step, stacked by DISTANCE"),
    desc(inert(wide(value(Centre, 3, 9, "DISTANCE", DISTANCE, Semitones), 2), arp_off), "Pitch gap between stacked copies, in semitones; a preset can hold each copy's own"),
    title(2, 12, "STEP"),
    desc(inert(wide(toggle(3, 12, "CHORD", "ARP", "STEP CHORD", flag!(arp.step_chord)), 2), arp_off_or_unstepped), "Every key you hold plays the STEP line together, each step sounding them all; off, the line plays from the last key pressed alone"),
    desc(inert(wide(toggle(3, 14, "FINISH", "ARP", "FINISH GATE", flag!(arp.finish_gate)), 2), arp_off_or_unstepped), "Lifting the last key lets the STEP line's sounding notes play out their step's gate before the line stops; off, they stop at once"),
    title(4, 0, "KEY"),
    desc(wide(toggle(5, 0, "FORCE", "KEY", "FORCE", flag!(force_key)), 3), "Moves each note this row sends to the nearest note in the song's key; notes in the key pass unchanged"),
    desc(inert(wide(switch(5, 3, Ties::LABELS, "KEY", "TIES", choose!(Ties::ALL, ties)), 3), unforced), "A note halfway between two key notes goes UP or DOWN; ALT takes turns from up; SMART keeps its interval above the lowest held key"),
    title(4, 7, "PRESET"),
    desc(held(wide(toggle(5, 7, "LOCK", "ARP", "LOCK", flag!(arp.lock)), 2)), "Keeps this arp and its pattern when a preset lands"),
    title(4, 12, "CLIP"),
    desc(wide(button(5, 12, "FLATTEN", "ARP", "FLATTEN", Act::Flatten), 2), "Prints the notes this row sends into the clip and turns the arp off, VELOCITY to 127 and TRANSPOSE to 0, so nothing heard changes"),
    title(6, 0, "PITCH"),
    desc(wide(value(Centre, 7, 0, "TRANSPOSE", TRANSPOSE, Semitones), 3), "Moves every note entering this row up or down, in semitones"),
];

impl Faced for MidiRow {
    fn readout_rows(&self) -> i32 {
        READOUT_ROWS
    }

    fn hover(&self, face: &mut Face, point: Option<(i32, i32)>) -> bool {
        face.hover(point, READOUT_ROWS, &CELLS)
    }

    fn down(&mut self, face: &mut Face, x: i32, y: i32, bpm: f64) -> bool {
        face.down(x, y, READOUT_ROWS, &CELLS, self, bpm)
    }

    fn moved(&mut self, face: &mut Face, x: i32, y: i32) -> bool {
        face.moved(x, y, READOUT_ROWS, &CELLS, self)
    }

    fn wheel(&mut self, face: &mut Face, x: i32, y: i32, dy: i32) -> bool {
        face.wheel(x, y, dy, READOUT_ROWS, &CELLS, self)
    }

    fn up(&mut self, face: &mut Face, x: i32, y: i32) -> bool {
        face.up(x, y, READOUT_ROWS, &CELLS, self)
    }

    fn begin(&self, face: &mut Face, first: Option<char>) -> bool {
        face.begin(&CELLS, self, first)
    }

    fn key(&mut self, face: &mut Face, key: Key, command: bool, bpm: f64) -> bool {
        face.key(key, command, &CELLS, self, bpm)
    }

    fn nudge(&mut self, face: &mut Face, direction: i32, coarse: bool, bpm: f64) -> bool {
        face.nudge(&CELLS, self, direction, coarse, bpm)
    }

    fn index_at(&self, x: i32, y: i32, view: View) -> Option<usize> {
        face::index_at(&CELLS, self, READOUT_ROWS, x, y, view)
    }

    fn setting_of(&self, cell: usize) -> Option<(&'static str, &'static str, f64)> {
        face::setting_at(&CELLS, self, cell)
    }

    fn assign(&mut self, band: &str, param: &str, value: f64) -> Option<usize> {
        face::assign_named(&CELLS, self, band, param, value)
    }

    fn restore(&mut self, cell: usize) -> Option<(&'static str, &'static str)> {
        face::restore_at(&CELLS, self, cell)
    }

    fn armable(&self, cell: usize) -> Option<(&'static str, &'static str)> {
        face::armable_cell(&CELLS, cell)
    }

    fn armable_at(&self, x: i32, y: i32, view: View) -> Option<(&'static str, &'static str)> {
        face::armable_at(&CELLS, self, READOUT_ROWS, x, y, view)
    }

    fn automatable_of(&self, band: &str, param: &str) -> Option<(usize, &'static str, &'static str)> {
        face::automatable_of(&CELLS, band, param)
    }

    fn spelling(&self, band: &str, param: &str, travel: f64) -> Option<(String, &'static str)> {
        face::spelling(&CELLS, band, param, travel)
    }

    fn automated_rect(&self, band: &str, param: &str, view: View) -> Option<Rect> {
        face::automated_rect(&CELLS, READOUT_ROWS, band, param, view)
    }
}
