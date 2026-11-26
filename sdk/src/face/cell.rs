use super::*;
use crate::json::Json;
use crate::param::{joined, knobs};
use Format::*;
use Kind::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Rise,
    Centre,
    Pan,
    Toggle,
    Switch,
    Pick,
    Button(Act),
    Page(usize),
    Open(usize, usize),
    Title,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Flatten,
    Match,
}

pub struct Menu<P: 'static> {
    pub get: fn(&P) -> f64,
    pub set: fn(&mut P, f64),
    pub options: &'static [&'static str],
    pub offered: fn(&P, usize) -> bool,
}

impl<P> Menu<P> {
    pub const fn of(field: Field<P>, options: &'static [&'static str], offered: fn(&P, usize) -> bool) -> Menu<P> {
        Menu { get: field.get, set: field.set, options, offered }
    }
}

impl<P> Clone for Menu<P> {
    fn clone(&self) -> Menu<P> {
        *self
    }
}

impl<P> Copy for Menu<P> {}

pub struct Cell<P: 'static> {
    pub row: i32,
    pub column: i32,
    pub span: i32,
    pub kind: Kind,
    pub label: &'static str,
    pub named: Option<fn(&P) -> &'static str>,
    pub menu: Option<Menu<P>>,
    pub param: Param<P>,
    pub format: Format,
    pub options: &'static [&'static str],
    pub inert: fn(&P) -> bool,
    pub refuses: fn(&P) -> bool,
    pub icon: Option<Icon>,
    pub tempo: Option<fn(&mut P, f64)>,
    pub desc: &'static str,
    pub page: Option<usize>,
    pub pages: usize,
    pub held: bool,
    pub tray: Option<usize>,
}

impl<P> Clone for Cell<P> {
    fn clone(&self) -> Cell<P> {
        *self
    }
}

impl<P> Copy for Cell<P> {}

impl<P> Cell<P> {
    pub(super) fn icon_w(&self) -> i32 {
        if self.icon.is_some() { ICON_W + OPTION_PAD } else { 0 }
    }

    pub(super) fn label_of(&self, patch: &P) -> &'static str {
        self.named.map_or(self.label, |named| named(patch))
    }

    pub(super) fn name_of(&self, patch: &P) -> &'static str {
        self.named.map_or(self.param.name, |named| named(patch))
    }
}

pub fn never<P>(_: &P) -> bool {
    false
}

pub const fn value<P>(kind: Kind, row: i32, column: i32, label: &'static str, param: Param<P>, format: Format) -> Cell<P> {
    Cell { row, column, span: 1, kind, label, named: None, menu: None, param, format, options: &[], inert: never, refuses: never, icon: None, tempo: None, desc: "", page: None, pages: 1, held: false, tray: None }
}

pub const fn toggle<P>(row: i32, column: i32, label: &'static str, band: &'static str, param: &'static str, field: Field<P>) -> Cell<P> {
    Cell { row, column, span: 1, kind: Kind::Toggle, label, named: None, menu: None, param: Param::new(band, param, field, UNIT), format: Format::Percent, options: &[], inert: never, refuses: never, icon: None, tempo: None, desc: "", page: None, pages: 1, held: false, tray: None }
}

pub const fn switch<P>(row: i32, column: i32, words: &'static [&'static str], band: &'static str, param: &'static str, field: Field<P>) -> Cell<P> {
    Cell { row, column, span: 1, kind: Kind::Switch, label: "", named: None, menu: None, param: Param::new(band, param, field, linear(0.0, last_word(words))), format: Format::Percent, options: words, inert: never, refuses: never, icon: None, tempo: None, desc: "", page: None, pages: 1, held: false, tray: None }
}

const fn last_word(words: &[&str]) -> f64 {
    if words.len() > 1 { (words.len() - 1) as f64 } else { 1.0 }
}

pub const fn pick<P>(row: i32, column: i32, span: i32, band: &'static str, param: &'static str, field: Field<P>, options: &'static [&'static str]) -> Cell<P> {
    Cell { row, column, span, kind: Kind::Pick, label: "", named: None, menu: None, param: Param::new(band, param, field, UNIT), format: Format::Integer, options, inert: never, refuses: never, icon: None, tempo: None, desc: "", page: None, pages: 1, held: false, tray: None }
}

pub const fn button<P>(row: i32, column: i32, label: &'static str, band: &'static str, param: &'static str, act: Act) -> Cell<P> {
    Cell { row, column, span: 1, kind: Kind::Button(act), label, named: None, menu: None, param: Param::new(band, param, field!(zero, ignore), UNIT), format: Format::Percent, options: &[], inert: never, refuses: never, icon: None, tempo: None, desc: "", page: None, pages: 1, held: false, tray: None }
}

pub const fn page<P>(row: i32, column: i32, label: &'static str, band: &'static str, param: &'static str, index: usize) -> Cell<P> {
    Cell { row, column, span: 1, kind: Kind::Page(index), label, named: None, menu: None, param: Param::new(band, param, field!(zero, ignore), UNIT), format: Format::Percent, options: &[], inert: never, refuses: never, icon: None, tempo: None, desc: "", page: None, pages: 1, held: false, tray: None }
}

pub const fn open<P>(row: i32, column: i32, label: &'static str, band: &'static str, param: &'static str, first: usize, count: usize) -> Cell<P> {
    Cell { kind: Kind::Open(first, count), ..page(row, column, label, band, param, 0) }
}

pub const fn title<P>(row: i32, column: i32, label: &'static str) -> Cell<P> {
    Cell { kind: Kind::Title, ..page(row, column, label, "", "", 0) }
}

pub const fn desc<P>(cell: Cell<P>, desc: &'static str) -> Cell<P> {
    Cell { desc, ..cell }
}

pub const fn seeded<P>(cell: Cell<P>, band: &str, name: &str) -> Cell<P> {
    Cell { param: cell.param.seeded(band, name), ..cell }
}

pub const fn saved<P>(cell: Cell<P>, path: &'static str) -> Cell<P> {
    Cell { param: cell.param.saved(path), ..cell }
}

pub const fn named<P>(cell: Cell<P>, named: fn(&P) -> &'static str) -> Cell<P> {
    Cell { named: Some(named), ..cell }
}

pub const fn menu<P>(cell: Cell<P>, menu: Menu<P>) -> Cell<P> {
    Cell { menu: Some(menu), ..cell }
}

pub const fn unless<P>(cell: Cell<P>, off: fn(&P) -> bool) -> Cell<P> {
    Cell { inert: off, refuses: off, ..cell }
}

pub const fn inert<P>(cell: Cell<P>, inert: fn(&P) -> bool) -> Cell<P> {
    Cell { inert, ..cell }
}

pub const fn refuses<P>(cell: Cell<P>, refuses: fn(&P) -> bool) -> Cell<P> {
    Cell { refuses, ..cell }
}

pub const fn icon<P>(cell: Cell<P>, icon: Icon) -> Cell<P> {
    Cell { icon: Some(icon), ..cell }
}

pub const fn tempo<P>(cell: Cell<P>, tempo: fn(&mut P, f64)) -> Cell<P> {
    Cell { tempo: Some(tempo), ..cell }
}

pub const fn paged<P>(cell: Cell<P>, page: usize) -> Cell<P> {
    Cell { page: Some(page), ..cell }
}

pub const fn wide<P>(cell: Cell<P>, span: i32) -> Cell<P> {
    Cell { span, ..cell }
}

pub const fn held<P>(cell: Cell<P>) -> Cell<P> {
    Cell { held: true, ..cell }
}

pub const fn viewed<P, const N: usize>(mut cells: [Cell<P>; N], page: usize) -> [Cell<P>; N] {
    let mut i = 0;
    while i < N {
        cells[i].page = Some(page);
        i += 1;
    }
    cells
}

pub const fn trayed<P, const N: usize>(mut cells: [Cell<P>; N], tray: usize) -> [Cell<P>; N] {
    let mut i = 0;
    while i < N {
        cells[i].tray = Some(tray);
        i += 1;
    }
    cells
}

pub const fn widened<P, const N: usize>(mut cells: [Cell<P>; N], span: i32) -> [Cell<P>; N] {
    let mut i = 0;
    while i < N {
        cells[i].column *= span;
        cells[i].span = span;
        i += 1;
    }
    cells
}

pub const fn spanned<P, const N: usize>(mut cells: [Cell<P>; N], page: usize, pages: usize) -> [Cell<P>; N] {
    let mut i = 0;
    while i < N {
        cells[i].page = Some(page);
        cells[i].pages = pages;
        i += 1;
    }
    cells
}

fn words<P>(cell: &Cell<P>) -> f64 {
    cell.options.len().max(2) as f64
}

pub(super) fn next_word<P>(cell: &Cell<P>, value: f64) -> f64 {
    (value.round() + 1.0) % words(cell)
}

pub(super) fn stepped_word<P>(cell: &Cell<P>, value: f64, direction: i32) -> f64 {
    (value.round() + f64::from(direction)).clamp(0.0, words(cell) - 1.0)
}

pub(super) fn word<P>(cell: &Cell<P>, value: f64) -> &'static str {
    match cell.kind {
        Switch => choice(cell.options, value),
        _ => OFF_ON[usize::from(value >= 0.5)],
    }
}

pub const fn flat<P, const N: usize>(parts: &[&[Cell<P>]]) -> [Cell<P>; N] {
    joined(parts, value(Rise, 0, 0, "", Param::new("", "", field!(zero, ignore), UNIT), Percent))
}

pub const EMPTY: [Cell<()>; 0] = [];

pub fn armable<P>(cell: &Cell<P>) -> bool {
    !cell.held && matches!(cell.kind, Rise | Centre | Pan | Toggle | Switch)
}

fn switching<P>(cell: &Cell<P>) -> bool {
    !cell.held && matches!(cell.kind, Toggle | Switch)
}

pub fn holds<P>(cells: &[Cell<P>], band: &str, param: &str) -> bool {
    cells.iter().any(|cell| cell.held && cell.param.band == band && cell.param.name == param)
}

pub fn automatable<'a, P>(cells: &'a [Cell<P>], band: &str, param: &str) -> Option<&'a Cell<P>> {
    cells.iter().find(|cell| armable(cell) && cell.param.band == band && cell.param.name == param)
}

pub fn switched<'a, P>(cells: &'a [Cell<P>], band: &str, param: &str) -> Option<&'a Cell<P>> {
    cells.iter().find(|cell| switching(cell) && cell.param.band == band && cell.param.name == param)
}

fn earlier_named<P>(cells: &[Cell<P>], index: usize, among: fn(&Cell<P>) -> bool) -> bool {
    let cell = &cells[index];
    cells[..index].iter().any(|earlier| among(earlier) && earlier.param.band == cell.param.band && earlier.param.name == cell.param.name)
}

fn knob<P>(cell: &Cell<P>) -> bool {
    matches!(cell.kind, Rise | Centre | Pan | Toggle | Switch | Pick)
}

pub fn switches<P>(cells: &[Cell<P>]) -> impl Iterator<Item = Param<P>> + '_ {
    (0..cells.len()).filter(|&index| switching(&cells[index]) && !earlier_named(cells, index, switching)).map(|index| cells[index].param)
}

pub fn described<P: Copy>(cells: &[Cell<P>], default: P, spell: impl Fn(&P, &P) -> Json) -> Vec<(String, String)> {
    let params: Vec<Param<P>> = (0..cells.len()).filter(|&index| knob(&cells[index]) && !earlier_named(cells, index, knob)).map(|index| cells[index].param).collect();
    let found = knobs(&params, default, spell).into_iter().filter(|knob| !knob.path.contains('[')).filter_map(|knob| {
        let cell = cells.iter().find(|cell| cell.param.band == knob.band && cell.param.name == knob.name)?;
        let text = if cell.desc.is_empty() { format!("{} {}", cell.param.band, cell.param.name) } else { String::from(cell.desc) };
        Some((knob.path, text))
    });
    found.collect()
}

pub fn description(described: &[(String, String)], path: &str) -> String {
    described.iter().find(|(known, _)| known == path).map_or_else(String::new, |(_, text)| text.clone())
}

pub fn value_cell_at<P>(cells: &[Cell<P>], rows: i32, x: i32, y: i32, view: View) -> Option<&Cell<P>> {
    cells.iter().find(|cell| seen(cell, view) && matches!(cell.kind, Rise | Centre | Pan) && cell_rect(cell, rows).contains(x, y))
}

pub(super) fn live_at<P>(cells: &[Cell<P>], patch: &P, view: View, index: usize) -> Option<usize> {
    let own = cells.get(index)?;
    let slot = slot_of(own);
    let at = |cell: &Cell<P>| seen(cell, view) && slot_of(cell) == slot;
    cells.iter().position(|cell| at(cell) && !(cell.refuses)(patch)).or_else(|| at(own).then_some(index))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct View {
    pub page: usize,
    pub tray: Option<usize>,
}

pub fn on_page<P>(cell: &Cell<P>, page: usize) -> bool {
    cell.page.is_none_or(|own| page >= own && page < own + cell.pages)
}

pub fn seen<P>(cell: &Cell<P>, view: View) -> bool {
    on_page(cell, view.page) && (cell.row >= 0 || (cell.tray.is_some() && cell.tray == view.tray))
}

pub fn opener<P>(cells: &[Cell<P>], tray: usize) -> Option<usize> {
    cells.iter().position(|cell| cell.tray == Some(tray) && matches!(cell.kind, Rise | Centre | Pan))
}

pub fn pickable<P>(cell: &Cell<P>) -> bool {
    !matches!(cell.kind, Button(_) | Page(_) | Open(..) | Title)
}

pub fn index_of<P>(cells: &[Cell<P>], band: &str, param: &str) -> Option<usize> {
    cells.iter().position(|cell| cell.param.band == band && cell.param.name == param)
}

pub fn index_at<P>(cells: &[Cell<P>], patch: &P, rows: i32, x: i32, y: i32, view: View) -> Option<usize> {
    let index = cells.iter().position(|cell| seen(cell, view) && pickable(cell) && cell_rect(cell, rows).contains(x, y))?;
    live_at(cells, patch, view, index)
}

pub fn clamped<P>(cell: &Cell<P>, value: f64) -> f64 {
    let (low, high) = (cell.param.scale.value(0.0), cell.param.scale.value(1.0));
    value.clamp(low.min(high), low.max(high))
}

pub fn assign<P>(cell: &Cell<P>, patch: &mut P, value: f64) {
    let value = match cell.kind {
        Rise | Centre | Pan => clamped(cell, value),
        Toggle | Switch => value.round().clamp(0.0, words(cell) - 1.0),
        Pick => {
            if cell.options.is_empty() {
                value.round().max(0.0)
            } else {
                value.round().clamp(0.0, (cell.options.len() - 1) as f64)
            }
        }
        Button(_) | Page(_) | Open(..) | Title => return,
    };
    (cell.param.set)(patch, value);
}

pub fn restore<P: Patch>(cell: &Cell<P>, patch: &mut P) {
    (cell.param.set)(patch, (cell.param.get)(&patch.factory()));
}

pub fn setting_at<P>(cells: &[Cell<P>], patch: &P, index: usize) -> Option<(&'static str, &'static str, f64)> {
    cells.get(index).filter(|cell| pickable(cell)).map(|cell| (cell.param.band, cell.param.name, (cell.param.get)(patch)))
}

pub fn assign_named<P>(cells: &[Cell<P>], patch: &mut P, band: &str, param: &str, value: f64) -> Option<usize> {
    let index = index_of(cells, band, param)?;
    assign(&cells[index], patch, value);
    Some(index)
}

pub fn restore_at<P: Patch>(cells: &[Cell<P>], patch: &mut P, index: usize) -> Option<(&'static str, &'static str)> {
    let cell = cells.get(index).filter(|cell| pickable(cell))?;
    restore(cell, patch);
    Some((cell.param.band, cell.param.name))
}

pub fn armable_cell<P>(cells: &[Cell<P>], index: usize) -> Option<(&'static str, &'static str)> {
    cells.get(index).filter(|cell| armable(cell)).map(|cell| (cell.param.band, cell.param.name))
}

pub fn armable_at<P>(cells: &[Cell<P>], patch: &P, rows: i32, x: i32, y: i32, view: View) -> Option<(&'static str, &'static str)> {
    let cell = cells.iter().filter(|cell| seen(cell, view) && armable(cell) && cell_rect(cell, rows).contains(x, y)).find(|cell| !(cell.refuses)(patch))?;
    Some((cell.param.band, cell.param.name))
}

pub fn automatable_of<P>(cells: &[Cell<P>], band: &str, param: &str) -> Option<(usize, &'static str, &'static str)> {
    let cell = automatable(cells, band, param)?;
    index_of(cells, cell.param.band, cell.param.name).map(|index| (index, cell.param.band, cell.param.name))
}

pub fn spelling<P>(cells: &[Cell<P>], band: &str, param: &str, travel: f64) -> Option<(String, &'static str)> {
    automatable(cells, band, param).map(|cell| (spelled_value(cell, cell.param.scale.value(travel)), cell.desc))
}

pub fn automated_rect<P>(cells: &[Cell<P>], rows: i32, band: &str, param: &str, view: View) -> Option<Rect> {
    cells.iter().find(|cell| armable(cell) && seen(cell, view) && cell.param.band == band && cell.param.name == param).map(|cell| cell_rect(cell, rows))
}

pub fn spelled<P>(cell: &Cell<P>, patch: &P) -> String {
    spelled_value(cell, (cell.param.get)(patch))
}

fn spelled_value<P>(cell: &Cell<P>, value: f64) -> String {
    match cell.kind {
        Rise | Centre | Pan => spell(cell.format, value),
        Toggle | Switch => String::from(word(cell, value)),
        Pick => String::from(cell.options.get(value.round() as usize).copied().unwrap_or("")),
        Button(_) | Page(_) | Open(..) | Title => String::from(cell.label),
    }
}

const UNITS: [f64; 7] = [0.001, 0.01, 0.1, 1.0, 10.0, 100.0, 1000.0];

fn unit(format: Format, value: f64, direction: f64) -> f64 {
    let shown = spell(format, value);
    UNITS.into_iter().find(|unit| spell(format, value + direction * unit) != shown).unwrap_or(1.0)
}

fn places(format: Format, coarse: bool) -> f64 {
    match (coarse, format) {
        (false, _) => 1.0,
        (true, Semitones) => 12.0,
        (true, _) => 10.0,
    }
}

pub(super) fn turn<P>(cell: &Cell<P>, patch: &mut P, direction: i32, coarse: bool) {
    let direction = f64::from(direction);
    let was = (cell.param.get)(patch);
    let shown = spell(cell.format, was);
    let unit = unit(cell.format, was, direction);
    let places = places(cell.format, coarse);
    let mut value = was;
    loop {
        value = ((value / unit).round() + direction * places) * unit;
        let before = (cell.param.get)(patch);
        (cell.param.set)(patch, cell.param.scale.value(cell.param.scale.travel(value)));
        let now = (cell.param.get)(patch);
        if now == before || spell(cell.format, now) != shown {
            return;
        }
    }
}
