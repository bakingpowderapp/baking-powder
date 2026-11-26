use super::*;
use crate::arp::{Articulation, Join, MidiRow};
use coresynth::{Params, Target};
use sdk::division::Division;
use super::warp::{ENVELOPE_MAX, Loop};
use sdk::face::Drawn;
use fx::Slot;
use sdk::json::{Json, single, tidy};
use sdk::param::Knob;
use sdk::schema::{Fields, bounded, index, items, name, one_of, params};
use crate::theory::{ChordVariation, degree_index};

params!(pub(super) impl Layer (fields, read, document) {
    number level, "level", 0.0..=1.0 => "the row's level into the rack";
    whole times, "times", 1..=Row::MAX_TIMES => "this many copies of the Core sound together, each with its own run of random draws for drift and scatter and the same settings and lanes; every copy counts toward the rack's rows";
});

impl Layer {
    fn to_json(self) -> Json {
        let mut fields = Fields::new();
        fields.push("kind", Json::String(String::from("core")));
        self.fields(&Layer::DEFAULT, &mut fields);
        self.patch.fields(&Params::DEFAULT, &mut fields);
        fields.into_json()
    }

    fn from_json(json: &Json) -> Layer {
        let mut layer = Layer { patch: Params::from_json(Some(json), Params::DEFAULT), ..Layer::DEFAULT };
        layer.read(Some(json));
        layer.seed = json.get("seed").and_then(Json::as_f64).filter(|seed| seed.fract() == 0.0 && (1.0..=MOST_SEED).contains(seed)).map_or(0, |seed| seed as u64);
        layer
    }
}

impl Bus {
    fn to_json(self) -> Json {
        let mut fields = Fields::new();
        fields.push("kind", Json::String(String::from(Bus::KIND)));
        fields.push("levels", Json::Array(self.levels.iter().map(|level| Json::Number(*level)).collect()));
        fields.into_json()
    }

    fn from_json(json: &Json) -> Bus {
        let mut bus = Bus::DEFAULT;
        for (level, item) in bus.levels.iter_mut().zip(items(Some(json), "levels")) {
            *level = item.as_f64().unwrap_or(*level).clamp(0.0, 1.0);
        }
        bus
    }
}

impl Row {
    pub fn to_json(&self) -> Json {
        match self {
            Row::Midi(midi) => midi.to_json(),
            Row::Core(layer) => layer.to_json(),
            Row::Effect(slot) => slot.to_json(),
            Row::Bus(bus) => bus.to_json(),
        }
    }

    pub fn from_json(json: &Json) -> Option<Row> {
        match json.get("kind")?.as_str()? {
            MidiRow::KIND => Some(Row::Midi(MidiRow::from_json(json))),
            "core" => Some(Row::Core(Layer::from_json(json))),
            Bus::KIND => Some(Row::Bus(Bus::from_json(json))),
            _ => Slot::from_json(json).map(Row::Effect),
        }
    }
}

params!(pub(super) impl Return (fields, read, document) {
    single gain, "gain", 0.0..=GAIN_CEILING => "post-rack fader; 1 unity";
});

impl Return {
    fn to_json(&self) -> Json {
        let mut fields = Fields::new();
        self.fields(&Return::DEFAULT, &mut fields);
        if !self.rack.is_empty() {
            fields.push("rack", rows_to_json(&self.rack));
        }
        let rows = |row: usize| self.rack.get(row).copied();
        automation_to_json(&mut fields, &self.automation, &rows);
        fields.into_json()
    }

    fn from_json(json: &Json) -> Return {
        let (rack, landed) = settled_rows_from_json(Cores::Refused, json.get("rack"));
        let rows = |row: usize| landing(&landed, &rack, row);
        let automation = Automation { curves: curves_from_json(json.get("curves"), &rows), lfos: lfos_from_json(json.get("lfos"), &rows) };
        let mut ret = Return { rack, automation, ..Return::DEFAULT };
        ret.read(Some(json));
        ret
    }
}

const MOST_SEED: f64 = 9_007_199_254_740_991.0;

fn seeded_rows_to_json(track: usize, rows: &[Row]) -> Json {
    let mut spelled = rows_to_json(rows);
    if let Json::Array(entries) = &mut spelled {
        for (row, (entry, json)) in rows.iter().zip(entries).enumerate() {
            if let (Row::Core(layer), Json::Object(fields)) = (entry, json)
                && layer.seed != 0
                && layer.seed != (track * RACK_ROWS + row) as u64
            {
                fields.push((String::from("seed"), Json::Number(layer.seed as f64)));
            }
        }
    }
    spelled
}

fn rows_to_json(rows: &[Row]) -> Json {
    Json::Array(rows.iter().map(Row::to_json).collect())
}

fn kinds(track: &Json) -> (bool, bool) {
    let kind = track.get("kind").and_then(Json::as_str).unwrap_or("midi");
    (kind == "audio", kind == "group")
}

fn settled_rows_from_json(cores: Cores, json: Option<&Json>) -> (Vec<Row>, Vec<Option<usize>>) {
    let (rows, landed) = rows_from_json(json);
    let settled = landed.into_iter().map(|at| settled_index(cores, &rows, at?)).collect();
    (settled_rack(cores, rows), settled)
}

fn rows_from_json(json: Option<&Json>) -> (Vec<Row>, Vec<Option<usize>>) {
    let entries = json.and_then(Json::as_array).unwrap_or(&[]);
    let parsed: Vec<Option<Row>> = entries.iter().map(Row::from_json).collect();
    let landed = parsed.iter().scan(0, |kept, row| {
        let at = row.is_some().then_some(*kept);
        *kept += usize::from(row.is_some());
        Some(at)
    });
    let landed = landed.collect();
    (parsed.into_iter().flatten().collect(), landed)
}

fn landing(landed: &[Option<usize>], rack: &[Row], row: usize) -> Option<(usize, Row)> {
    let at = landed.get(row).copied().flatten()?;
    Some((at, *rack.get(at)?))
}

params!(pub(super) impl PaletteColumn (fields, read, document) {
    choice selected: ChordVariation, "variation" => "the chord built on the degree";
    flag parallel, "parallel" => "borrows from the parallel scale";
    flag sharp3, "sharp3" => "raises the third if not major";
    flag flat7, "flat7" => "lowers a major seventh";
    whole bass_offset, "inversion", 0..=2 => "which chord tone is the bass";
    whole octave, "octaveShift", -PaletteColumn::OCTAVE_REACH..=PaletteColumn::OCTAVE_REACH => "octaves the whole chord moves";
    whole bottom, "liftLowest", 0..=i32::MAX, "whole number 0 or more" => "lowest notes moved by liftOctaves";
    whole top, "liftHighest", 0..=i32::MAX, "whole number 0 or more" => "highest notes moved by liftOctaves";
    whole lift, "liftOctaves", -PaletteColumn::OCTAVE_REACH..=PaletteColumn::OCTAVE_REACH => "octaves the lifted notes move";
});

impl PaletteColumn {
    fn from_json(json: &Json) -> PaletteColumn {
        let mut column = PaletteColumn::DEFAULT;
        column.read(Some(json));
        column
    }
}

fn column_to_json(column: PaletteColumn, deck: usize, degree: i32) -> Json {
    let mut fields = Fields::new();
    fields.number("deck", deck as f64, 0.0);
    fields.push("degree", Json::Number(f64::from(degree)));
    column.fields(&PaletteColumn::DEFAULT, &mut fields);
    fields.into_json()
}

fn columns_to_json(columns: &[PaletteColumn; 7]) -> Vec<Json> {
    let spelled = columns.iter().enumerate().filter(|(_, column)| **column != PaletteColumn::DEFAULT);
    spelled.map(|(at, column)| column_to_json(*column, 0, at as i32 + 1)).collect()
}

fn columns_from_json(json: Option<&Json>) -> [PaletteColumn; 7] {
    let mut columns = [PaletteColumn::DEFAULT; 7];
    for entry in json.and_then(Json::as_array).unwrap_or(&[]) {
        let degree = entry.get("degree").and_then(Json::as_i32).unwrap_or(0);
        if (1..=7).contains(&degree) {
            columns[degree_index(degree)] = PaletteColumn::from_json(entry);
        }
    }
    columns
}

impl Preset {
    pub fn to_json(&self) -> Json {
        match self {
            Preset::Row(row, lfos) => {
                let mut json = row.renumbered(|_| None).to_json();
                let spelled: Vec<Json> = lfos.iter().filter_map(|lfo| lfo_to_json(lfo, &|at| (at == 0).then_some(*row))).collect();
                if let Json::Object(fields) = &mut json
                    && !spelled.is_empty()
                {
                    fields.push((String::from("lfos"), Json::Array(spelled)));
                }
                json
            }
            Preset::Rack(sound) => {
                let mut fields = Fields::new();
                fields.push("kind", Json::String(String::from("rack")));
                fields.push("rack", rows_to_json(&sound.rack.iter().map(|row| row.renumbered(|_| None)).collect::<Vec<Row>>()));
                let rows = |at: usize| sound.rack.get(at).copied();
                fields.list("lfos", sound.lfos.iter().filter_map(|lfo| lfo_to_json(lfo, &rows)).collect());
                fields.into_json()
            }
            Preset::Chords(columns) => {
                let mut fields = Fields::new();
                fields.push("kind", Json::String(String::from("chords")));
                fields.list("palette", columns_to_json(columns));
                fields.into_json()
            }
        }
    }

    pub fn from_json(json: &Json) -> Option<Preset> {
        match json.get("kind")?.as_str()? {
            "rack" => {
                let (rack, landed) = rows_from_json(json.get("rack"));
                let lfos = rowed_lfos(json.get("lfos"), &|at| landing(&landed, &rack, at));
                Some(Preset::Rack(Sound { rack, lfos }))
            }
            "chords" => Some(Preset::Chords(columns_from_json(json.get("palette")))),
            _ => Row::from_json(json).map(|row| Preset::Row(row, rowed_lfos(json.get("lfos"), &|at| (at == 0).then_some((0, row))))),
        }
    }
}

impl Document {
    pub fn from_json(json: &Json) -> Option<Document> {
        match json.get("kind")?.as_str()? {
            "project" => Project::from_json(json).map(Document::Project),
            _ => Preset::from_json(json).map(Document::Preset),
        }
    }
}

params!(pub(super) impl Project (fields, read, document) {
    lead bpm, "bpm", LEAST_BPM..=MOST_BPM => "tempo";
    pick root: Root, "root" => "the palette's key";
    pick scale: ScaleName, "scale" => "the palette's scale";
    text brief, "brief" => "user's song brief; follow it, edit only when asked";
    flag metronome, "metronome" => "the click plays with the transport";
    flag lock, "automationLock" => "lanes refuse edits and stay put when clips move";
});

params!(pub(super) impl Track (words, read_words, words_document) {
    text brief, "brief" => "user's track brief; same rules, under the song brief";
    text sound, "sound" => "the preset the sound came from; set when one lands";
    text sound_desc, "soundDescription" => "that preset's description";
});

params!(pub(super) impl Track (mixer, read_mixer, mixer_document) {
    flag solo, "solo" => "only soloed tracks are heard; mute wins; sidechain signals still hear the rest";
    flag mute, "mute" => "the track is silent; a sidechain signal naming it still hears it";
    single gain, "gain", 0.0..=GAIN_CEILING => "post-rack fader; 1 unity";
    single delay_ms, "delayMs", -DELAY_MS..=DELAY_MS => "positive delays, negative advances; sends, group feed and sidechain signals move too";
});

impl Project {
    pub fn to_json(&self) -> Json {
        let blank = Track::blank();
        let tracks = self.tracks.iter().enumerate().map(|(index, track)| {
            let mut fields = Fields::new();
            fields.push("name", Json::String(track.name.clone()));
            track.words(&blank, &mut fields);
            fields.push("kind", Json::String(track.kind().to_ascii_lowercase()));
            if let Some(source) = track.follows {
                fields.push("follows", Json::Number(source as f64));
            }
            fields.flag("inGroup", track.inside, false);
            fields.flag("folded", track.folded, false);
            track.mixer(&blank, &mut fields);
            fields.push("sendLevels", gains_to_json(&track.sends));
            fields.push("rack", seeded_rows_to_json(index, &track.rack));
            fields.list("clips", track.clips.iter().map(clip_to_json).collect());
            let rows = |row: usize| track.rack.get(row).copied();
            automation_to_json(&mut fields, &track.automation, &rows);
            fields.into_json()
        });
        let palette = (0..DECKS).flat_map(|deck| (1..=7).map(move |degree| (deck, degree))).filter_map(|(deck, degree)| {
            let column = self.column(deck, degree);
            (column != PaletteColumn::DEFAULT).then(|| column_to_json(column, deck, degree))
        });
        let mut fields = Fields::new();
        fields.push("kind", Json::String(String::from("project")));
        self.fields(&Project::blank(), &mut fields);
        fields.push("tracks", Json::Array(tracks.collect()));
        fields.list("palette", palette.collect());
        if self.master != Return::DEFAULT {
            fields.push("master", self.master.to_json());
        }
        fields.push("returns", Json::Array(self.sends.iter().map(Return::to_json).collect()));
        fields.into_json()
    }

    pub fn from_json(json: &Json) -> Option<Project> {
        let flag = |value: &Json, key: &str| value.get(key).and_then(Json::as_bool).unwrap_or(false);
        let int = |value: &Json, key: &str| value.get(key).and_then(Json::as_i32).unwrap_or(0);
        let mut project = Project::blank();
        project.read(Some(json));
        project.master = json.get("master").map_or(Return::DEFAULT, Return::from_json);
        for (send, value) in project.sends.iter_mut().zip(json.get("returns").and_then(Json::as_array).unwrap_or(&[])) {
            *send = Return::from_json(value);
        }
        for track in json.get("tracks").and_then(Json::as_array).unwrap_or(&[]).iter().take(TRACKS) {
            let (audio, group) = kinds(track);
            let refused = audio || group;
            let cores = if refused { Cores::Refused } else { Cores::Required };
            let (rack, landed) = settled_rows_from_json(cores, track.get("rack"));
            let mut built = Track {
                name: String::from(track.get("name").and_then(Json::as_str).unwrap_or_else(|| Track { audio, group, ..Track::blank() }.kind())),
                sends: gains_from_json(track.get("sendLevels"), 0.0, 1.0),
                rack,
                clips: track.get("clips").and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(clip_from_json).collect(),
                audio,
                group,
                inside: !group && flag(track, "inGroup"),
                folded: group && flag(track, "folded"),
                follows: track.get("follows").and_then(|value| value.as_index(TRACKS)).filter(|_| !refused),
                ..Track::blank()
            };
            built.clips.sort_by_key(|clip| clip.start);
            built.read_words(Some(track));
            built.read_mixer(Some(track));
            let rows = |row: usize| landing(&landed, &built.rack, row);
            let curves = curves_from_json(track.get("curves"), &rows);
            let lfos = lfos_from_json(track.get("lfos"), &rows);
            built.automation = Automation { curves, lfos };
            project.tracks.push(built);
        }
        if project.tracks.is_empty() {
            project.add_midi_track(0);
        }
        let count = project.tracks.len();
        project.renumber(|source| (source < count).then_some(source));
        project.unloop_gates();
        for entry in json.get("palette").and_then(Json::as_array).unwrap_or(&[]) {
            let degree = int(entry, "degree");
            let Some(deck) = usize::try_from(int(entry, "deck")).ok().filter(|deck| *deck < DECKS) else {
                continue;
            };
            if (1..=7).contains(&degree) {
                *project.column_mut(deck, degree) = PaletteColumn::from_json(entry);
            }
        }
        project.next_number = project.tracks.iter().filter_map(|track| track.name.strip_prefix(track.kind())?.strip_prefix(' ')?.parse::<u32>().ok()).max().unwrap_or(0).saturating_add(1);
        project.seed();
        project.fit_times();
        Some(project)
    }

    pub fn crowded(json: &Json) -> Option<(Host, usize)> {
        let over = |cores: Cores, owner: &Json| wanted(cores, &rows_from_json(owner.get("rack")).0).saturating_sub(RACK_ROWS);
        let listed = |key: &str| json.get(key).and_then(Json::as_array).unwrap_or(&[]).iter().enumerate();
        let tracks = listed("tracks").take(TRACKS).map(|(track, owner)| {
            let (audio, group) = kinds(owner);
            (Host::Track(track), over(if audio || group { Cores::Refused } else { Cores::Required }, owner))
        });
        let sends = listed("returns").take(SEND_COUNT).map(|(send, owner)| (Host::Send(send), over(Cores::Refused, owner)));
        let master = json.get("master").map(|owner| (Host::Master, over(Cores::Refused, owner)));
        tracks.chain(sends).chain(master).find(|(_, over)| *over > 0)
    }
}

const GAIN_CEILING: f32 = 2.0;
const LONGEST_SECONDS: f64 = crate::wav::LONGEST_SECONDS as f64;

fn number(value: f32) -> Json {
    Json::Number(single(value))
}

fn gains_to_json(gains: &[f32]) -> Json {
    Json::Array(gains.iter().map(|gain| number(*gain)).collect())
}

fn gains_from_json<const N: usize>(json: Option<&Json>, default: f32, ceiling: f32) -> [f32; N] {
    let mut gains = [default; N];
    for (gain, value) in gains.iter_mut().zip(json.and_then(Json::as_array).unwrap_or(&[])) {
        if let Some(number) = value.as_f64() {
            *gain = (number as f32).clamp(0.0, ceiling);
        }
    }
    gains
}

type Rows<'a> = &'a dyn Fn(usize) -> Option<Row>;
type Landing<'a> = &'a dyn Fn(usize) -> Option<(usize, Row)>;

pub(crate) const TRACK_GAIN: &str = "gain";
const ROUNDER: f64 = 1e9;

#[derive(Clone, Copy)]
pub(crate) enum Units {
    Gain,
    Share,
    Knob(&'static Knob),
}

impl Units {
    pub(crate) fn value(self, travel: f64) -> f64 {
        match self {
            Units::Gain => fader_gain(travel),
            Units::Share => travel,
            Units::Knob(knob) => knob.value(travel),
        }
    }

    fn travel(self, value: f64) -> f64 {
        let travel = match self {
            Units::Gain => fader_position(value),
            Units::Share => value,
            Units::Knob(knob) => knob.travel(value),
        };
        travel.clamp(0.0, 1.0)
    }

    fn settled(self, value: f64) -> f64 {
        let travel = self.travel(value);
        let rounder = (travel * ROUNDER).round() / ROUNDER;
        if tidy(self.value(rounder)) == tidy(value) { rounder } else { tidy(travel) }
    }

    fn spoken(self, travel: f64) -> f64 {
        self.settled(tidy(self.value(travel)))
    }

    fn position(self, value: f64, kept: Option<f64>) -> f64 {
        match kept.filter(|travel| tidy(self.value(*travel)) == tidy(value)) {
            Some(travel) => travel.clamp(0.0, 1.0),
            None => self.settled(value),
        }
    }

    fn of_lfo(self, field: Option<LfoField>) -> Units {
        match field {
            Some(field) if field != LfoField::Center => Units::Share,
            _ => self,
        }
    }
}

fn fader_gain(position: f64) -> f64 {
    let unity = f64::from(UNITY_POSITION);
    if position >= unity { 10f64.powf((position - unity) / (1.0 - unity) * f64::from(TOP_DB) / 20.0) } else { position / unity }
}

pub(crate) fn fader_position(gain: f64) -> f64 {
    let unity = f64::from(UNITY_POSITION);
    if gain >= 1.0 { unity + 20.0 * gain.log10() / f64::from(TOP_DB) * (1.0 - unity) } else { gain * unity }
}

fn aim_to_json(fields: &mut Fields, aim: &Aim, rows: Rows) -> Option<Units> {
    let (row, target) = match aim {
        Aim::Gain => {
            fields.push("param", Json::String(String::from(TRACK_GAIN)));
            return Some(Units::Gain);
        }
        Aim::Send(send) => {
            fields.push("send", Json::Number(*send as f64));
            return Some(Units::Share);
        }
        Aim::Level(row) | Aim::Wet(row) | Aim::Knob { row, .. } => (*row, rows(*row)?),
    };
    fields.push("row", Json::Number(row as f64));
    let Aim::Knob { band, name, .. } = aim else {
        fields.push("param", Json::String(String::from(target.mix_key())));
        return (target.mix_aim(row).as_ref() == Some(aim)).then_some(Units::Share);
    };
    let knob = target.knobs().iter().find(|knob| knob.band == band && knob.name == name)?;
    fields.push("param", Json::String(knob.path.clone()));
    Some(Units::Knob(knob))
}

fn aim_from_json(json: &Json, rows: Landing) -> Option<(Aim, Units)> {
    if let Some(send) = index(Some(json), "send", SEND_COUNT) {
        return Some((Aim::Send(send), Units::Share));
    }
    let path = json.get("param").and_then(Json::as_str)?;
    if path == TRACK_GAIN && json.get("row").is_none() {
        return Some((Aim::Gain, Units::Gain));
    }
    let (row, target) = rows(index(Some(json), "row", RACK_ROWS)?)?;
    if path == target.mix_key() {
        return Some((target.mix_aim(row)?, Units::Share));
    }
    let knob = target.knobs().iter().find(|knob| knob.path == path)?;
    Some((Aim::knob(row, knob.band, knob.name), Units::Knob(knob)))
}

fn automation_to_json(fields: &mut Fields, automation: &Automation, rows: Rows) {
    fields.list("curves", automation.curves.iter().filter_map(|curve| curve_to_json(curve, rows)).collect());
    fields.list("lfos", automation.lfos.iter().filter_map(|lfo| lfo_to_json(lfo, rows)).collect());
}

params!(pub(super) impl LaneLfo (fields, read, document) {
    switch sync, "tempoSync" => "true cycles at division, false at rateHz";
    own division: Division, "division", |p, d, key, f| if p.sync { f.push(key, Json::from(p.division.name())) } else { f.name(key, p.division.name(), d.division.name()) }, |json, d, key| name(json, key).and_then(Division::from_name).unwrap_or(d.division), |d, key, about, f| f.manual(key, one_of(Division::KEYS, d.division.name()), about) => "cycle length when synced";
    own rate_hz: f64, "rateHz", |p, d, key, f| if p.sync { f.number(key, p.rate_hz, d.rate_hz) } else { f.push(key, Json::Number(p.rate_hz)) }, |json, d, key| sdk::schema::number(json, key, d.rate_hz).clamp(LaneLfo::MIN_HZ, LaneLfo::AUDIO_MAX_HZ), |d, key, about, f| f.manual(key, bounded(d.rate_hz, LaneLfo::MIN_HZ..=LaneLfo::MAX_HZ, false), about) => "free rate; audioRate pitch lanes reach 2000 Hz";
    number phase, "startPhase", 0.0..=1.0 => "where in the cycle the lane starts";
    flag retrigger, "retrigger" => "the cycle restarts at each new note leaving the midi row above the lane, a chord once, a tie or slide never; a midi row's own lane hears the notes entering it, and a gain or send lane the track's last midi row";
    flag legato, "legato" => "with retrigger on the song clock, restart only at a note that sounds when no earlier one still does";
    flag once, "once" => "one cycle, then hold";
    choice clock: LfoClock, "clock" => "song: song position; note: restarts on each new note leaving the midi row above the lane, a chord once, a tie or slide never; a midi row's own lane hears the notes entering that row";
    choice shape: LfoShape, "shape" => "walk is smoothed random; drawn follows points";
    own points: Drawn, "points", |p, d, key, f| if p.shape == LfoShape::Drawn || p.points != d.points { f.push(key, p.points.to_json()) }, |json, d, key| Drawn::from_json(items(json, key)).unwrap_or(d.points), |_, key, about, f| f.push(key, Drawn::manual(about)) => "2-32 points ordered by x, ends at x=0/1; y=0/1 spans the depth; default ramp 0-1";
    flag audio, "audioRate" => "audio-rate tuneSemitones/fineCents modulation, up to 2000 Hz";
    number fade_ms, "fadeMs", 0.0..=LaneLfo::MAX_FADE_MS => "modulation fade-in after restart";
    number lag_ms, "lagMs", 0.0..=LaneLfo::MAX_LAG_MS => "value slew; ignored by audioRate pitch lanes";
});

pub(crate) fn lfo_to_json(lfo: &LaneLfo, rows: Rows) -> Option<Json> {
    let mut fields = Fields::new();
    let units = aim_to_json(&mut fields, &lfo.aim, rows)?;
    fields.push("amount", Json::Number(lfo.amount));
    fields.push("center", Json::Number(units.value(lfo.centre)));
    if units.spoken(lfo.centre) != lfo.centre {
        fields.push("centerTravel", Json::Number(lfo.centre));
    }
    lfo.fields(&LaneLfo::blank(Aim::Gain), &mut fields);
    Some(fields.into_json())
}

fn rowed_lfos(json: Option<&Json>, rows: Landing) -> Vec<LaneLfo> {
    lfos_from_json(json, rows).into_iter().filter(|lfo| lfo.aim.row().is_some()).collect()
}

fn lfos_from_json(json: Option<&Json>, rows: Landing) -> Vec<LaneLfo> {
    json.and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(|lfo| lfo_from_json(lfo, rows)).collect()
}

fn lfo_from_json(json: &Json, rows: Landing) -> Option<LaneLfo> {
    let (aim, units) = aim_from_json(json, rows)?;
    let amount = json.get("amount").and_then(Json::as_f64)?.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return None;
    }
    let mut lfo = LaneLfo { amount, ..LaneLfo::blank(aim) };
    lfo.read(Some(json));
    lfo.set_audio(lfo.audio);
    lfo.centre = json.get("center").and_then(Json::as_f64).map_or(lfo.centre, |center| units.position(center, json.get("centerTravel").and_then(Json::as_f64)));
    Some(lfo)
}

pub(crate) fn clip_to_json(clip: &Clip) -> Json {
    let mut fields = Fields::new();
    fields.push("startBeat", Json::Number(beats(clip.start)));
    fields.push("lengthBeats", Json::Number(beats(clip.length)));
    fields.name("name", &clip.label, "");
    match &clip.file {
        Some(file) => fields.push("wave", wave_to_json(file)),
        None => fields.push("notes", Json::Array(clip.notes.iter().map(note_to_json).collect())),
    }
    fields.into_json()
}

fn note_to_json(note: &Note) -> Json {
    let mut fields = Fields::new();
    fields.push("pitch", Json::Number(f64::from(note.pitch)));
    fields.push("startBeat", number(note.start));
    fields.push("lengthBeats", number(note.length));
    fields.push("velocity", Json::Number(f64::from(note.velocity)));
    let articulation = note.articulation;
    if articulation.cents != 0.0 {
        fields.push("cents", number(articulation.cents));
    }
    if let Some(modulation) = articulation.modulation {
        fields.push("mod", number(modulation));
    }
    fields.number("shift", f64::from(articulation.shift), 0.0);
    fields.flag("tie", articulation.join == Join::Tie, false);
    fields.flag("slide", articulation.join == Join::Slide, false);
    fields.into_json()
}

params!(pub(super) impl File (fields, read, document) {
    number head, "headSeconds", -LONGEST_SECONDS..=LONGEST_SECONDS, "number" => "file offset at clip head; negative adds leading silence";
    number offset, "startSeconds", -LONGEST_SECONDS..=LONGEST_SECONDS, "number" => "start trim, added to headSeconds";
    number tail, "tailSeconds", 0.0..=LONGEST_SECONDS, "number 0 or more" => "end trim, in seconds";
    whole pitch, "semitones", -File::SEMITONES..=File::SEMITONES => "pitch shift of the wave";
    number fine, "cents", -File::CENTS..=File::CENTS => "fine pitch shift of the wave";
    number level, "levelDb", -File::DECIBELS..=File::DECIBELS => "the wave's level";
    flag warp, "warp" => "plays the file at the song's tempo, transient by transient; semitones then keep time";
    number tempo, "warpBpm", 0.0..=MOST_BPM, "number" => "the file's own tempo; 0 takes the song's when warp turns on";
    own gap: Loop, "warpGap", |p, d, key, f| f.name(key, p.gap.name(), d.gap.name()), |json, _, key| Loop::parse(name(json, key).unwrap_or_default()), |d, key, about, f| f.manual(key, Json::from(format!("rest, forward or backAndForth, default {}", d.gap.name())), about) => "what fills a gap when the song runs slower, silence, the segment's second half again, or that half back and forth";
    number envelope, "warpEnvelope", 0.0..=f64::from(ENVELOPE_MAX) => "with warpGap rest, how long each transient holds, 100 to the next";
});

fn wave_to_json(file: &File) -> Json {
    let mut fields = Fields::new();
    fields.push("name", Json::String(file.name.clone()));
    file.fields(&File::new(String::new()), &mut fields);
    let spans = file.spans.iter().map(|span| {
        let mut fields = Fields::new();
        fields.push("fromSeconds", Json::Number(span.from));
        fields.push("toSeconds", Json::Number(span.to));
        fields.push("levelDb", Json::Number(span.level));
        fields.into_json()
    });
    fields.list("spans", spans.collect());
    for slope in Slope::BOTH {
        if let Some(fade) = file.fades.of(slope) {
            fields.push(slope.key(), fade_to_json(fade));
        }
    }
    fields.into_json()
}

pub(crate) fn fade_to_json(fade: Fade) -> Json {
    let mut fields = Fields::new();
    fields.push("fromSeconds", Json::Number(fade.from));
    fields.push("toSeconds", Json::Number(fade.to));
    fields.number("bend", single(fade.bend), 0.0);
    fields.into_json()
}

fn file_from_json(json: &Json) -> Option<File> {
    let mut file = File::new(String::from(json.get("name").and_then(Json::as_str)?));
    file.read(Some(json));
    for span in json.get("spans").and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(span_from_json) {
        file.lay_span(span);
    }
    file.fades = fades_from_json(json, Slope::Fall);
    Some(file)
}

pub(crate) fn fades_from_json(wave: &Json, yielding: Slope) -> Fades {
    let written = |slope: Slope| wave.get(slope.key()).and_then(|fade| fade_from_json(fade, slope));
    [yielding.other(), yielding].into_iter().fold(Fades::NONE, |fades, slope| fades.with(slope, written(slope)))
}

fn fade_from_json(json: &Json, slope: Slope) -> Option<Fade> {
    let at = |key: &str| json.get(key).and_then(Json::as_f64).filter(|value| value.is_finite());
    let (from, to) = (at("fromSeconds")?.clamp(0.0, LONGEST_SECONDS), at("toSeconds")?.clamp(0.0, LONGEST_SECONDS));
    let (from, to) = match slope {
        Slope::Rise => (from.min(to), to),
        Slope::Fall => (from, to.max(from)),
    };
    Some(Fade { from, to, bend: at("bend").unwrap_or(0.0).clamp(-1.0, 1.0) as f32 })
}

fn span_from_json(json: &Json) -> Option<Span> {
    let at = |key: &str| json.get(key).and_then(Json::as_f64).filter(|value| value.is_finite());
    let (from, to) = (at("fromSeconds")?.clamp(0.0, LONGEST_SECONDS), at("toSeconds")?.clamp(0.0, LONGEST_SECONDS));
    let level = at("levelDb")?.clamp(-File::DECIBELS, File::DECIBELS);
    (to - from >= Span::LEAST).then_some(Span { from, to, level })
}

pub fn wave_names(document: &Json, names: &mut Vec<String>) {
    for track in document.get("tracks").and_then(Json::as_array).unwrap_or(&[]) {
        for clip in track.get("clips").and_then(Json::as_array).unwrap_or(&[]) {
            if let Some(name) = clip.get("wave").and_then(|wave| wave.get("name")).and_then(Json::as_str)
                && !names.iter().any(|known| known == name)
            {
                names.push(String::from(name));
            }
        }
    }
}

pub fn clip_from_json(json: &Json) -> Option<Clip> {
    let at = |key: &str, default: f64| ticks(json.get(key).and_then(Json::as_f64).filter(|n| n.is_finite()).unwrap_or(default));
    let start = at("startBeat", 0.0);
    let length = at("lengthBeats", f64::from(BEATS_PER_BAR));
    if start < 0 || length < 1 || start > LONGEST_TICKS - length {
        return None;
    }
    let notes = json.get("notes").and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(|note| {
        let float = |key: &str, default: f32| note.get(key).and_then(Json::as_f64).map(|n| n as f32).filter(|n| n.is_finite()).unwrap_or(default);
        let pitch = u8::try_from(note.get("pitch").and_then(Json::as_i64)?).ok().filter(|pitch| *pitch <= 127)?;
        let note_start = float("startBeat", 0.0);
        let note_length = float("lengthBeats", beats(length) as f32 - note_start);
        let velocity = note.get("velocity").and_then(Json::as_i64).map_or(VELOCITY, |v| v.clamp(1, 127) as u8);
        let flag = |key: &str| note.get(key).and_then(Json::as_bool).unwrap_or(false);
        let join = match (flag("slide"), flag("tie")) {
            (true, _) => Join::Slide,
            (false, true) => Join::Tie,
            (false, false) => Join::Play,
        };
        let articulation = Articulation {
            cents: float("cents", 0.0).clamp(-Articulation::MAX_CENTS, Articulation::MAX_CENTS),
            modulation: note.get("mod").and_then(Json::as_f64).filter(|m| m.is_finite()).map(|m| m.clamp(-1.0, 1.0) as f32),
            shift: note.get("shift").and_then(Json::as_i64).map_or(0, |shift| shift.clamp(-i64::from(Articulation::MAX_SHIFT), i64::from(Articulation::MAX_SHIFT)) as i8),
            join,
        };
        (note_start >= 0.0 && note_length > 0.0).then_some(Note { pitch, start: note_start, length: note_length, velocity, articulation })
    });
    let file = json.get("wave").and_then(file_from_json);
    let notes = if file.is_some() { Vec::new() } else { notes.collect() };
    Some(Clip { start, length, label: String::from(json.get("name").and_then(Json::as_str).unwrap_or("")), notes, file })
}

pub(crate) fn curve_to_json(curve: &Curve, rows: Rows) -> Option<Json> {
    let mut fields = Fields::new();
    let aimed = aim_to_json(&mut fields, &curve.aim, rows)?;
    if let Some(field) = curve.lfo {
        fields.push("lfoParam", Json::String(String::from(field.name())));
    }
    let units = aimed.of_lfo(curve.lfo);
    let nodes = curve.nodes.iter().map(|node| {
        let mut fields = Fields::new();
        fields.push("beat", Json::Number(beats(node.tick)));
        let travel = single(node.travel);
        fields.push("value", Json::Number(units.value(travel)));
        if units.spoken(travel) as f32 != node.travel {
            fields.push("travel", Json::Number(travel));
        }
        fields.push("bend", number(node.bend));
        fields.into_json()
    });
    fields.push("nodes", Json::Array(nodes.collect()));
    Some(fields.into_json())
}

fn curves_from_json(json: Option<&Json>, rows: Landing) -> Vec<Curve> {
    json.and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(|curve| curve_from_json(curve, rows)).collect()
}

fn curve_from_json(json: &Json, rows: Landing) -> Option<Curve> {
    let (aim, units) = aim_from_json(json, rows)?;
    let lfo = match json.get("lfoParam") {
        None => None,
        Some(key) => Some(key.as_str().and_then(LfoField::from_name)?),
    };
    let units = units.of_lfo(lfo);
    let pairs = json.get("nodes").and_then(Json::as_array)?.iter().filter_map(|node| {
        let beat = match (node.get("beat").and_then(Json::as_f64), node.get("bar").and_then(Json::as_f64)) {
            (Some(beat), _) => beat,
            (None, Some(bar)) => (bar - 1.0) * f64::from(BEATS_PER_BAR),
            (None, None) => return None,
        };
        let tick = ticks(beat);
        let value = match (node.get("value").and_then(Json::as_f64), node.get("valueDb").and_then(Json::as_f64)) {
            (Some(value), _) => value,
            (None, Some(db)) if matches!(units, Units::Gain) => 10f64.powf(db / 20.0),
            _ => return None,
        };
        let travel = units.position(value, node.get("travel").and_then(Json::as_f64)) as f32;
        let bend = node.get("bend").and_then(Json::as_f64).unwrap_or(0.0) as f32;
        (0..=LONGEST_TICKS).contains(&tick).then_some(Node { tick, travel, bend: bend.clamp(-1.0, 1.0) })
    });
    let mut nodes: Vec<Node> = pairs.collect();
    nodes.sort_by_key(|node| node.tick);
    nodes.dedup_by_key(|node| node.tick);
    (!nodes.is_empty()).then_some(Curve { aim, lfo, nodes })
}

pub(crate) fn inside_of(patch: &Params, band: &str, param: &str) -> Vec<(String, &'static str, f64)> {
    let motion = &patch.motion;
    let slots = [("lfo1", motion.lfo1.target, motion.lfo1.amount), ("lfo2", motion.lfo2.target, motion.lfo2.amount), ("env1", motion.env1.target, motion.env1.amount), ("env2", motion.env2.target, motion.env2.amount)];
    let aimed = slots.into_iter().filter(|(_, target, _)| aims(*target, band, param)).map(|(source, _, amount)| (format!("motion/{source}/amount"), source, amount));
    let (filter, modulation, analog) = (&patch.filter, &patch.modulation, patch.analog.heard());
    let mut fixed = match (band, param) {
        ("FILTER", "CUTOFF") => vec![("filter/envAmount", "filter env", filter.env_amount), ("filter/keyTrack", "key", filter.key_track), ("filter/env/velocityAmount", "velocity", if filter.env_amount == 0.0 { 0.0 } else { filter.env.velocity }), ("filter/fmAmount", patch.modulator.name(), filter.fm), ("mod/cutoffOctaves", "mod", modulation.cutoff)],
        ("FILTER", "RESONANCE") => vec![("filter/resonanceVelocity", "velocity", filter.resonance_velocity)],
        ("OSC 1", "LEVEL") => vec![("mod/osc1Level", "mod", modulation.level1)],
        ("OSC 2", "LEVEL") => vec![("mod/osc2Level", "mod", modulation.level2)],
        ("OSC 2", "TUNE" | "FINE") => vec![("mod/osc2Semitones", "mod", modulation.tune2)],
        ("OSC 3", "TUNE" | "FINE") => vec![("mod/osc3Semitones", "mod", modulation.tune3)],
        ("OSC 4", "TUNE" | "FINE") => vec![("mod/osc4Semitones", "mod", modulation.tune4)],
        _ => Vec::new(),
    };
    match (PITCH_BANDS.iter().position(|known| *known == band), band, param) {
        (Some(_), _, "TUNE" | "FINE") => fixed.extend([("analog/driftCents", "scatter", analog.detune), ("analog/wanderCents", "drift", analog.wander), ("analog/jitterCents", "jitter", analog.jitter)]),
        (Some(osc), _, "PULSE WIDTH") if patch.oscs()[osc].pure() => fixed.extend([("mod/pulseWidth", "mod", modulation.pulse_width), ("analog/widthWander", "drift", analog.width_wander), ("analog/widthScatter", "scatter", analog.width_scatter)]),
        (None, "FILTER", "CUTOFF") => fixed.extend([("analog/cutoffWanderCents", "drift", analog.cutoff_wander), ("analog/cutoffScatterCents", "scatter", analog.cutoff_scatter)]),
        (None, "AMP ENV" | "FILTER ENV", "ATTACK" | "DECAY" | "RELEASE") => fixed.push(("analog/envScatter", "scatter", analog.env_scatter)),
        _ => {}
    }
    let fixed = fixed.into_iter().filter(|(_, _, amount)| *amount != 0.0).map(|(key, source, amount)| (String::from(key), source, amount));
    aimed.chain(fixed).collect()
}

fn aims(target: Target, band: &str, param: &str) -> bool {
    let Some(osc) = PITCH_BANDS.iter().position(|known| *known == band) else {
        return [("NOISE", "LEVEL", Target::Noise), ("FILTER", "CUTOFF", Target::Cutoff), ("FILTER", "RESONANCE", Target::Resonance), ("LFO 1", "RATE", Target::Lfo1Rate), ("LFO 2", "RATE", Target::Lfo2Rate), ("LFO 1", "AMOUNT", Target::Lfo1Amount), ("LFO 2", "AMOUNT", Target::Lfo2Amount)].contains(&(band, param, target));
    };
    match param {
        "TUNE" | "FINE" => target == Target::Pitch || target == [Target::Pitch1, Target::Pitch2, Target::Pitch3, Target::Pitch4][osc],
        "FM" => target == Target::Fm || target == Target::fm_of(osc),
        "LEVEL" => target == [Target::Level1, Target::Level2, Target::Level3, Target::Level4][osc],
        "PULSE WIDTH" => target == [Target::PulseWidth1, Target::PulseWidth2, Target::PulseWidth3, Target::PulseWidth4][osc],
        _ => false,
    }
}
