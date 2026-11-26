use super::Project;
use crate::theory::{ChordAlterations, ChordInfo, ChordQuality, ChordVariation, ScaleName, chord_info, degree_index, degree_intervals, dom_chord_info, parallel_flip, quality_for, scale_pitch_classes};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaletteColumn {
    pub selected: ChordVariation,
    pub parallel: bool,
    pub sharp3: bool,
    pub flat7: bool,
    pub bass_offset: i32,
    pub octave: i32,
    pub bottom: i32,
    pub top: i32,
    pub lift: i32,
}

impl PaletteColumn {
    pub const DEFAULT: PaletteColumn = PaletteColumn { selected: ChordVariation::Triad, parallel: false, sharp3: false, flat7: false, bass_offset: 0, octave: 0, bottom: 0, top: 0, lift: 0 };
    pub const OCTAVE_REACH: i32 = 2;

    pub fn picked(self, count: usize) -> (usize, usize) {
        let most = count.saturating_sub(1);
        ((self.bottom.max(0) as usize).min(most), (self.top.max(0) as usize).min(most))
    }

    pub fn split(self, count: usize) -> bool {
        self.picked(count) != (0, 0)
    }

    pub fn voice(self, mut notes: Vec<u8>) -> Vec<u8> {
        notes.sort_unstable();
        let count = notes.len();
        let (bottom, top) = self.picked(count);
        notes
            .into_iter()
            .enumerate()
            .filter_map(|(at, note)| {
                let lifted = at < bottom || at >= count - top;
                let shift = (self.octave + if lifted { self.lift } else { 0 }) * 12;
                u8::try_from(i32::from(note) + shift).ok().filter(|n| *n <= 127)
            })
            .collect()
    }
}

pub const DECKS: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub deck: usize,
    pub degree: i32,
    pub variation: ChordVariation,
}

impl Project {
    pub fn column(&self, deck: usize, degree: i32) -> PaletteColumn {
        self.decks[deck][degree_index(degree)]
    }

    pub fn column_mut(&mut self, deck: usize, degree: i32) -> &mut PaletteColumn {
        &mut self.decks[deck][degree_index(degree)]
    }

    pub fn effective_scale(&self, deck: usize, degree: i32) -> ScaleName {
        if self.column(deck, degree).parallel { parallel_flip(self.scale) } else { self.scale }
    }

    pub fn sharp3_usable(&self, deck: usize, degree: i32) -> bool {
        quality_for(degree, self.effective_scale(deck, degree)) != ChordQuality::Major
    }

    pub fn flat7_usable(&self, deck: usize, degree: i32) -> bool {
        degree_intervals(self.root, self.effective_scale(deck, degree), degree).seventh_is_major
    }

    pub fn drop_inert_alterations(&mut self, deck: usize, degree: i32) {
        let sharp3 = self.sharp3_usable(deck, degree);
        let flat7 = self.flat7_usable(deck, degree);
        let column = self.column_mut(deck, degree);
        column.sharp3 &= sharp3;
        column.flat7 &= flat7;
    }

    fn borrowed_roman_prefix(&self, degree: i32, parallel: bool) -> &'static str {
        if !parallel {
            return "";
        }
        let at = degree_index(degree);
        let home = scale_pitch_classes(self.root, self.scale)[at];
        let borrowed = scale_pitch_classes(self.root, parallel_flip(self.scale))[at];
        match (borrowed - home).rem_euclid(12) {
            1 => "♯",
            11 => "♭",
            _ => "",
        }
    }

    pub fn chord(&self, chord: Chord) -> ChordInfo {
        self.chord_with_column(chord, self.column(chord.deck, chord.degree))
    }

    pub fn chord_with_column(&self, chord: Chord, column: PaletteColumn) -> ChordInfo {
        let scale = if column.parallel { parallel_flip(self.scale) } else { self.scale };
        let prefix = self.borrowed_roman_prefix(chord.degree, column.parallel);
        if chord.variation == ChordVariation::Dom7 {
            return dom_chord_info(self.root, scale, chord.degree, prefix);
        }
        let alterations = ChordAlterations { sharp3: column.sharp3, flat7: column.flat7 };
        let base = chord_info(self.root, scale, chord.degree, chord.variation, column.bass_offset, alterations);
        ChordInfo { roman: format!("{prefix}{}", base.roman), ..base }
    }

    pub fn chord_notes(&self, chord: Chord) -> Vec<u8> {
        self.column(chord.deck, chord.degree).voice(self.chord(chord).midi_notes)
    }
}
