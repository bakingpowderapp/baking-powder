use crate::draw::{DrawList, Rect, Tone};
use crate::font;
use crate::keyboard::Key;
pub use crate::param::{Field, Param, UNIT, choice, ignore, linear, log, zero};
pub use crate::param::{choose, field, flag};
use crate::param::{Patch, release_ms};

mod cell;
mod drawn;
mod paint;
mod sketch;
mod spell;

pub use cell::*;
pub use drawn::*;
pub use paint::*;
pub use sketch::*;
pub use spell::*;
use Kind::*;

pub const FRAME: i32 = 2;
pub const W: i32 = COLUMNS * PITCH + FRAME;
pub const H: i32 = ROWS * PITCH + FRAME;
pub const CELL: i32 = 33;
pub const PITCH: i32 = CELL - FRAME;
pub const COLUMNS: i32 = 16;
pub const ROWS: i32 = 15;
pub const READOUT_ROWS: i32 = 7;
pub const NO_READOUT: i32 = 0;
pub const INTERIOR: i32 = CELL - 2 * FRAME;
pub const LABEL_SCALE: i32 = 1;
const CENTRE: i32 = INTERIOR / 2;
pub const THROW: f64 = 150.0;
const EPSILON: f64 = 1e-4;
const READOUT_PAD: i32 = 10;
pub const READOUT_SCALE: i32 = 2;
const LEADING: i32 = 6;
const READOUT_LINE: i32 = font::GLYPH_H * READOUT_SCALE + LEADING;
const HINT_LINE: i32 = font::GLYPH_H * LABEL_SCALE + LEADING;
pub const HINT_LINES: usize = 2;
pub const VALUE_SCALE: i32 = 4;
const OPTION_H: i32 = 14;
const OPTION_PAD: i32 = 4;
const PANEL_MIN_W: i32 = 48;
pub const ICON_STEPS: i32 = 8;
pub const ICON_NOTES: i32 = 4;
pub const ICON_PITCH: i32 = 2;
const ICON_W: i32 = (ICON_STEPS - 1) * ICON_PITCH + 1;
const ICON_H: i32 = (ICON_NOTES - 1) * ICON_PITCH + 1;
const CHEVRON_W: i32 = 3;
const CHEVRON_H: i32 = 5;
const CHEVRON_INSET: i32 = 2;
pub const WHEEL_PX: i32 = 12;
const ARM: i32 = 4;
pub const DOT: i32 = 5;
pub const DOT_INSET: i32 = 2;
const ENTRY_GLYPHS: usize = font::glyphs_in(W - 2 * FRAME - 2 * READOUT_PAD, VALUE_SCALE);
const TYPE_HINT: &str = "Type a value, Return sets it, Escape leaves";
const UNREAD_HINT: &str = "Can't read that, Escape leaves";

pub type Icon = fn(&mut DrawList, usize, i32, i32, Tone);

pub const OFF_ON: [&str; 2] = ["OFF", "ON"];

pub trait Faced {
    fn readout_rows(&self) -> i32;

    fn hover(&self, face: &mut Face, point: Option<(i32, i32)>) -> bool;

    fn down(&mut self, face: &mut Face, x: i32, y: i32, bpm: f64) -> bool;

    fn moved(&mut self, face: &mut Face, x: i32, y: i32) -> bool;

    fn wheel(&mut self, face: &mut Face, x: i32, y: i32, dy: i32) -> bool;

    fn up(&mut self, face: &mut Face, x: i32, y: i32) -> bool;

    fn begin(&self, face: &mut Face, first: Option<char>) -> bool;

    fn key(&mut self, face: &mut Face, key: Key, command: bool, bpm: f64) -> bool;

    fn nudge(&mut self, face: &mut Face, direction: i32, coarse: bool, bpm: f64) -> bool;

    fn index_at(&self, x: i32, y: i32, view: View) -> Option<usize>;

    fn setting_of(&self, cell: usize) -> Option<(&'static str, &'static str, f64)>;

    fn assign(&mut self, band: &str, param: &str, value: f64) -> Option<usize>;

    fn restore(&mut self, cell: usize) -> Option<(&'static str, &'static str)>;

    fn armable(&self, cell: usize) -> Option<(&'static str, &'static str)>;

    fn armable_at(&self, x: i32, y: i32, view: View) -> Option<(&'static str, &'static str)>;

    fn automatable_of(&self, band: &str, param: &str) -> Option<(usize, &'static str, &'static str)>;

    fn spelling(&self, band: &str, param: &str, travel: f64) -> Option<(String, &'static str)>;

    fn automated_rect(&self, band: &str, param: &str, view: View) -> Option<Rect>;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Blank;

impl Faced for Blank {
    fn readout_rows(&self) -> i32 {
        READOUT_ROWS
    }

    fn hover(&self, _: &mut Face, _: Option<(i32, i32)>) -> bool {
        false
    }

    fn down(&mut self, _: &mut Face, _: i32, _: i32, _: f64) -> bool {
        false
    }

    fn moved(&mut self, _: &mut Face, _: i32, _: i32) -> bool {
        false
    }

    fn wheel(&mut self, _: &mut Face, _: i32, _: i32, _: i32) -> bool {
        false
    }

    fn up(&mut self, _: &mut Face, _: i32, _: i32) -> bool {
        false
    }

    fn begin(&self, _: &mut Face, _: Option<char>) -> bool {
        false
    }

    fn key(&mut self, _: &mut Face, _: Key, _: bool, _: f64) -> bool {
        false
    }

    fn nudge(&mut self, _: &mut Face, _: i32, _: bool, _: f64) -> bool {
        false
    }

    fn index_at(&self, _: i32, _: i32, _: View) -> Option<usize> {
        None
    }

    fn setting_of(&self, _: usize) -> Option<(&'static str, &'static str, f64)> {
        None
    }

    fn assign(&mut self, _: &str, _: &str, _: f64) -> Option<usize> {
        None
    }

    fn restore(&mut self, _: usize) -> Option<(&'static str, &'static str)> {
        None
    }

    fn armable(&self, _: usize) -> Option<(&'static str, &'static str)> {
        None
    }

    fn armable_at(&self, _: i32, _: i32, _: View) -> Option<(&'static str, &'static str)> {
        None
    }

    fn automatable_of(&self, _: &str, _: &str) -> Option<(usize, &'static str, &'static str)> {
        None
    }

    fn spelling(&self, _: &str, _: &str, _: f64) -> Option<(String, &'static str)> {
        None
    }

    fn automated_rect(&self, _: &str, _: &str, _: View) -> Option<Rect> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Nothing,
    Cell(usize),
    End(usize, i32),
    Choice(usize, usize),
    Panel,
}

#[derive(Clone, Copy)]
struct Drag {
    cell: usize,
    origin_y: i32,
    last_y: i32,
    anchor: f64,
    armed: bool,
    again: bool,
}

#[derive(Clone, Copy)]
struct Panel {
    cell: usize,
    pressed: Option<usize>,
    menu: Option<u32>,
}

fn offered_mask<P>(menu: &Menu<P>, patch: &P) -> u32 {
    (0..menu.options.len().min(32)).filter(|&option| (menu.offered)(patch, option)).fold(0, |mask, option| mask | (1 << option))
}

fn nth_offered(mask: u32, n: usize) -> Option<usize> {
    (0..32).filter(|&option| mask & (1 << option) != 0).nth(n)
}

fn offered_position(mask: u32, option: usize) -> Option<usize> {
    (mask & (1 << option) != 0).then(|| (0..option).filter(|&earlier| mask & (1 << earlier) != 0).count())
}

#[derive(Clone)]
struct Entry {
    cell: usize,
    text: String,
}

pub struct Face {
    hover: Hit,
    drag: Option<Drag>,
    panel: Option<Panel>,
    entry: Option<Entry>,
    act: Option<Act>,
    wheel: (Option<usize>, i32),
    picked: Option<usize>,
    nudged: Option<usize>,
    page: usize,
    tray: Option<usize>,
    names: Vec<String>,
    withheld: Option<usize>,
    asleep: Option<(Act, &'static str)>,
    keyed: bool,
}

impl Default for Face {
    fn default() -> Face {
        Face::new()
    }
}

impl Face {
    pub fn new() -> Face {
        Face { hover: Hit::Nothing, drag: None, panel: None, entry: None, act: None, wheel: (None, 0), picked: None, nudged: None, page: 0, tray: None, names: Vec::new(), withheld: None, asleep: None, keyed: false }
    }

    pub fn keyed(self) -> Face {
        Face { keyed: true, ..self }
    }

    pub fn close(&mut self) {
        *self = Face { names: std::mem::take(&mut self.names), withheld: self.withheld, asleep: self.asleep, keyed: self.keyed, ..Face::new() };
    }

    pub fn name_list(&mut self, names: Vec<String>, withheld: Option<usize>) {
        (self.names, self.withheld) = (names, withheld);
    }

    pub fn asleep(&mut self, act: Act, why: Option<&'static str>) {
        self.asleep = why.map(|why| (act, why));
    }

    fn sleeping<P>(&self, cell: &Cell<P>) -> Option<&'static str> {
        match (cell.kind, self.asleep) {
            (Button(act), Some((held, why))) if act == held => Some(why),
            _ => None,
        }
    }

    fn options<'a, P>(&'a self, cell: &'a Cell<P>) -> Vec<&'a str> {
        if cell.options.is_empty() { self.names.iter().map(String::as_str).collect() } else { cell.options.to_vec() }
    }

    fn panel_options<'a, P>(&'a self, cell: &'a Cell<P>, panel: Panel) -> Vec<&'a str> {
        match (panel.menu, cell.menu) {
            (Some(mask), Some(menu)) => menu.options.iter().enumerate().filter(|(option, _)| mask & (1 << option) != 0).map(|(_, name)| *name).collect(),
            _ => self.offered(cell).into_iter().map(|(_, name)| name).collect(),
        }
    }

    fn offered<'a, P>(&'a self, cell: &'a Cell<P>) -> Vec<(usize, &'a str)> {
        let withheld = self.withheld.filter(|_| cell.options.is_empty());
        self.options(cell).into_iter().enumerate().filter(|(option, _)| withheld != Some(*option)).collect()
    }

    fn cell_panel<P>(&self, rows: i32, cell: &Cell<P>, panel: Panel) -> Rect {
        panel_rect(cell_rect(cell, rows), &self.panel_options(cell, panel), cell.icon_w(), Rect::new(0, 0, W, H))
    }

    fn spelled<P>(&self, cell: &Cell<P>, patch: &P) -> String {
        match cell.kind {
            Pick => self.options(cell).get((cell.param.get)(patch).round() as usize).map_or_else(String::new, |option| String::from(*option)),
            _ => spelled(cell, patch),
        }
    }

    fn step<P>(&self, cell: &Cell<P>, patch: &mut P, steps: i32) {
        let offered = self.offered(cell);
        let count = offered.len().max(1) as i32;
        let value = (cell.param.get)(patch).round().max(0.0) as usize;
        let current = offered.iter().position(|(option, _)| *option >= value).map_or(count - 1, |at| at as i32);
        let next = offered.get((current + steps).rem_euclid(count) as usize).map_or(0, |(option, _)| *option);
        (cell.param.set)(patch, next as f64);
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn picked(&self) -> Option<usize> {
        self.picked
    }

    pub fn picking(&self) -> bool {
        self.picked.is_some()
    }

    pub fn pick(&mut self, index: usize) {
        self.picked = Some(index);
    }

    pub fn open(&mut self, index: usize, tray: usize) {
        self.picked = Some(index);
        self.tray = Some(tray);
    }

    fn opened(&self, first: usize, count: usize) -> bool {
        self.tray.is_some_and(|tray| tray >= first && tray < first + count)
    }

    pub fn view(&self) -> View {
        View { page: self.page, tray: self.tray }
    }

    pub fn page(&self) -> usize {
        self.page
    }

    pub fn turn(&mut self, page: usize) {
        if page != self.page {
            self.page = page;
            self.picked = None;
            self.tray = None;
            self.nudged = None;
        }
    }

    fn live<P>(&self, cells: &[Cell<P>], patch: &P, index: usize) -> Option<usize> {
        live_at(cells, patch, self.view(), index)
    }

    fn shows<P>(&self, cell: &Cell<P>) -> bool {
        seen(cell, self.view())
    }

    pub fn unpick(&mut self) -> bool {
        self.nudged = None;
        self.entry = None;
        self.tray = None;
        self.picked.take().is_some()
    }

    pub fn typing(&self) -> bool {
        self.entry.is_some()
    }

    pub fn leave(&mut self) -> bool {
        self.entry.take().is_some()
    }

    pub fn begin<P>(&mut self, cells: &[Cell<P>], patch: &P, first: Option<char>) -> bool {
        if !self.keyed || self.drag.is_some() || self.panel.is_some() {
            return false;
        }
        let Some((index, cell)) = self.picked.and_then(|index| cells.get(index).map(|cell| (index, cell))) else {
            return false;
        };
        if !pickable(cell) || (cell.refuses)(patch) {
            return false;
        }
        self.nudged = None;
        self.entry = Some(Entry { cell: index, text: first.into_iter().collect() });
        true
    }

    pub fn key<P>(&mut self, key: Key, command: bool, cells: &[Cell<P>], patch: &mut P, bpm: f64) -> bool {
        let Some(entry) = &mut self.entry else {
            return false;
        };
        match key {
            Key::Return => {
                let (index, text) = (entry.cell, entry.text.clone());
                let Some((cell, value)) = cells.get(index).and_then(|cell| self.typed_value(cell, &text).map(|value| (cell, value))) else {
                    return false;
                };
                let was = (cell.param.get)(patch);
                assign(cell, patch, value);
                if let Some(tempo) = cell.tempo
                    && (cell.param.get)(patch) != was
                {
                    tempo(patch, bpm);
                }
                self.entry = None;
                self.nudged = Some(index);
                true
            }
            Key::Escape => {
                self.entry = None;
                true
            }
            Key::Delete => entry.text.pop().is_some(),
            Key::Char(c) if !command && cells.get(entry.cell).is_some_and(|cell| entry.text.chars().count() < room(cell)) => {
                entry.text.push(c);
                true
            }
            Key::Char(_) | Key::Space | Key::Left | Key::Right | Key::Up | Key::Down => false,
        }
    }

    fn words_of<'a, P>(&'a self, cell: &'a Cell<P>) -> Vec<&'a str> {
        match cell.kind {
            Toggle => OFF_ON.to_vec(),
            Switch => cell.options.to_vec(),
            _ => self.options(cell),
        }
    }

    fn typed_value<P>(&self, cell: &Cell<P>, text: &str) -> Option<f64> {
        match cell.kind {
            Rise | Centre | Pan => read(cell.format, text),
            Toggle | Switch => option_of(&self.words_of(cell), text).map(|index| index as f64),
            Pick => {
                let offered = self.offered(cell);
                let names: Vec<&str> = offered.iter().map(|(_, name)| *name).collect();
                option_of(&names, text).map(|at| offered[at].0 as f64)
            }
            Button(_) | Page(_) | Open(..) | Title => None,
        }
    }

    fn preview<P>(&self, cell: &Cell<P>, text: &str) -> Option<String> {
        let value = self.typed_value(cell, text)?;
        match cell.kind {
            Rise | Centre | Pan => Some(spell(cell.format, clamped(cell, value))),

            _ => self.words_of(cell).get(value as usize).map(|word| String::from(*word)),
        }
    }

    fn draw_entry<P>(&self, list: &mut DrawList, rows: i32, cells: &[Cell<P>], entry: &Entry) {
        let Some(cell) = cells.get(entry.cell) else {
            return;
        };
        let hint = match (entry.text.is_empty(), self.preview(cell, &entry.text)) {
            (true, _) => String::from(TYPE_HINT),
            (false, Some(preview)) => format!("Return sets {preview}, Escape leaves"),
            (false, None) => String::from(UNREAD_HINT),
        };
        show_entry(list, rows, cell.param.band, cell.param.name, &entry.text, ghost(cell), &hint);
    }

    pub fn nudged_cell(&self) -> Option<usize> {
        self.nudged
    }

    pub fn dragged_cell(&self) -> Option<usize> {
        self.drag.map(|drag| drag.cell)
    }

    pub fn hovered_cell(&self) -> Option<usize> {
        match self.hover {
            Hit::Cell(index) | Hit::End(index, _) | Hit::Choice(index, _) => Some(index),
            Hit::Panel | Hit::Nothing => None,
        }
    }

    pub fn draw_band<P>(&self, list: &mut DrawList, cells: &[Cell<P>], patch: &P) {
        for index in 0..cells.len() {
            self.draw_cell(list, NO_READOUT, cells, index, patch);
        }
        if let Some(panel) = self.panel {
            self.draw_panel(list, NO_READOUT, cells, panel, patch);
        }
    }

    pub fn panel_open(&self) -> bool {
        self.panel.is_some()
    }

    pub fn take_act(&mut self) -> Option<Act> {
        self.act.take()
    }

    fn hit<P>(&self, x: i32, y: i32, rows: i32, cells: &[Cell<P>]) -> Hit {
        if let Some(panel) = self.panel
            && let Some(cell) = cells.get(panel.cell)
        {
            let rect = self.cell_panel(rows, cell, panel);
            if rect.contains(x, y) {
                return match option_at(rect, &self.panel_options(cell, panel), x, y) {
                    Some(index) => Hit::Choice(panel.cell, index),
                    None => Hit::Panel,
                };
            }
        }
        let Some(index) = cells.iter().position(|cell| self.shows(cell) && cell.kind != Title && cell_rect(cell, rows).contains(x, y)) else {
            return Hit::Nothing;
        };
        let cell = &cells[index];
        if cell.kind == Pick && cell.span >= 2 {
            let inner = interior(cell_rect(cell, rows));
            let third = inner.w / 3;
            if x < inner.x + third {
                return Hit::End(index, -1);
            }
            if x >= inner.x + inner.w - third {
                return Hit::End(index, 1);
            }
        }
        Hit::Cell(index)
    }

    pub fn hover<P>(&mut self, point: Option<(i32, i32)>, rows: i32, cells: &[Cell<P>]) -> bool {
        let hit = point.map_or(Hit::Nothing, |(x, y)| self.hit(x, y, rows, cells));
        let changed = hit != self.hover || self.nudged.take().is_some();
        self.hover = hit;
        changed
    }

    pub fn down<P>(&mut self, x: i32, y: i32, rows: i32, cells: &[Cell<P>], patch: &mut P, bpm: f64) -> bool {
        let hit = self.hit(x, y, rows, cells);
        self.hover = hit;
        self.nudged = None;
        if let Some(panel) = self.panel {
            self.panel = match hit {
                Hit::Choice(_, index) => Some(Panel { pressed: Some(index), ..panel }),
                Hit::Panel => Some(panel),
                _ => None,
            };
            return true;
        }
        let (index, end) = match hit {
            Hit::Cell(index) => (index, None),
            Hit::End(index, direction) => (index, Some(direction)),
            Hit::Choice(..) | Hit::Panel | Hit::Nothing => return false,
        };
        let Some(index) = self.live(cells, patch, index) else {
            return false;
        };
        let cell = &cells[index];
        if (cell.refuses)(patch) || self.sleeping(cell).is_some() {
            return false;
        }
        if let Some(direction) = end.filter(|_| cell.kind == Pick) {
            self.step(cell, patch, direction);
            self.picked = Some(index);
            self.tray = cell.tray;
            return true;
        }
        match cell.kind {
            Rise | Centre | Pan => self.drag = Some(Drag { cell: index, origin_y: y, last_y: y, anchor: cell.param.scale.travel((cell.param.get)(patch)), armed: false, again: self.picked == Some(index) }),
            Toggle | Switch => {
                (cell.param.set)(patch, next_word(cell, (cell.param.get)(patch)));
                if let Some(tempo) = cell.tempo {
                    tempo(patch, bpm);
                }
            }
            Pick => self.panel = Some(Panel { cell: index, pressed: None, menu: None }),
            Button(act) => self.act = Some(act),
            Page(index) => self.turn(index),
            Open(first, count) if self.opened(first, count) => {
                self.unpick();
            }
            Open(first, _) => {
                if let Some(lead) = opener(cells, first) {
                    self.open(lead, first);
                }
            }
            Title => {}
        }
        if pickable(cell) {
            self.picked = Some(index);
            self.tray = cell.tray;
        }
        true
    }

    pub fn nudge<P>(&mut self, cells: &[Cell<P>], patch: &mut P, direction: i32, coarse: bool, bpm: f64) -> bool {
        let Some(cell) = self.picked.and_then(|index| cells.get(index)) else {
            return false;
        };
        if (cell.refuses)(patch) {
            return false;
        }
        match cell.kind {
            Rise | Centre | Pan => turn(cell, patch, direction, coarse),
            Toggle | Switch => {
                let was = (cell.param.get)(patch);
                (cell.param.set)(patch, stepped_word(cell, was, direction));
                if let Some(tempo) = cell.tempo
                    && (cell.param.get)(patch) != was
                {
                    tempo(patch, bpm);
                }
            }
            Pick => self.step(cell, patch, direction),
            Button(_) | Page(_) | Open(..) | Title => {}
        }
        self.nudged = self.picked;
        true
    }

    pub fn wheel<P>(&mut self, x: i32, y: i32, dy: i32, rows: i32, cells: &[Cell<P>], patch: &mut P) -> bool {
        let index = match self.hit(x, y, rows, cells) {
            Hit::Cell(index) | Hit::End(index, _) => index,
            Hit::Choice(..) | Hit::Panel | Hit::Nothing => return false,
        };
        let Some(index) = self.live(cells, patch, index) else {
            return false;
        };
        let cell = &cells[index];
        if cell.kind != Pick || (cell.refuses)(patch) || self.panel.is_some() {
            return false;
        }
        if self.wheel.0 != Some(index) {
            self.wheel = (Some(index), 0);
        }
        self.wheel.1 += dy;
        let steps = self.wheel.1 / WHEEL_PX;
        if steps == 0 {
            return false;
        }
        self.wheel.1 -= steps * WHEEL_PX;
        self.step(cell, patch, steps);
        true
    }

    pub fn moved<P>(&mut self, x: i32, y: i32, rows: i32, cells: &[Cell<P>], patch: &mut P) -> bool {
        let Some(drag) = &mut self.drag else {
            return self.hover(Some((x, y)), rows, cells);
        };
        let Some(cell) = cells.get(drag.cell) else {
            return false;
        };
        if y == drag.last_y {
            return false;
        }
        drag.last_y = y;
        drag.armed |= (y - drag.origin_y).abs() >= ARM;
        if !drag.armed {
            return false;
        }
        (cell.param.set)(patch, cell.param.scale.value(drag.anchor + f64::from(drag.origin_y - y) / THROW));
        true
    }

    pub fn up<P>(&mut self, x: i32, y: i32, rows: i32, cells: &[Cell<P>], patch: &mut P) -> bool {
        if let Some(drag) = self.drag.take() {
            self.hover = self.hit(x, y, rows, cells);
            if !drag.armed && drag.again {
                if let Some(menu) = cells.get(drag.cell).and_then(|cell| cell.menu) {
                    self.panel = Some(Panel { cell: drag.cell, pressed: None, menu: Some(offered_mask(&menu, patch)) });
                    self.hover = self.hit(x, y, rows, cells);
                } else if self.keyed {
                    self.entry = Some(Entry { cell: drag.cell, text: String::new() });
                } else {
                    self.picked = None;
                    self.tray = None;
                }
            }
            return true;
        }
        let Some(panel) = self.panel else {
            return false;
        };
        let Some(pressed) = panel.pressed else {
            return false;
        };
        if self.hit(x, y, rows, cells) == Hit::Choice(panel.cell, pressed)
            && let Some(cell) = cells.get(panel.cell)
        {
            match (panel.menu, cell.menu) {
                (Some(mask), Some(menu)) => {
                    if let Some(option) = nth_offered(mask, pressed) {
                        (menu.set)(patch, option as f64);
                    }
                }
                _ => {
                    if let Some((option, _)) = self.offered(cell).get(pressed) {
                        (cell.param.set)(patch, *option as f64);
                    }
                }
            }
            self.panel = None;
        } else {
            self.panel = Some(Panel { pressed: None, ..panel });
        }
        self.hover = self.hit(x, y, rows, cells);
        true
    }

    pub fn draw<P>(&self, list: &mut DrawList, rows: i32, cells: &[Cell<P>], patch: &P, rest: impl FnOnce(&mut DrawList, Rect)) {
        self.draw_readout(list, rows, cells, patch, rest);
        self.draw_cells(list, rows, cells, patch);
    }

    pub fn draw_pictured<P>(&self, list: &mut DrawList, rows: i32, cells: &[Cell<P>], patch: &P, picture: impl FnOnce(&mut DrawList, Rect)) {
        match &self.entry {
            Some(entry) => self.draw_entry(list, rows, cells, entry),
            None => {
                let rect = readout_rect(rows);
                list.frame(rect, FRAME, Tone::Ink100);
                picture(list, interior(rect));
            }
        }
        self.draw_cells(list, rows, cells, patch);
    }

    fn draw_cells<P>(&self, list: &mut DrawList, rows: i32, cells: &[Cell<P>], patch: &P) {
        list.frame(grid_rect(rows), FRAME, Tone::Ink100);
        for row in 0..ROWS - rows {
            for column in 0..COLUMNS {
                let filling = cells.iter().position(|cell| self.shows(cell) && cell.row == row && column >= cell.column && column < cell.column + cell.span);
                if let Some(index) = filling.filter(|&index| cells[index].column == column) {
                    self.draw_cell(list, rows, cells, self.live(cells, patch, index).unwrap_or(index), patch);
                }
            }
        }
        if self.entry.is_none() {
            for index in (0..cells.len()).filter(|&index| cells[index].row < 0 && self.shows(&cells[index])) {
                self.draw_cell(list, rows, cells, self.live(cells, patch, index).unwrap_or(index), patch);
            }
        }
        if let Some(panel) = self.panel {
            self.draw_panel(list, rows, cells, panel, patch);
        }
    }

    fn shown(&self) -> Option<(usize, Option<usize>)> {
        match (self.drag, self.nudged, self.hover) {
            (Some(drag), _, _) => Some((drag.cell, None)),
            (None, Some(index), _) => Some((index, None)),
            (None, None, Hit::Cell(index) | Hit::End(index, _)) => Some((index, None)),
            (None, None, Hit::Choice(index, option)) => Some((index, Some(option))),
            (None, None, Hit::Panel | Hit::Nothing) => None,
        }
    }

    pub fn shown_cell(&self) -> Option<usize> {
        self.shown().map(|(index, _)| index)
    }

    pub fn shown_value<P>(&self, cells: &[Cell<P>], patch: &P) -> Option<(usize, String)> {
        let (index, option) = self.shown()?;
        let index = self.live(cells, patch, index)?;
        let cell = &cells[index];
        let value = match option {
            Some(option) => self.panel.map_or_else(|| self.options(cell), |panel| self.panel_options(cell, panel)).get(option).map_or_else(String::new, |option| String::from(*option)),
            None => self.spelled(cell, patch),
        };
        Some((index, value))
    }

    fn draw_readout<P>(&self, list: &mut DrawList, rows: i32, cells: &[Cell<P>], patch: &P, rest: impl FnOnce(&mut DrawList, Rect)) {
        if let Some(entry) = &self.entry {
            self.draw_entry(list, rows, cells, entry);
            return;
        }
        match self.shown_value(cells, patch) {
            Some((index, value)) => {
                let cell = &cells[index];
                show_readout(list, rows, cell.param.band, cell.name_of(patch), &value, self.sleeping(cell).unwrap_or(cell.desc));
            }
            None => {
                let rect = readout_rect(rows);
                list.frame(rect, FRAME, Tone::Ink100);
                rest(list, interior(rect));
            }
        }
    }

    fn draw_cell<P>(&self, list: &mut DrawList, rows: i32, cells: &[Cell<P>], index: usize, patch: &P) {
        let cell = &cells[index];
        let rect = cell_rect(cell, rows);
        let zone = match self.hover {
            Hit::Cell(hovered) if cells.get(hovered).is_some_and(|over| slot_of(over) == slot_of(cell)) => Some(0),
            Hit::End(hovered, direction) if cells.get(hovered).is_some_and(|over| slot_of(over) == slot_of(cell)) => Some(direction),
            _ => None,
        };
        let asleep = self.sleeping(cell).is_some();
        let hovered = zone.is_some() && !(cell.refuses)(patch) && !asleep;
        let picked = self.picked == Some(index);
        if (cell.inert)(patch) || asleep {
            list.frame(rect, FRAME, Tone::Ink100);
            if hovered {
                list.fill(interior(rect), Tone::Ink25);
            }
            if picked {
                list.frame(interior(rect), FRAME, Tone::Ink100);
            }
            return;
        }
        let value = (cell.param.get)(patch);
        let open = self.panel.is_some_and(|panel| panel.cell == index);
        let label = cell.label_of(patch);
        match cell.kind {
            Toggle => draw_word_cell(list, rect, label, value >= 0.5, hovered),
            Switch => draw_word_cell(list, rect, word(cell, value), false, hovered),
            Pick => {
                let options = self.options(cell);
                let option = options.get(value.round() as usize).copied().unwrap_or("");
                match zone.filter(|_| hovered && !open && cell.span >= 2) {
                    Some(zone) => draw_pick_cell(list, rect, option, zone),
                    None => draw_word_cell(list, rect, option, open, hovered),
                }
            }
            Rise | Centre => draw_value_cell(list, rect, cell.kind == Centre, cell.param.scale.travel(value), label, hovered),
            Pan => draw_pan_cell(list, rect, cell.param.scale.travel(value), label, hovered),
            Button(_) => draw_word_cell(list, rect, label, false, hovered),
            Page(index) => draw_word_cell(list, rect, label, self.page == index, hovered),
            Open(first, count) => draw_word_cell(list, rect, label, self.opened(first, count), hovered),
            Title => draw_title(list, rect, label),
        }
        if cell.kind == Toggle {
            draw_brackets(list, interior(rect), if value >= 0.5 { Tone::Ground } else { Tone::Ink100 });
        }
        if cell.kind == Pick || cell.menu.is_some() {
            let fill = match cell.kind {
                Rise | Centre => fill_rows(cell.kind == Centre, cell.param.scale.travel(value)),
                _ => None,
            };
            let lit = open && cell.kind == Pick;
            draw_arrow(list, interior(rect), fill, if lit { Tone::Ground } else { Tone::Ink100 });
        }
        if picked {
            let inked = (matches!(cell.kind, Toggle) && value >= 0.5) || open;
            list.frame(interior(rect), FRAME, if inked { Tone::Ground } else { Tone::Ink100 });
        }
    }

    fn draw_panel<P>(&self, list: &mut DrawList, rows: i32, cells: &[Cell<P>], panel: Panel, patch: &P) {
        let Some(cell) = cells.get(panel.cell) else {
            return;
        };
        let hovered = match self.hover {
            Hit::Choice(_, index) => Some(index),
            _ => None,
        };
        let current = match (panel.menu, cell.menu) {
            (Some(mask), Some(menu)) => offered_position(mask, (menu.get)(patch).round() as usize),
            _ => self.offered(cell).iter().position(|(option, _)| *option == (cell.param.get)(patch).round() as usize),
        };
        draw_options(list, self.cell_panel(rows, cell, panel), &self.panel_options(cell, panel), current, hovered, panel.pressed, cell.icon);
    }
}
