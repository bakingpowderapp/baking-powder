use sdk::schema::choice;
use std::cmp::Reverse;

choice!(pub Root { C = "C", Db = "Db", D = "D", Eb = "Eb", E = "E", F = "F", Gb = "Gb", G = "G", Ab = "Ab", A = "A", Bb = "Bb", B = "B" });

impl Root {
    fn pitch_class(self) -> i32 {
        self.index() as i32
    }
}

choice!(pub ScaleName { Major = "MAJOR" / "major", Minor = "MINOR" / "minor", Dorian = "DORIAN" / "dorian", Phrygian = "PHRYGIAN" / "phrygian", Lydian = "LYDIAN" / "lydian", Mixolydian = "MIXOLYDIAN" / "mixolydian" });

impl ScaleName {
    fn intervals(self) -> [i32; 7] {
        match self {
            ScaleName::Major => [0, 2, 4, 5, 7, 9, 11],
            ScaleName::Minor => [0, 2, 3, 5, 7, 8, 10],
            ScaleName::Dorian => [0, 2, 3, 5, 7, 9, 10],
            ScaleName::Phrygian => [0, 1, 3, 5, 7, 8, 10],
            ScaleName::Lydian => [0, 2, 4, 6, 7, 9, 11],
            ScaleName::Mixolydian => [0, 2, 4, 5, 7, 9, 10],
        }
    }

    fn degree_qualities(self) -> [ChordQuality; 7] {
        use ChordQuality::{Diminished as Dim, Major as Maj, Minor as Min};
        match self {
            ScaleName::Major => [Maj, Min, Min, Maj, Maj, Min, Dim],
            ScaleName::Minor => [Min, Dim, Maj, Min, Min, Maj, Maj],
            ScaleName::Dorian => [Min, Min, Maj, Maj, Min, Dim, Maj],
            ScaleName::Phrygian => [Min, Maj, Maj, Min, Dim, Maj, Min],
            ScaleName::Lydian => [Maj, Maj, Min, Dim, Maj, Min, Min],
            ScaleName::Mixolydian => [Maj, Min, Dim, Maj, Min, Min, Maj],
        }
    }

    fn parent_offset(self) -> i32 {
        match self {
            ScaleName::Major => 0,
            ScaleName::Minor => 3,
            ScaleName::Dorian => 10,
            ScaleName::Phrygian => 8,
            ScaleName::Lydian => 7,
            ScaleName::Mixolydian => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChordQuality {
    Major,
    Minor,
    Diminished,
}

const SHARP_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

fn wrap12(x: i32) -> i32 {
    x.rem_euclid(12)
}

fn pc_from_name(name: &str) -> Option<i32> {
    SHARP_NAMES.iter().position(|n| *n == name).map(|p| p as i32).or_else(|| Root::from_name(name).map(Root::pitch_class))
}

pub fn prefer_flats(root: Root, scale: ScaleName) -> bool {
    if root.name().contains('b') {
        return true;
    }
    let parent_pc = wrap12(root.pitch_class() + scale.parent_offset());
    matches!(parent_pc, 1 | 3 | 5 | 8 | 10)
}

pub fn pitch_class_name(pc: i32, flats: bool) -> &'static str {
    let p = wrap12(pc) as usize;
    if flats { Root::ALL[p].name() } else { SHARP_NAMES[p] }
}

fn scale_notes(root: Root, scale: ScaleName, base_octave: i32) -> [String; 7] {
    let root_pc = root.pitch_class();
    let flats = prefer_flats(root, scale);
    let intervals = scale.intervals();
    std::array::from_fn(|i| {
        let abs = root_pc + intervals[i];
        format!("{}{}", pitch_class_name(abs, flats), base_octave + abs.div_euclid(12))
    })
}

pub fn scale_pitch_classes(root: Root, scale: ScaleName) -> [i32; 7] {
    let intervals = scale.intervals();
    std::array::from_fn(|i| wrap12(root.pitch_class() + intervals[i]))
}

pub fn key_bits(root: Root, scale: ScaleName) -> u16 {
    scale_pitch_classes(root, scale).into_iter().fold((root.pitch_class() as u16) << 12, |bits, class| bits | 1 << class)
}

pub const FITS: usize = 5;
const KEY_COLUMN: usize = 13;
const DEGREE_WEIGHT: [i32; 7] = [6, 1, 2, 1, 3, 1, 1];
const HOME_SLACK_PERCENT: i32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fit {
    pub root: Root,
    pub scale: ScaleName,
    pub percent: i32,
}

impl Fit {
    pub fn label(self) -> String {
        let key = format!("{} {}", flat_spelled(self.root.name()), ScaleName::LABELS[self.scale.index()]);
        format!("{key:<width$}{:>4}%", self.percent, width = KEY_COLUMN)
    }
}

fn ranked(heard: &[i32; 12], bass: &[i32; 12]) -> Vec<(i32, Fit)> {
    let total: i32 = heard.iter().sum();
    if total == 0 {
        return Vec::new();
    }
    let mut ranked = Vec::new();
    for &root in Root::ALL {
        for &scale in ScaleName::ALL {
            let (mut inside, mut weight) = (0, 0);
            for (degree, class) in scale_pitch_classes(root, scale).into_iter().enumerate() {
                let class = class as usize;
                inside += heard[class];
                weight += (heard[class] + bass[class]) * DEGREE_WEIGHT[degree];
            }
            ranked.push((Reverse((inside, weight)), Fit { root, scale, percent: inside * 100 / total }));
        }
    }
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(Reverse((inside, _)), fit)| (inside, fit)).collect()
}

pub fn fits(heard: &[i32; 12], bass: &[i32; 12]) -> Vec<Fit> {
    ranked(heard, bass).into_iter().map(|(_, fit)| fit).take(FITS).collect()
}

pub fn keyed(heard: &[i32; 12], bass: &[i32; 12], home: Option<(Root, ScaleName)>) -> Option<(Fit, i32)> {
    let keys = ranked(heard, bass);
    let (best_inside, best) = *keys.first()?;
    let total: i32 = heard.iter().sum();
    let at_home = home.and_then(|(root, scale)| keys.iter().find(|(inside, fit)| fit.root == root && fit.scale == scale && (best_inside - inside) * 100 <= HOME_SLACK_PERCENT * total)).copied();
    let (inside, fit) = at_home.unwrap_or((best_inside, best));
    Some((fit, total - inside))
}

fn split_note(name: &str) -> (&str, i32) {
    let bytes = name.as_bytes();
    let mut octave_start = bytes.len();
    for i in (0..bytes.len()).rev() {
        if bytes[i].is_ascii_digit() || bytes[i] == b'-' {
            octave_start = i;
        } else {
            break;
        }
    }
    (&name[..octave_start], name[octave_start..].parse().unwrap_or(0))
}

fn pitch_class(note: &str) -> &str {
    split_note(note).0
}

fn bump_octave(note: &str, delta: i32) -> String {
    let (pc, octave) = split_note(note);
    format!("{pc}{}", octave + delta)
}

fn note_at_semitone_offset(base_note: &str, semitones: i32, flats: bool) -> String {
    let (pc, octave) = split_note(base_note);
    let abs = pc_from_name(pc).unwrap_or(0) + semitones;
    format!("{}{}", pitch_class_name(wrap12(abs), flats), octave + abs.div_euclid(12))
}

fn note_name_to_midi(name: &str) -> u8 {
    let (pc, octave) = split_note(name);
    ((octave + 1) * 12 + pc_from_name(pc).unwrap_or(0)).clamp(0, 127) as u8
}

pub fn degree_index(degree: i32) -> usize {
    (degree - 1).rem_euclid(7) as usize
}

const ROMANS: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];

fn roman_for_degree(degree: i32, scale: ScaleName) -> String {
    let at = degree_index(degree);
    match scale.degree_qualities()[at] {
        ChordQuality::Major => ROMANS[at].to_string(),
        ChordQuality::Minor => ROMANS[at].to_lowercase(),
        ChordQuality::Diminished => format!("{}°", ROMANS[at].to_lowercase()),
    }
}

pub fn quality_for(degree: i32, scale: ScaleName) -> ChordQuality {
    scale.degree_qualities()[degree_index(degree)]
}

pub fn parallel_flip(scale: ScaleName) -> ScaleName {
    match scale {
        ScaleName::Major | ScaleName::Lydian | ScaleName::Mixolydian => ScaleName::Minor,
        ScaleName::Minor | ScaleName::Dorian | ScaleName::Phrygian => ScaleName::Major,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChordAlterations {
    pub sharp3: bool,
    pub flat7: bool,
}

choice!(pub ChordVariation { Triad = "triad", Add9 = "add9", Seventh = "seventh", Ninth = "ninth", Sus2 = "sus2", Sus4 = "sus4", Dom7 = "dom7", Add6 = "add6", SixNine = "six-nine", Power = "power", SevenSus4 = "seven-sus4", Eleventh = "eleventh" });

pub const DISPLAYED_VARIATIONS: [ChordVariation; 11] = [
    ChordVariation::Triad,
    ChordVariation::Power,
    ChordVariation::Add9,
    ChordVariation::Add6,
    ChordVariation::SixNine,
    ChordVariation::Seventh,
    ChordVariation::Ninth,
    ChordVariation::Eleventh,
    ChordVariation::Sus2,
    ChordVariation::Sus4,
    ChordVariation::SevenSus4,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegreeIntervals {
    pub is_b9: bool,
    pub sixth_is_major: bool,
    pub seventh_is_major: bool,
    pub fourth_is_augmented: bool,
}

pub fn degree_intervals(root: Root, scale: ScaleName, degree: i32) -> DegreeIntervals {
    let notes = scale_notes(root, scale, 3);
    let rp = degree_index(degree);
    let pc_of = |n: &str| pc_from_name(pitch_class(n)).unwrap_or(0);
    let root_pc = pc_of(&notes[rp]);
    let interval = |steps: usize| wrap12(pc_of(&notes[(rp + steps) % 7]) - root_pc);
    DegreeIntervals {
        is_b9: interval(1) == 1,
        sixth_is_major: interval(5) == 9,
        seventh_is_major: interval(6) == 11,
        fourth_is_augmented: interval(3) == 6,
    }
}

fn bass_name_for_inversion(root: Root, scale: ScaleName, degree: i32, variation: ChordVariation, bass_offset: i32, alterations: ChordAlterations) -> String {
    let notes = scale_notes(root, scale, 3);
    let rp = degree_index(degree);
    if bass_offset == 0 {
        return pitch_class(&notes[rp]).to_string();
    }
    let is_add6_family = matches!(variation, ChordVariation::Add6 | ChordVariation::SixNine);
    let second_inv_offset = if is_add6_family { 5 } else { 4 };
    let idx = (rp + if bass_offset == 1 { 2 } else { second_inv_offset }) % 7;
    let q = quality_for(degree, scale);
    let raised = alterations.sharp3 && ((bass_offset == 1 && q != ChordQuality::Major) || (bass_offset == 2 && !is_add6_family && q == ChordQuality::Diminished));
    if !raised {
        return pitch_class(&notes[idx]).to_string();
    }
    pitch_class(&note_at_semitone_offset(&notes[idx], 1, false)).to_string()
}

fn chord_label(root: Root, scale: ScaleName, degree: i32, variation: ChordVariation, alterations: ChordAlterations) -> String {
    use ChordQuality::{Diminished, Major, Minor};
    use ChordVariation as V;
    let notes = scale_notes(root, scale, 3);
    let r = pitch_class(&notes[degree_index(degree)]).to_string();
    let q = if alterations.sharp3 { Major } else { quality_for(degree, scale) };
    let iv = degree_intervals(root, scale, degree);
    let seventh_is_major = !alterations.flat7 && iv.seventh_is_major;
    let dim5 = if q == Diminished { "(b5)" } else { "" };
    let sus4 = if iv.fourth_is_augmented { "sus#4" } else { "sus4" };
    let seventh = if seventh_is_major { "maj7" } else { "7" };
    match variation {
        V::Triad => match q {
            Major => r,
            Minor => format!("{r}m"),
            Diminished => format!("{r}°"),
        },
        V::Sus2 => format!("{r}{}{dim5}", if iv.is_b9 { "susb2" } else { "sus2" }),
        V::Sus4 => format!("{r}{sus4}{dim5}"),
        V::Dom7 => format!("{r}7"),
        V::Power => format!("{r}{}", if q == Diminished { "(b5)" } else { "5" }),
        V::SevenSus4 => format!("{r}{seventh}{sus4}{dim5}"),
        V::Eleventh => {
            if iv.fourth_is_augmented {
                return format!("{r}{}", if seventh_is_major { "maj9(#11)" } else { "9(#11)" });
            }
            match q {
                Major => format!("{r}{}", if seventh_is_major { "maj11" } else { "11" }),
                Minor => format!("{r}m11"),
                Diminished => format!("{r}ø11"),
            }
        }
        V::Add6 => match q {
            Major => format!("{r}{}", if iv.sixth_is_major { "6" } else { "(addb6)" }),
            Minor => format!("{r}{}", if iv.sixth_is_major { "m6" } else { "m(b6)" }),
            Diminished => format!("{r}{}", if iv.sixth_is_major { "°(add6)" } else { "°(addb6)" }),
        },
        V::SixNine => {
            let ext = if iv.is_b9 { "add11" } else { "add9" };
            match (q, iv.sixth_is_major, iv.is_b9) {
                (Major, true, false) => format!("{r}6/9"),
                (Major, true, true) => format!("{r}6(add11)"),
                (Major, false, _) => format!("{r}(b6,{ext})"),
                (Minor, true, false) => format!("{r}m6/9"),
                (Minor, true, true) => format!("{r}m6(add11)"),
                (Minor, false, _) => format!("{r}m(b6,{ext})"),
                (Diminished, true, false) => format!("{r}°6/9"),
                (Diminished, true, true) => format!("{r}°6(add11)"),
                (Diminished, false, _) => format!("{r}°(addb6,{ext})"),
            }
        }
        V::Add9 => {
            let ext = if iv.is_b9 { "(add11)" } else { "(add9)" };
            match q {
                Major => format!("{r}{ext}"),
                Minor => format!("{r}m{ext}"),
                Diminished => format!("{r}°{ext}"),
            }
        }
        V::Seventh => match q {
            Major => format!("{r}{seventh}"),
            Minor => format!("{r}m7"),
            Diminished => format!("{r}ø7"),
        },
        V::Ninth => {
            if iv.is_b9 {
                return match q {
                    Major => format!("{r}{}", if seventh_is_major { "maj7(add11)" } else { "7(add11)" }),
                    Minor => format!("{r}m11"),
                    Diminished => format!("{r}ø11"),
                };
            }
            match q {
                Major => format!("{r}{}", if seventh_is_major { "maj9" } else { "9" }),
                Minor => format!("{r}m9"),
                Diminished => format!("{r}ø9"),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChordInfo {
    pub label: String,
    pub roman: String,
    pub midi_notes: Vec<u8>,
}

fn chord_voicing(root: Root, scale: ScaleName, degree: i32, variation: ChordVariation, bass_offset: i32, alterations: ChordAlterations) -> Vec<String> {
    let notes = scale_notes(root, scale, 3);
    let rp = degree_index(degree) as i32;
    let bass_name = bass_name_for_inversion(root, scale, degree, variation, bass_offset, alterations);
    let get = |pos: i32| bump_octave(&notes[pos.rem_euclid(7) as usize], pos.div_euclid(7));
    let iv = degree_intervals(root, scale, degree);
    let q = quality_for(degree, scale);
    let raise_third = alterations.sharp3 && q != ChordQuality::Major;
    let raise_fifth = alterations.sharp3 && q == ChordQuality::Diminished;
    let lower_seventh = alterations.flat7 && iv.seventh_is_major;
    let second = get(rp + 1);
    let third = if raise_third { note_at_semitone_offset(&get(rp + 2), 1, false) } else { get(rp + 2) };
    let fourth = get(rp + 3);
    let fifth = if raise_fifth { note_at_semitone_offset(&get(rp + 4), 1, false) } else { get(rp + 4) };
    let sixth = get(rp + 5);
    let seventh = if lower_seventh { note_at_semitone_offset(&get(rp + 6), -1, true) } else { get(rp + 6) };
    let ninth = get(rp + 8);
    let middle = match variation {
        ChordVariation::Sus2 => second,
        ChordVariation::Sus4 | ChordVariation::SevenSus4 => fourth.clone(),
        _ => third,
    };
    let is_add6_family = matches!(variation, ChordVariation::Add6 | ChordVariation::SixNine);
    let fifth_slot = if is_add6_family { sixth } else { fifth };
    let mut voicing = vec![format!("{bass_name}1"), format!("{bass_name}2"), fifth_slot];
    if variation != ChordVariation::Power {
        voicing.push(middle);
    }
    voicing.push(bump_octave(&notes[degree_index(degree)], 1));
    let has7 = matches!(variation, ChordVariation::Seventh | ChordVariation::Ninth | ChordVariation::SevenSus4 | ChordVariation::Eleventh);
    if has7 {
        voicing.push(seventh);
    }
    if matches!(variation, ChordVariation::Add9 | ChordVariation::Ninth | ChordVariation::SixNine) {
        voicing.push(if iv.is_b9 { bump_octave(&fourth, 1) } else { ninth.clone() });
    }
    if variation == ChordVariation::Eleventh {
        if !iv.is_b9 {
            voicing.push(ninth);
        }
        voicing.push(bump_octave(&fourth, 1));
    }
    voicing
}

pub fn chord_info(root: Root, scale: ScaleName, degree: i32, variation: ChordVariation, bass_offset: i32, alterations: ChordAlterations) -> ChordInfo {
    let voicing = chord_voicing(root, scale, degree, variation, bass_offset, alterations);
    let midi_notes = voicing.iter().map(|n| note_name_to_midi(n)).collect();
    let mut label = chord_label(root, scale, degree, variation, alterations);
    if bass_offset > 0 {
        label.push('/');
        label.push_str(&bass_name_for_inversion(root, scale, degree, variation, bass_offset, alterations));
    }
    let roman = if alterations.sharp3 { ROMANS[degree_index(degree)].to_string() } else { roman_for_degree(degree, scale) };
    ChordInfo { label, roman, midi_notes }
}

pub fn dom_chord_info(root: Root, scale: ScaleName, degree: i32, roman_prefix: &str) -> ChordInfo {
    let column_notes = scale_notes(root, scale, 3);
    let target_note = &column_notes[degree_index(degree)];
    let dom_root = note_at_semitone_offset(target_note, 7, pitch_class(target_note).contains('b'));
    let dom_root_pc = pitch_class(&dom_root).to_string();
    let dom_flats = dom_root_pc.contains('b');
    let voicing = [
        format!("{dom_root_pc}1"),
        format!("{dom_root_pc}2"),
        note_at_semitone_offset(&dom_root, 7, dom_flats),
        note_at_semitone_offset(&dom_root, 4, dom_flats),
        bump_octave(&dom_root, 1),
        note_at_semitone_offset(&dom_root, 10, !dom_root_pc.contains('#')),
    ];
    ChordInfo {
        label: format!("{dom_root_pc}7"),
        roman: format!("V/{roman_prefix}{}", roman_for_degree(degree, scale)),
        midi_notes: voicing.iter().map(|n| note_name_to_midi(n)).collect(),
    }
}

pub fn flat_spelled(label: &str) -> String {
    label.replace('b', "\u{266D}")
}
