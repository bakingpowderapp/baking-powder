use sdk::draw::{DrawList, Rect, Tone};
use sdk::font;
use crate::project::{Chord, ClipChord, PaletteColumn, Project, Sounding};
use crate::theory::{ChordQuality, ChordVariation, DISPLAYED_VARIATIONS, Root, ScaleName, degree_index, degree_intervals, flat_spelled, parallel_flip, quality_for};

pub const SCALE: f32 = 1.4;
const TILE_HEADER_H: i32 = 37;
const PILL_H: i32 = 14;
const TILE_GAP: i32 = 3;
const ROW_GAP: i32 = 1;
const COL_GAP: i32 = 6;
const PILL_GAP: i32 = 3;
const GRIP_W: i32 = 13;
const DRAG_START: i32 = 4;
const CHUNK_W: i32 = 2;
const CHUNK_H: i32 = 5;
const CHUNK_GAP: i32 = 1;
const CHUNK_LEAD: i32 = 3;
const MAX_CHUNKS: usize = 6;
const PLACES_SHOWN: usize = 3;
const PANEL_FRAME: i32 = 2;
const PANEL_PAD: i32 = 4;
const PANEL_LINE_GAP: i32 = 2;
const ROWS: i32 = 12;
const ROW_DOM: u8 = 11;
const DEGREES: u8 = 7;
const INV_Y: i32 = PILL_H + TILE_GAP;
const OCT_Y: i32 = 2 * (PILL_H + TILE_GAP);
const HEADER_Y: i32 = 3 * (PILL_H + TILE_GAP);
const ROWS_Y: i32 = HEADER_Y + TILE_HEADER_H + TILE_GAP;

type Key = (Root, ScaleName, [PaletteColumn; 7]);

struct Spelled {
    roman: String,
    label: String,
    column: PaletteColumn,
    classes: Vec<u8>,
    count: usize,
}

impl Spelled {
    fn new(project: &Project, chord: Chord, column: PaletteColumn) -> Spelled {
        let info = project.chord_with_column(chord, column);
        let notes = column.voice(info.midi_notes);
        Spelled { roman: info.roman, label: flat_spelled(&info.label), column, classes: pitch_classes(&notes), count: notes.len() }
    }

    fn label_in(&self, width: i32) -> String {
        font::shortened(&self.label, font::glyphs_in(width - 4, 1))
    }
}

struct Chords {
    key: Option<Key>,
    spelled: Vec<Vec<Spelled>>,
}

impl Chords {
    fn refresh(&mut self, project: &Project, deck: usize) {
        let key = (project.root, project.scale, project.decks[deck].map(|column| PaletteColumn { selected: ChordVariation::Triad, ..column }));
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.spelled = (0..DEGREES)
            .flat_map(|c| (0..ROWS as u8).map(move |row| Chord { deck, degree: degree(c), variation: variation_for_row(row) }))
            .map(|chord| {
                let column = project.column(deck, chord.degree);
                let mut spelled = vec![Spelled::new(project, chord, column)];
                let mut modifiers: Vec<u8> = (0..8).collect();
                modifiers.sort_by_key(|mask| (mask.count_ones(), mask & 1));
                for mask in modifiers {
                    let (parallel, sharp3, flat7) = (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0);
                    let scale = if parallel { parallel_flip(project.scale) } else { project.scale };
                    if sharp3 && quality_for(chord.degree, scale) == ChordQuality::Major
                        || flat7 && !degree_intervals(project.root, scale, chord.degree).seventh_is_major
                        || chord.variation == ChordVariation::Dom7 && (sharp3 || flat7)
                    {
                        continue;
                    }
                    let candidate = Spelled::new(project, chord, PaletteColumn { parallel, sharp3, flat7, ..column });
                    if !spelled.iter().any(|existing| existing.classes == candidate.classes) {
                        spelled.push(candidate);
                    }
                }
                spelled
            })
            .collect();
    }

    fn variants(&self, chord: Chord) -> Option<&[Spelled]> {
        self.spelled.get(degree_index(chord.degree) * ROWS as usize + row_of(chord.variation) as usize).map(Vec::as_slice)
    }

    fn get(&self, chord: Chord) -> Option<&Spelled> {
        self.variants(chord)?.first()
    }

    fn heard(&self, chord: Chord, classes: &[u8], alternatives: bool) -> Option<&Spelled> {
        self.variants(chord)?.iter().take(if alternatives { usize::MAX } else { 1 }).find(|spelled| spelled.classes == classes)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Parallel(u8),
    Sharp3(u8),
    Flat7(u8),
    Inversion(u8, i32),
    Octave(u8, i32),
    Split(u8, i32),
    Header(u8),
    Grip(u8),
    Row(u8, u8),
    Reset,
    Big,
    Save,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Up {
    Nothing,
    Changed,
    Big,
    Save,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Session {
    Idle,
    Pill(Hit),
    HeaderPress(u8),
    Arm { chord: Chord, x: i32, y: i32, armed: bool },
    Scrub(u8, u8),
}

pub struct Palette {
    deck: usize,
    w: i32,
    h: i32,
    shift: bool,
    session: Session,
    reset_armed: bool,
    hover: Option<Hit>,
    hits: Vec<(Hit, Rect)>,
    chords: Chords,
}

fn split_edge(total: i32, n: i32, gap: i32, k: i32) -> i32 {
    let inner = (total - (n - 1) * gap).max(0);
    (k as f64 * inner as f64 / n.max(1) as f64).round() as i32 + k * gap
}

fn split_cell(total: i32, n: i32, gap: i32, k: i32) -> (i32, i32) {
    (split_edge(total, n, gap, k), split_edge(total, n, gap, k + 1) - gap)
}

fn degree(column: u8) -> i32 {
    i32::from(column) + 1
}

fn variation_for_row(row: u8) -> ChordVariation {
    if row == ROW_DOM { ChordVariation::Dom7 } else { DISPLAYED_VARIATIONS[row as usize] }
}

fn row_of(variation: ChordVariation) -> u8 {
    if variation == ChordVariation::Dom7 {
        return ROW_DOM;
    }
    DISPLAYED_VARIATIONS.iter().position(|v| *v == variation).unwrap_or(0) as u8
}

fn pitch_classes(notes: &[u8]) -> Vec<u8> {
    let mut classes: Vec<u8> = notes.iter().map(|note| note % 12).collect();
    classes.sort_unstable();
    classes.dedup();
    classes
}

fn tones(classes: &[u8], reference: &[u8]) -> (usize, usize) {
    let shared = classes.iter().filter(|class| reference.contains(class)).count();
    (shared, classes.len() - shared)
}

fn meter(list: &mut DrawList, r: Rect, travel: f32) -> i32 {
    let split = r.x + (r.w as f32 * travel.clamp(0.0, 1.0)).round() as i32;
    list.fill(Rect::new(r.x, r.y, split - r.x, r.h), Tone::Ink100);
    list.frame(r, 1, Tone::Ink100);
    split
}

fn split_text(list: &mut DrawList, r: Rect, text: &str, split: i32, after: Tone, scale: i32) {
    let x = r.x + (r.w - font::width(text, scale)) / 2;
    let y = r.y + (r.h - font::GLYPH_H * scale) / 2;
    list.text_split(x, y, text, split, Tone::Ground, after, scale);
}

fn number(list: &mut DrawList, cell: Rect, line: Rect, places: &[String], split: i32) {
    let Some(first) = places.first() else {
        return;
    };
    let shown = if places.len() > PLACES_SHOWN { format!("{first}…") } else { places.join(" ") };
    let x = cell.x + cell.w - CHUNK_LEAD - font::width(&shown, 1);
    list.text_split(x, line.y + (line.h - font::GLYPH_H) / 2, &shown, split, Tone::Ground, Tone::Ink100, 1);
}

fn places_panel(list: &mut DrawList, x: i32, w: i32, anchor: Rect, h: i32, places: &[String]) {
    let step = font::GLYPH_H + PANEL_LINE_GAP;
    let lines = font::fitted(&places.join(" "), font::glyphs_in(w - 2 * (PANEL_FRAME + PANEL_PAD), 1), ((h - 2 * (PANEL_FRAME + PANEL_PAD) + PANEL_LINE_GAP) / step).max(1) as usize);
    let ph = lines.len() as i32 * step - PANEL_LINE_GAP + 2 * (PANEL_FRAME + PANEL_PAD);
    let below = anchor.y + anchor.h + ROW_GAP;
    let y = if below + ph <= h { below } else { (anchor.y - ROW_GAP - ph).max(0) };
    let panel = Rect::new(x, y, w, ph);
    list.fill(panel, Tone::Ground);
    list.frame(panel, PANEL_FRAME, Tone::Ink100);
    for (i, line) in lines.iter().enumerate() {
        list.text(x + PANEL_FRAME + PANEL_PAD, y + PANEL_FRAME + PANEL_PAD + i as i32 * step, line, Tone::Ink100, 1);
    }
}

fn cell(list: &mut DrawList, r: Rect, label: &str, lit: bool) {
    if lit {
        list.fill(r, Tone::Ink100);
        list.text_centered(r, label, Tone::Ground, 1);
    } else {
        list.frame(r, 1, Tone::Ink25);
        list.text_centered(r, label, Tone::Ink75, 1);
    }
}

impl Palette {
    pub fn new(deck: usize) -> Palette {
        Palette { deck, w: 0, h: 0, shift: false, session: Session::Idle, reset_armed: false, hover: None, hits: Vec::new(), chords: Chords { key: None, spelled: Vec::new() } }
    }

    fn column(&self, project: &Project, c: u8) -> PaletteColumn {
        project.column(self.deck, degree(c))
    }

    fn column_mut<'a>(&self, project: &'a mut Project, c: u8) -> &'a mut PaletteColumn {
        project.column_mut(self.deck, degree(c))
    }

    fn chord(&self, c: u8, row: u8) -> Chord {
        Chord { deck: self.deck, degree: degree(c), variation: variation_for_row(row) }
    }

    fn header(&self, project: &Project, c: u8) -> Chord {
        Chord { deck: self.deck, degree: degree(c), variation: self.column(project, c).selected }
    }

    fn count(&self, project: &Project, c: u8) -> usize {
        self.chords.get(self.header(project, c)).map_or(0, |spelled| spelled.count)
    }

    fn picked(&self, project: &Project, c: u8, aux: i32) -> usize {
        let (bottom, top) = self.column(project, c).picked(self.count(project, c));
        if aux < 0 { bottom } else { top }
    }

    fn shift(&self, project: &Project, c: u8) -> i32 {
        let column = self.column(project, c);
        if column.split(self.count(project, c)) { column.lift } else { column.octave }
    }

    pub fn active(&self) -> bool {
        self.session != Session::Idle
    }

    pub fn set_shift(&mut self, shift: bool) -> bool {
        if self.shift == shift {
            return false;
        }
        self.shift = shift;
        if matches!(self.session, Session::HeaderPress(_) | Session::Scrub(..)) {
            self.session = Session::Idle;
        }
        true
    }

    pub fn lift(&mut self, project: &Project, outside: bool) -> Option<Chord> {
        let chord = match self.session {
            Session::HeaderPress(c) if outside => self.header(project, c),
            Session::Arm { chord, armed: true, .. } => chord,
            _ => return None,
        };
        self.session = Session::Idle;
        Some(chord)
    }

    pub fn playing(&self, project: &Project) -> Option<Chord> {
        match self.session {
            Session::HeaderPress(c) => Some(self.header(project, c)),
            Session::Scrub(c, row) => Some(self.chord(c, row)),
            Session::Idle | Session::Pill(_) | Session::Arm { .. } => None,
        }
    }

    pub fn layout(&mut self, w: i32, h: i32, project: &Project) {
        self.w = w;
        self.h = h;
        self.chords.refresh(project, self.deck);
    }

    fn offered(&self, project: &Project, c: u8) -> impl Iterator<Item = Chord> + '_ {
        std::iter::once(self.header(project, c)).chain((0..ROWS as u8).filter(move |&row| c != DEGREES - 1 || row != ROW_DOM).map(move |row| self.chord(c, row)))
    }

    fn compute(&self, project: &Project, detected: &[Option<PaletteColumn>; DEGREES as usize]) -> Vec<(Hit, Rect)> {
        let (w, h) = (self.w, self.h);
        let mut hits = Vec::new();
        for c in 0..DEGREES {
            let d = degree(c);
            let column = project.column(self.deck, d);
            let (x0, x1) = split_cell(w, 7, COL_GAP, i32::from(c));
            let cw = x1 - x0;
            if cw <= 0 || h <= 0 {
                continue;
            }
            let s3_shown = column.sharp3 || project.sharp3_usable(self.deck, d) || detected[c as usize].is_some_and(|column| column.sharp3);
            let f7_shown = column.flat7 || project.flat7_usable(self.deck, d) || detected[c as usize].is_some_and(|column| column.flat7);
            let alts = i32::from(s3_shown) + i32::from(f7_shown);
            if alts == 0 {
                hits.push((Hit::Parallel(c), Rect::new(x0, 0, cw, PILL_H)));
            } else {
                let (p0, p1) = split_cell(cw, 2, PILL_GAP, 0);
                let (a0, a1) = split_cell(cw, 2, PILL_GAP, 1);
                hits.push((Hit::Parallel(c), Rect::new(x0 + p0, 0, p1 - p0, PILL_H)));
                let aw = a1 - a0;
                let mut slot = 0;
                let mut place = |hit: Hit, hits: &mut Vec<(Hit, Rect)>| {
                    let (s0, s1) = if alts == 1 { (0, aw) } else { split_cell(aw, 2, PILL_GAP, slot) };
                    hits.push((hit, Rect::new(x0 + a0 + s0, 0, s1 - s0, PILL_H)));
                    slot += 1;
                };
                if s3_shown {
                    place(Hit::Sharp3(c), &mut hits);
                }
                if f7_shown {
                    place(Hit::Flat7(c), &mut hits);
                }
            }
            for (i, aux) in [(0, 1), (1, 2)] {
                let (s0, s1) = split_cell(cw, 2, PILL_GAP, i);
                hits.push((Hit::Inversion(c, aux), Rect::new(x0 + s0, INV_Y, s1 - s0, PILL_H)));
            }
            for (i, hit) in [Hit::Octave(c, -1), Hit::Split(c, -1), Hit::Split(c, 1), Hit::Octave(c, 1)].into_iter().enumerate() {
                let (s0, s1) = split_cell(cw, 4, PILL_GAP, i as i32);
                hits.push((hit, Rect::new(x0 + s0, OCT_Y, s1 - s0, PILL_H)));
            }
            let rail = if self.shift { 0 } else { GRIP_W.min(cw / 2) };
            hits.push((Hit::Header(c), Rect::new(x0 + rail, HEADER_Y, (cw - 2 * rail).max(0), TILE_HEADER_H)));
            if rail > 0 {
                hits.push((Hit::Grip(c), Rect::new(x0, HEADER_Y, rail, TILE_HEADER_H)));
                hits.push((Hit::Grip(c), Rect::new(x1 - rail, HEADER_Y, rail, TILE_HEADER_H)));
            }
            let rows_total = (h - ROWS_Y).max(0);
            for r in 0..ROWS {
                let (t0, t1) = split_cell(rows_total, ROWS, ROW_GAP, r);
                let row = Rect::new(x0, ROWS_Y + t0, cw, (t1 - t0).max(0));
                if c == DEGREES - 1 && r == i32::from(ROW_DOM) {
                    for (k, hit) in [Hit::Reset, Hit::Big, Hit::Save].into_iter().enumerate() {
                        let (s0, s1) = split_cell(cw, 3, PILL_GAP, k as i32);
                        hits.push((hit, Rect::new(x0 + s0, row.y, s1 - s0, row.h)));
                    }
                } else {
                    hits.push((Hit::Row(c, r as u8), row));
                }
            }
        }
        hits
    }

    pub fn draw(&mut self, list: &mut DrawList, project: &Project, latched: Option<Chord>, sounding: Option<&Sounding>, progression: &[ClipChord], big: bool) {
        list.fill(Rect::new(0, 0, self.w, self.h), Tone::Ground);
        let sequence: Vec<Vec<u8>> = progression.iter().map(|chord| pitch_classes(&chord.pitches)).collect();
        let places = |chord: Chord| -> Vec<String> { sequence.iter().enumerate().filter(|(_, classes)| self.chords.heard(chord, classes, false).is_some()).map(|(index, _)| (index + 1).to_string()).collect() };
        let pressed = match self.session {
            Session::Pill(hit) => Some(hit),
            _ => None,
        };
        let anchor = self.playing(project).or(latched);
        let heard = sounding.map(|sounding| (pitch_classes(&sounding.pitches), sounding.travel));
        let reference = anchor.map(|chord| pitch_classes(&project.chord_notes(chord))).or_else(|| heard.as_ref().map(|(classes, _)| classes.clone()));
        let alternatives = anchor.is_none() && heard.as_ref().is_some_and(|(classes, _)| !(0..DEGREES).flat_map(|c| self.offered(project, c)).any(|chord| self.chords.get(chord).is_some_and(|spelled| spelled.classes == *classes)));
        let detected = std::array::from_fn(|c| {
            let (classes, _) = heard.as_ref().filter(|_| alternatives)?;
            self.offered(project, c as u8).find_map(|chord| self.chords.heard(chord, classes, true)).map(|spelled| spelled.column)
        });
        self.hits = self.compute(project, &detected);
        let column = |c: u8| detected[c as usize].unwrap_or_else(|| self.column(project, c));
        let filled = |lit: bool, chord: Chord| {
            if lit {
                self.chords.get(chord).map(|spelled| (spelled, 1.0))
            } else {
                heard.as_ref().and_then(|(classes, travel)| {
                    self.chords.heard(chord, classes, alternatives)
                        .filter(|spelled| {
                            let shown = column(degree_index(chord.degree) as u8);
                            (spelled.column.parallel, spelled.column.sharp3, spelled.column.flat7) == (shown.parallel, shown.sharp3, shown.flat7)
                        })
                        .map(|spelled| (spelled, *travel))
                })
            }
        };
        for &(hit, r) in &self.hits {
            let is_pressed = pressed == Some(hit);
            match hit {
                Hit::Parallel(c) => cell(list, r, "parallel", column(c).parallel || is_pressed),
                Hit::Sharp3(c) => cell(list, r, "♯3", column(c).sharp3 || is_pressed),
                Hit::Flat7(c) => cell(list, r, "♭7", column(c).flat7 || is_pressed),
                Hit::Inversion(c, aux) => cell(list, r, if aux == 1 { "1st" } else { "2nd" }, self.column(project, c).bass_offset == aux || is_pressed),
                Hit::Split(c, aux) => {
                    let picked = self.picked(project, c, aux);
                    cell(list, r, &picked.to_string(), picked > 0 || is_pressed);
                }
                Hit::Octave(c, aux) => {
                    let octave = self.shift(project, c);
                    let on_side = octave * aux > 0;
                    cell(list, r, &format!("{}{}", if aux < 0 { "−" } else { "+" }, if on_side { octave.abs() } else { 1 }), on_side || is_pressed);
                }
                Hit::Header(c) => {
                    let chord = self.header(project, c);
                    let Some(spelled) = self.chords.get(chord) else {
                        continue;
                    };
                    let lit = self.session == Session::HeaderPress(c) || latched == Some(chord);
                    let (x0, x1) = split_cell(self.w, 7, COL_GAP, i32::from(c));
                    let band = Rect::new(x0, r.y, x1 - x0, r.h);
                    let ty = r.y + (r.h - (3 * font::GLYPH_H + TILE_GAP)) / 2;
                    let roman = Rect::new(band.x, ty, band.w, 2 * font::GLYPH_H);
                    let label = Rect::new(band.x, ty + 2 * font::GLYPH_H + TILE_GAP, band.w, font::GLYPH_H);
                    let split = match filled(lit, chord) {
                        Some((playing, travel)) => {
                            let split = meter(list, band, travel);
                            split_text(list, roman, &playing.roman, split, Tone::Ink100, 2);
                            split_text(list, label, &playing.label_in(label.w), split, Tone::Ink75, 1);
                            split
                        }
                        None => {
                            list.frame(band, 1, Tone::Ink25);
                            list.text_centered(roman, &spelled.roman, Tone::Ink100, 2);
                            list.text_centered(label, &spelled.label_in(label.w), Tone::Ink75, 1);
                            band.x
                        }
                    };
                    number(list, band, roman, &places(chord), split);
                }
                Hit::Grip(_) => {
                    let gx = r.x + (r.w - 6) / 2;
                    let gy = r.y + (r.h - 10) / 2;
                    for row in 0..3 {
                        for col in 0..2 {
                            list.fill(Rect::new(gx + col * 4, gy + row * 4, 2, 2), Tone::Ink50);
                        }
                    }
                }
                Hit::Row(c, row) => {
                    let chord = self.chord(c, row);
                    let Some(spelled) = self.chords.get(chord) else {
                        continue;
                    };
                    let lit = self.session == Session::Scrub(c, row) || latched == Some(chord);
                    let split = match filled(lit, chord) {
                        Some((playing, travel)) => {
                            let split = meter(list, r, travel);
                            split_text(list, r, &playing.label_in(r.w), split, Tone::Ink100, 1);
                            split
                        }
                        None => {
                            let selected = row_of(self.column(project, c).selected) == row;
                            list.frame(r, if selected { 2 } else { 1 }, if selected { Tone::Ink100 } else { Tone::Ink25 });
                            list.text_centered(r, &spelled.label_in(r.w), if selected { Tone::Ink100 } else { Tone::Ink75 }, 1);
                            if let Some(reference) = reference.as_deref().filter(|_| anchor != Some(chord)) {
                                let (shared, fresh) = tones(&spelled.classes, reference);
                                for i in 0..(shared + fresh).min(MAX_CHUNKS) {
                                    let tone = if i < shared { Tone::Ink75 } else { Tone::Ink25 };
                                    list.fill(Rect::new(r.x + CHUNK_LEAD + i as i32 * (CHUNK_W + CHUNK_GAP), r.y + (r.h - CHUNK_H) / 2, CHUNK_W, CHUNK_H), tone);
                                }
                            }
                            r.x
                        }
                    };
                    number(list, r, r, &places(chord), split);
                }
                Hit::Reset => cell(list, r, if self.reset_armed { "reset?" } else { "reset" }, self.reset_armed || is_pressed),
                Hit::Big => cell(list, r, "big", big || is_pressed),
                Hit::Save => cell(list, r, "save", is_pressed),
            }
        }
        let hovered = match self.hover {
            Some(Hit::Header(c)) => Some((c, self.header(project, c))),
            Some(Hit::Row(c, row)) => Some((c, self.chord(c, row))),
            _ => None,
        };
        if let Some((c, chord)) = hovered && let Some(&(_, anchor)) = self.hits.iter().find(|(hit, _)| Some(*hit) == self.hover) {
            let places = places(chord);
            if places.len() > PLACES_SHOWN {
                let (x0, x1) = split_cell(self.w, 7, COL_GAP, i32::from(c));
                places_panel(list, x0, x1 - x0, anchor, self.h, &places);
            }
        }
    }

    pub fn hover(&mut self, point: Option<(i32, i32)>) -> bool {
        let hit = point.and_then(|(x, y)| self.hit(x, y)).filter(|hit| matches!(hit, Hit::Header(_) | Hit::Row(..)));
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        self.hits.iter().find(|(_, r)| r.contains(x, y)).map(|(hit, _)| *hit)
    }

    fn hit_row(&self, x: i32, y: i32) -> Option<(u8, u8)> {
        self.hits.iter().find_map(|(hit, r)| match hit {
            Hit::Row(c, row) if r.contains(x, y) => Some((*c, *row)),
            _ => None,
        })
    }

    pub fn down(&mut self, x: i32, y: i32, project: &mut Project) -> bool {
        let Some(hit) = self.hit(x, y) else {
            return std::mem::replace(&mut self.reset_armed, false);
        };
        if hit != Hit::Reset {
            self.reset_armed = false;
        }
        let arm = |chord: Chord| Session::Arm { chord, x, y, armed: false };
        self.session = match hit {
            Hit::Row(c, row) if self.shift => arm(self.chord(c, row)),
            Hit::Row(c, row) => {
                self.column_mut(project, c).selected = variation_for_row(row);
                Session::Scrub(c, row)
            }
            Hit::Header(c) if self.shift => arm(self.header(project, c)),
            Hit::Header(c) => Session::HeaderPress(c),
            Hit::Grip(c) => arm(self.header(project, c)),
            other => Session::Pill(other),
        };
        true
    }

    pub fn moved(&mut self, x: i32, y: i32, project: &mut Project) -> bool {
        if let Session::Arm { x: x0, y: y0, armed, .. } = &mut self.session {
            *armed |= (x - *x0).abs() + (y - *y0).abs() > DRAG_START;
            return false;
        }
        if let Session::Scrub(c, row) = self.session && let Some((nc, nr)) = self.hit_row(x, y) && (nc, nr) != (c, row) {
            self.column_mut(project, nc).selected = variation_for_row(nr);
            self.session = Session::Scrub(nc, nr);
            return true;
        }
        false
    }

    pub fn up(&mut self, x: i32, y: i32, project: &mut Project) -> Up {
        match std::mem::replace(&mut self.session, Session::Idle) {
            Session::Scrub(..) | Session::HeaderPress(_) => Up::Changed,
            Session::Pill(hit) if self.hit(x, y) == Some(hit) => self.fire(hit, project),
            Session::Pill(_) => Up::Changed,
            Session::Arm { .. } | Session::Idle => Up::Nothing,
        }
    }

    fn fire(&mut self, hit: Hit, project: &mut Project) -> Up {
        match hit {
            Hit::Parallel(c) => {
                let column = self.column_mut(project, c);
                column.parallel = !column.parallel;
                project.drop_inert_alterations(self.deck, degree(c));
            }
            Hit::Sharp3(c) => self.column_mut(project, c).sharp3 ^= true,
            Hit::Flat7(c) => self.column_mut(project, c).flat7 ^= true,
            Hit::Inversion(c, aux) => {
                let column = self.column_mut(project, c);
                column.bass_offset = if column.bass_offset == aux { 0 } else { aux };
            }
            Hit::Split(c, aux) => {
                let picked = (self.picked(project, c, aux) + 1) % self.count(project, c).max(1);
                let column = self.column_mut(project, c);
                *(if aux < 0 { &mut column.bottom } else { &mut column.top }) = picked as i32;
            }
            Hit::Octave(c, aux) => {
                let split = self.column(project, c).split(self.count(project, c));
                let column = self.column_mut(project, c);
                let value = if split { &mut column.lift } else { &mut column.octave };
                let steps = *value * aux;
                *value = if steps == PaletteColumn::OCTAVE_REACH { 0 } else { aux * (steps.max(0) + 1) };
            }
            Hit::Reset => {
                if self.reset_armed {
                    project.decks[self.deck] = [PaletteColumn::DEFAULT; 7];
                }
                self.reset_armed = !self.reset_armed;
            }
            Hit::Big => return Up::Big,
            Hit::Save => return Up::Save,
            Hit::Header(_) | Hit::Grip(_) | Hit::Row(..) => {}
        }
        Up::Changed
    }
}
