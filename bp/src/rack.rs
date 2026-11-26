use crate::arp::MidiRow;
use crate::project::moved_index;
use sdk::draw::{DrawList, Rect, Tone};
use sdk::face::{self, CELL, FRAME, PITCH};
use fx::Kind;
use crate::grid;
use sdk::keyboard::Key;
use crate::project::{BUSES, Cores, RACK_ROWS, Row, level, set_level, shown as window, split, taken};

pub const H: i32 = face::H;
pub const SHOWN: usize = face::ROWS as usize;
const MASTER: usize = 0;
const HEAD: usize = 2;
const TABS_Y: i32 = 0;
const MASTER_Y: i32 = PITCH;
const ARM: i32 = 4;
const THROW: f64 = 100.0;
const SEARCH: &str = "Search";
const PICKS: [(Pick, &str); Kind::ALL.len() + 3] = {
    let mut picks = [(Pick::Paste, "PASTE"); Kind::ALL.len() + 3];
    picks[1] = (Pick::Midi, MidiRow::LABEL);
    picks[2] = (Pick::Core, "Core");
    let mut index = 0;
    while index < Kind::ALL.len() {
        picks[index + 3] = (Pick::Effect(Kind::ALL[index]), Kind::ALL[index].label());
        index += 1;
    }
    picks
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Nothing,
    Grip(usize),
    Name(usize),
    Solo(usize),
    Bypass(usize),
    Bus(usize),
    Add,
    Option(usize),
    Panel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Paste,
    Midi,
    Core,
    Effect(Kind),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Offer {
    pub cores: Cores,
    pub paste: bool,
}

impl Offer {
    fn tracked(self) -> bool {
        self.cores != Cores::Refused
    }

    fn lists(self, pick: Pick) -> bool {
        match pick {
            Pick::Paste => self.paste,
            Pick::Midi | Pick::Core => self.cores != Cores::Refused,
            Pick::Effect(_) => true,
        }
    }

    fn picks(self) -> impl Iterator<Item = (Pick, &'static str)> {
        PICKS.into_iter().filter(move |(pick, _)| self.lists(*pick))
    }

    fn matched(self, text: &str) -> Vec<(Pick, &'static str)> {
        let text = text.to_ascii_lowercase();
        let mut matched: Vec<(Pick, &'static str)> = self.picks().filter(|(_, label)| label.to_ascii_lowercase().contains(&text)).collect();
        matched.sort_by_key(|(_, label)| !label.to_ascii_lowercase().starts_with(&text));
        matched
    }

    fn labels(self, text: &str) -> Vec<&'static str> {
        self.matched(text).into_iter().map(|(_, label)| label).collect()
    }

    fn pick(self, text: &str, option: usize) -> Option<Pick> {
        self.matched(text).get(option).map(|(pick, _)| *pick)
    }
}

#[derive(Clone, Copy)]
struct Mix {
    row: usize,
    origin_y: i32,
    origin: f64,
    moved: bool,
}

#[derive(Clone, Copy)]
struct Level {
    bus: usize,
    origin_y: i32,
    origin: f64,
    moved: bool,
}

#[derive(Clone, Copy)]
struct Grip {
    row: usize,
    grab: i32,
    origin_y: i32,
    at: (i32, i32),
    armed: bool,
    removable: bool,
    landing: bool,
    pinned: bool,
}

#[derive(Clone)]
struct Marquee {
    origin: (i32, i32),
    at: (i32, i32),
    armed: bool,
    hit: Option<usize>,
    before: Vec<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Open(usize),
    Mix(usize),
    Restore(usize),
    Insert(Pick),
    Move(usize, usize),
    Remove(usize),
    RemovePicked,
    Land(usize),
    Solo(usize),
    Times(usize),
}

#[derive(Clone)]
struct Panel {
    pressed: Option<usize>,
    text: String,
    chosen: usize,
}

pub struct Rack {
    w: i32,
    hover: Hit,
    mix: Option<Mix>,
    level: Option<Level>,
    grip: Option<Grip>,
    marquee: Option<Marquee>,
    panel: Option<Panel>,
    picked: Vec<usize>,
    scroll: usize,
    wheel: i32,
    bus: usize,
    lead: usize,
    base: usize,
    count: usize,
    room: bool,
    cores: usize,
}

fn search_cell(options: Rect) -> Rect {
    Rect::new(options.x, options.y - PITCH, options.w, CELL)
}

fn fixed(rows: &[Row], row: usize) -> bool {
    row == MASTER && rows.first().is_some_and(Row::is_midi)
}

fn removable(rows: &[Row], row: usize, offer: Offer, cores: usize) -> bool {
    !fixed(rows, row) && (!rows[row].is_core() || offer.cores != Cores::Required || cores > 1)
}

fn numbered(times: usize, hovered: bool, lifted: bool) -> bool {
    times > 1 || (hovered && !lifted)
}

fn touching(a: Rect, b: Rect) -> bool {
    let shared = a.intersect(b);
    shared.w.min(shared.h) > 0 && shared.w.max(shared.h) > 2 * FRAME
}

impl Rack {
    pub fn new(w: i32) -> Rack {
        Rack { w, hover: Hit::Nothing, mix: None, level: None, grip: None, marquee: None, panel: None, picked: Vec::new(), scroll: 0, wheel: 0, bus: 0, lead: 0, base: 0, count: 0, room: true, cores: 0 }
    }

    pub fn close(&mut self) {
        *self = Rack { scroll: self.scroll, bus: self.bus, ..Rack::new(self.w) };
    }

    pub fn rewind(&mut self) {
        self.scroll = 0;
        self.wheel = 0;
        self.bus = 0;
    }

    pub fn bus(&self) -> usize {
        self.bus
    }

    fn frame(&mut self, rows: &[Row]) {
        let tabbed = fixed(rows, MASTER);
        let listed = if tabbed { window(rows, self.bus) } else { 0..rows.len() };
        self.lead = if tabbed { HEAD } else { 0 };
        (self.base, self.count) = (listed.start, listed.len());
        self.room = taken(rows) < RACK_ROWS;
        self.cores = rows.iter().filter(|row| row.is_core()).count();
    }

    fn turn(&mut self, bus: usize) {
        if bus != self.bus {
            self.bus = bus;
            self.picked.retain(|picked| *picked == MASTER);
            self.scroll = 0;
            self.wheel = 0;
        }
    }

    fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, H)
    }

    fn slots(&self) -> usize {
        SHOWN - self.lead
    }

    fn slot_y(&self, slot: usize) -> i32 {
        (slot + self.lead) as i32 * PITCH
    }

    fn row_y(&self, row: usize) -> i32 {
        self.slot_y(row - self.base - self.top())
    }

    fn tabs(&self) -> impl Iterator<Item = (usize, Rect)> {
        let edge = |bus: usize| bus as i32 * (self.w - FRAME) / BUSES as i32;
        (0..BUSES).filter(|_| self.lead > 0).map(move |bus| (bus, Rect::new(edge(bus), TABS_Y, edge(bus + 1) + FRAME - edge(bus), CELL)))
    }

    fn grip_cell(&self, y: i32) -> Rect {
        Rect::new(0, y, CELL, CELL)
    }

    fn name_cell(&self, y: i32) -> Rect {
        Rect::new(PITCH, y, self.w - 2 * PITCH, CELL)
    }

    fn square_cell(&self, y: i32) -> Rect {
        Rect::new(self.w - CELL, y, CELL, CELL)
    }

    fn row_cells(&self, y: i32) -> [Rect; 3] {
        [self.grip_cell(y), self.name_cell(y), self.square_cell(y)]
    }

    fn row_rect(&self, y: i32) -> Rect {
        Rect::new(0, y, self.w, CELL)
    }

    fn master(&self) -> Option<(usize, i32)> {
        (self.lead > 0).then_some((MASTER, MASTER_Y))
    }

    fn listed(&self) -> std::ops::Range<usize> {
        self.base..self.base + self.count
    }

    fn max_scroll(&self) -> usize {
        (self.count + usize::from(self.room)).saturating_sub(self.slots())
    }

    fn top(&self) -> usize {
        self.scroll.min(self.max_scroll())
    }

    fn shown(&self) -> std::ops::Range<usize> {
        let top = self.base + self.top();
        top..(top + self.slots()).min(self.base + self.count)
    }

    fn placed(&self) -> impl Iterator<Item = (usize, i32)> {
        self.master().into_iter().chain(self.shown().map(|row| (row, self.row_y(row))))
    }

    fn add_slot(&self) -> Option<usize> {
        self.room.then(|| self.count.checked_sub(self.top())).flatten().filter(|&slot| slot < self.slots())
    }

    fn add_cell(&self) -> Option<Rect> {
        self.add_slot().map(|slot| self.name_cell(self.slot_y(slot)))
    }

    fn drop_row(&self, grip: Grip) -> usize {
        let top = grip.at.1 - grip.grab - self.slot_y(0);
        let slot = (top + PITCH / 2).div_euclid(PITCH).clamp(0, self.slots() as i32 - 1) as usize;
        self.base + (slot + self.top()).min(self.count.saturating_sub(1))
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        self.bounds().contains(x, y)
    }

    fn discards(&self, x: i32, y: i32) -> bool {
        !self.inside(x, y) || self.add_cell().is_some_and(|cell| cell.contains(x, y))
    }

    fn grouped(&self, row: usize) -> bool {
        self.picked.contains(&row)
    }

    fn text(&self) -> &str {
        self.panel.as_ref().map_or("", |panel| panel.text.as_str())
    }

    fn options_rect(&self, labels: &[&str]) -> Rect {
        let anchor = self.row_rect(self.slot_y(self.add_slot().unwrap_or(0)));
        face::panel_rect(Rect::new(anchor.x, anchor.y + PITCH, anchor.w, anchor.h), labels, 0, self.bounds())
    }

    pub fn typing(&self) -> bool {
        self.panel.is_some()
    }

    pub fn search(&mut self, rows: &[Row]) -> bool {
        self.frame(rows);
        if !self.room {
            return false;
        }
        self.scroll_to(self.count);
        self.panel = Some(Panel { pressed: None, text: String::new(), chosen: 0 });
        true
    }

    pub fn type_key(&mut self, key: Key, shift: bool, command: bool, offer: Offer) -> (bool, Option<Action>) {
        let limit = face::entry_glyphs(self.w - 2 * PITCH);
        let Some(panel) = &mut self.panel else {
            return (false, None);
        };
        let found = offer.matched(&panel.text).len();
        match key {
            Key::Escape => {
                self.panel = None;
                (true, None)
            }
            Key::Return => {
                let pick = offer.pick(&panel.text, panel.chosen);
                self.panel = None;
                (true, pick.map(Action::Insert))
            }
            Key::Up | Key::Down => {
                let chosen = if key == Key::Up { panel.chosen.saturating_sub(1) } else { (panel.chosen + 1).min(found.saturating_sub(1)) };
                let changed = chosen != panel.chosen;
                panel.chosen = chosen;
                (changed, None)
            }
            Key::Delete => {
                panel.chosen = 0;
                (panel.text.pop().is_some(), None)
            }
            Key::Char(c) if !command && panel.text.chars().count() < limit => {
                panel.text.push(if shift { c.to_ascii_uppercase() } else { c });
                panel.chosen = 0;
                (true, None)
            }
            Key::Char(_) | Key::Space | Key::Left | Key::Right => (false, None),
        }
    }

    pub fn dragging(&self) -> bool {
        self.mix.is_some() || self.level.is_some() || self.grip.is_some() || self.marquee.is_some()
    }

    pub fn panel_open(&self) -> bool {
        self.panel.is_some()
    }

    pub fn held(&self) -> Option<usize> {
        self.lifting().map(|grip| grip.row)
    }

    fn lifting(&self) -> Option<Grip> {
        self.grip.filter(|grip| !grip.pinned)
    }

    pub fn handled(&self) -> Option<usize> {
        match self.hover {
            Hit::Grip(row) if self.lifting().is_none_or(|grip| !grip.armed) => Some(row),
            _ => None,
        }
    }

    pub fn landing(&self) -> Option<usize> {
        self.grip.filter(|grip| grip.armed && grip.landing).map(|grip| grip.row)
    }

    pub fn picked(&self) -> Vec<usize> {
        let mut picked = self.picked.clone();
        picked.sort_unstable();
        picked
    }

    pub fn pick(&mut self, rows: &[Row], picked: Vec<usize>) {
        if let Some(&first) = picked.iter().filter(|&&row| !fixed(rows, row)).min() {
            self.reveal(rows, first);
        }
        self.picked = picked;
    }

    pub fn reveal(&mut self, rows: &[Row], row: usize) {
        if fixed(rows, MASTER) && row != MASTER && split(rows) != Some(row) {
            self.turn(usize::from(split(rows).is_some_and(|at| row > at)));
        }
        self.frame(rows);
        if self.listed().contains(&row) {
            self.scroll_to(row - self.base);
        }
    }

    fn scroll_to(&mut self, place: usize) {
        if place < self.scroll {
            self.scroll = place;
        } else if place >= self.scroll + self.slots() {
            self.scroll = place + 1 - self.slots();
        }
    }

    pub fn wheel(&mut self, dy: i32, rows: &[Row]) -> bool {
        self.frame(rows);
        self.wheel += dy;
        let steps = self.wheel / PITCH;
        self.wheel -= steps * PITCH;
        let scroll = (self.top() as i32 - steps).clamp(0, self.max_scroll() as i32) as usize;
        let changed = scroll != self.scroll;
        self.scroll = scroll;
        changed
    }

    pub fn pick_all(&mut self, rows: &[Row]) -> bool {
        self.frame(rows);
        self.picked = self.master().map(|(row, _)| row).into_iter().chain(self.listed()).collect();
        !self.picked.is_empty()
    }

    pub fn unpick(&mut self) -> bool {
        let had = !self.picked.is_empty();
        self.picked.clear();
        had
    }

    pub fn moved_row(&mut self, from: usize, to: usize) {
        for picked in &mut self.picked {
            *picked = moved_index(*picked, from, to);
        }
    }

    pub fn removed_row(&mut self, index: usize) {
        self.picked.retain(|picked| *picked != index);
        for picked in &mut self.picked {
            if *picked > index {
                *picked -= 1;
            }
        }
    }

    fn hit(&self, x: i32, y: i32, rows: &[Row], offer: Offer) -> Hit {
        if self.panel.is_some() {
            let labels = offer.labels(self.text());
            let options = self.options_rect(&labels);
            if options.contains(x, y) {
                return match face::option_at(options, &labels, x, y) {
                    Some(index) => Hit::Option(index),
                    None => Hit::Panel,
                };
            }
            if search_cell(options).contains(x, y) {
                return Hit::Panel;
            }
        }
        if let Some((bus, _)) = self.tabs().find(|(_, cell)| cell.contains(x, y)) {
            return Hit::Bus(bus);
        }
        for (row, line) in self.placed() {
            if self.grip_cell(line).contains(x, y) {
                return Hit::Grip(row);
            }
            if self.square_cell(line).contains(x, y) {
                return match (rows[row].is_core(), offer.tracked()) {
                    (true, true) => Hit::Solo(row),
                    (true, false) => Hit::Nothing,
                    (false, _) => Hit::Bypass(row),
                };
            }
            if self.name_cell(line).contains(x, y) {
                return Hit::Name(row);
            }
        }
        if self.add_cell().is_some_and(|cell| cell.contains(x, y)) {
            return Hit::Add;
        }
        Hit::Nothing
    }

    pub fn name_at(&mut self, x: i32, y: i32, rows: &[Row], offer: Offer) -> Option<usize> {
        self.frame(rows);
        match self.hit(x, y, rows, offer) {
            Hit::Name(row) => Some(row),
            _ => None,
        }
    }

    pub fn hover(&mut self, point: Option<(i32, i32)>, rows: &[Row], offer: Offer) -> bool {
        self.frame(rows);
        let hit = point.map_or(Hit::Nothing, |(x, y)| self.hit(x, y, rows, offer));
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    pub fn down(&mut self, x: i32, y: i32, clicks: i32, shift: bool, offer: Offer, rows: &mut Vec<Row>) -> (bool, Option<Action>) {
        self.frame(rows);
        let hit = self.hit(x, y, rows, offer);
        self.hover = hit;
        if let Some(panel) = &mut self.panel {
            match hit {
                Hit::Option(index) => panel.pressed = Some(index),
                Hit::Panel => panel.pressed = None,
                _ => self.panel = None,
            }
            return (true, None);
        }
        if let Hit::Bus(bus) = hit {
            if clicks >= 2 {
                return (set_level(rows, bus, 1.0), None);
            }
            self.level = Some(Level { bus, origin_y: y, origin: level(rows, bus), moved: false });
            return (false, None);
        }
        if shift {
            let hit = match hit {
                Hit::Grip(row) | Hit::Name(row) => Some(row),
                _ => None,
            };
            self.marquee = Some(Marquee { origin: (x, y), at: (x, y), armed: false, hit, before: self.picked.clone() });
            return (false, None);
        }
        match hit {
            Hit::Grip(row) if fixed(rows, row) => {
                self.picked = vec![row];
                self.grip = Some(Grip { row, grab: y - MASTER_Y, origin_y: y, at: (x, y), armed: false, removable: false, landing: false, pinned: true });
                (true, None)
            }
            Hit::Grip(row) => {
                let lifts = |picked: usize| picked < rows.len() && removable(rows, picked, offer, self.cores);
                let removable = if self.grouped(row) { self.picked.iter().any(|&picked| lifts(picked)) } else { removable(rows, row, offer, self.cores) };
                self.grip = Some(Grip { row, grab: y - self.row_y(row), origin_y: y, at: (x, y), armed: false, removable, landing: false, pinned: false });
                (false, None)
            }
            Hit::Name(row) if !rows[row].mixes() => {
                self.picked = vec![row];
                (true, Some(Action::Open(row)))
            }
            Hit::Name(row) => {
                if clicks >= 2 {
                    return (false, Some(Action::Restore(row)));
                }
                self.mix = Some(Mix { row, origin_y: y, origin: rows[row].mix(), moved: false });
                (false, Some(Action::Mix(row)))
            }
            Hit::Solo(row) => (true, Some(Action::Solo(row))),
            Hit::Bypass(row) => {
                let (bypass, grouped) = (!rows[row].bypassed(), self.grouped(row));
                for (_, entry) in rows.iter_mut().enumerate().filter(|(at, _)| *at == row || (grouped && self.grouped(*at))) {
                    entry.set_bypass(bypass);
                }
                (true, None)
            }
            Hit::Add => {
                self.panel = Some(Panel { pressed: None, text: String::new(), chosen: 0 });
                (true, None)
            }
            Hit::Nothing => (self.unpick(), None),
            Hit::Option(_) | Hit::Panel | Hit::Bus(_) => (false, None),
        }
    }

    pub fn moved(&mut self, x: i32, y: i32, offer: Offer, rows: &mut Vec<Row>, landing: bool) -> bool {
        if let Some(level) = &mut self.level {
            let dy = level.origin_y - y;
            if !level.moved && dy.abs() < ARM {
                return false;
            }
            level.moved = true;
            return set_level(rows, level.bus, (level.origin + f64::from(dy) / THROW).clamp(0.0, 1.0));
        }
        self.frame(rows);
        if let Some(mix) = &mut self.mix {
            let dy = mix.origin_y - y;
            if !mix.moved && dy.abs() < ARM {
                return false;
            }
            mix.moved = true;
            let amount = (mix.origin + f64::from(dy) / THROW).clamp(0.0, 1.0);
            let changed = rows[mix.row].mix() != amount;
            rows[mix.row].set_mix(amount);
            return changed;
        }
        let outside = !self.inside(x, y);
        if let Some(grip) = &mut self.grip {
            grip.at = (x, y);
            grip.armed |= (y - grip.origin_y).abs() >= ARM || outside;
            grip.landing = landing;
            return grip.armed;
        }
        if let Some(mut marquee) = self.marquee.take() {
            marquee.at = (x, y);
            marquee.armed |= (x - marquee.origin.0).abs() >= ARM || (y - marquee.origin.1).abs() >= ARM;
            if marquee.armed {
                let sweep = grid::marquee(marquee.origin, marquee.at);
                let mut picked = marquee.before.clone();
                for (row, _) in self.placed().filter(|&(_, line)| self.row_rect(line).overlaps(sweep)) {
                    if !picked.contains(&row) {
                        picked.push(row);
                    }
                }
                self.picked = picked;
            }
            let armed = marquee.armed;
            self.marquee = Some(marquee);
            return armed;
        }
        self.hover(Some((x, y)), rows, offer)
    }

    pub fn up(&mut self, x: i32, y: i32, rows: &[Row], offer: Offer) -> (bool, Option<Action>) {
        if let Some(level) = self.level.take() {
            if !level.moved {
                self.turn(level.bus);
            }
            self.frame(rows);
            self.hover = self.hit(x, y, rows, offer);
            return (true, None);
        }
        self.frame(rows);
        if let Some(mix) = self.mix.take() {
            self.hover = self.hit(x, y, rows, offer);
            if mix.moved {
                return (true, None);
            }
            self.picked = vec![mix.row];
            return (true, Some(Action::Open(mix.row)));
        }
        if let Some(grip) = self.grip.take() {
            self.hover = self.hit(x, y, rows, offer);
            if !grip.armed {
                self.picked = vec![grip.row];
                return (true, (self.hover == Hit::Grip(grip.row)).then_some(Action::Times(grip.row)));
            }
            if grip.pinned {
                return (true, None);
            }
            if grip.landing {
                return (true, Some(Action::Land(grip.row)));
            }
            if grip.removable && self.discards(x, y) {
                return (true, Some(if self.grouped(grip.row) { Action::RemovePicked } else { Action::Remove(grip.row) }));
            }
            let to = self.drop_row(grip);
            return (true, (to != grip.row).then_some(Action::Move(grip.row, to)));
        }
        if let Some(marquee) = self.marquee.take() {
            self.hover = self.hit(x, y, rows, offer);
            if marquee.armed {
                return (true, None);
            }
            let Some(pressed) = marquee.hit else {
                return (false, None);
            };
            match self.picked.iter().position(|picked| *picked == pressed) {
                Some(at) => {
                    self.picked.remove(at);
                }
                None => self.picked.push(pressed),
            }
            return (true, None);
        }
        let Some(pressed) = self.panel.as_ref().and_then(|panel| panel.pressed) else {
            return (false, None);
        };
        let action = if self.hit(x, y, rows, offer) == Hit::Option(pressed) {
            let pick = offer.pick(self.text(), pressed);
            self.panel = None;
            pick.map(Action::Insert)
        } else {
            if let Some(panel) = &mut self.panel {
                panel.pressed = None;
            }
            None
        };
        self.hover = self.hit(x, y, rows, offer);
        (true, action)
    }

    pub fn draw(&mut self, list: &mut DrawList, rows: &[Row], open: Option<usize>, offer: Offer, solo: Option<(usize, bool)>, curved: &[bool]) {
        self.frame(rows);
        let tracked = offer.tracked();
        let lifted = self.lifting().filter(|grip| grip.armed);
        let landing = lifted.is_some_and(|grip| grip.landing);
        let discarding = !landing && lifted.is_some_and(|grip| grip.removable && self.discards(grip.at.0, grip.at.1));
        let leaving = landing || discarding;
        let mut order: Vec<usize> = self.listed().collect();
        if let Some(grip) = lifted {
            if discarding && self.grouped(grip.row) {
                order.retain(|&row| !(self.grouped(row) && removable(rows, row, offer, self.cores)));
            } else {
                order.remove(grip.row - self.base);
                if !leaving {
                    order.insert(self.drop_row(grip) - self.base, grip.row);
                }
            }
        }
        let top = self.top();
        let last = (top + self.slots()).min(order.len());
        let resting = &order[top.min(last)..last];
        let mut lit: Vec<Rect> = Vec::new();
        for (row, y) in self.master().into_iter().chain(resting.iter().enumerate().map(|(slot, &row)| (row, self.slot_y(slot)))) {
            let cells = self.row_cells(y);
            if lifted.is_some_and(|grip| grip.row == row) {
                for cell in cells {
                    list.frame(cell, FRAME, Tone::Ink100);
                }
                continue;
            }
            self.draw_row(list, cells, row, &rows[row], open == Some(row), solo, curved.get(row) == Some(&true), tracked);
            lit.extend(self.lit(cells, row, &rows[row], open == Some(row), solo, tracked));
        }
        for slot in resting.len()..self.slots() {
            let [grip, name, square] = self.row_cells(self.slot_y(slot));
            list.frame(grip, FRAME, Tone::Ink100);
            list.frame(square, FRAME, Tone::Ink100);
            if self.add_slot() != Some(slot) {
                list.frame(name, FRAME, Tone::Ink100);
            } else if lifted.is_some_and(|grip| grip.removable) {
                face::draw_word_cell(list, name, "X", discarding, false);
                lit.extend(discarding.then_some(name));
            } else {
                face::draw_word_cell(list, name, "+", self.panel.is_some(), self.hover == Hit::Add);
                lit.extend(self.panel.is_some().then_some(name));
            }
        }
        for (bus, cell) in self.tabs() {
            let dragged = self.level.is_some_and(|level| level.bus == bus && level.moved);
            let amount = level(rows, bus);
            let label = if dragged { format!("{:.0}%", amount * 100.0) } else { (bus + 1).to_string() };
            if bus == self.bus && !dragged {
                face::draw_word_cell(list, cell, &label, true, false);
            } else {
                face::draw_value_cell(list, cell, false, amount, &label, dragged || self.hover == Hit::Bus(bus));
            }
        }
        lit.sort_by_key(|cell| (cell.y, cell.x));
        for (at, &cell) in lit.iter().enumerate() {
            for &next in lit[at + 1..].iter().filter(|next| touching(cell, **next)) {
                list.divide(cell, next, FRAME);
            }
        }
        if let Some(grip) = lifted {
            let lift = grip.at.1 - grip.grab;
            if leaving {
                list.frame(self.row_rect(lift), FRAME, Tone::Ink25);
            } else {
                let cells = self.row_cells(lift);
                for cell in cells {
                    list.fill(cell, Tone::Ground);
                }
                self.draw_row(list, cells, grip.row, &rows[grip.row], open == Some(grip.row), solo, curved.get(grip.row) == Some(&true), tracked);
            }
        }
        if let Some(marquee) = self.marquee.as_ref().filter(|marquee| marquee.armed) {
            grid::draw_marquee(list, grid::marquee(marquee.origin, marquee.at).intersect(self.bounds()));
        }
        if let Some(panel) = &self.panel {
            let hovered = match self.hover {
                Hit::Option(index) => Some(index),
                _ => None,
            };
            let labels = offer.labels(&panel.text);
            let options = self.options_rect(&labels);
            face::draw_entry_cell(list, search_cell(options), &panel.text, SEARCH);
            if !labels.is_empty() {
                face::draw_options(list, options, &labels, Some(panel.chosen), hovered, panel.pressed, None);
            }
        }
    }

    fn lit(&self, cells: [Rect; 3], row: usize, entry: &Row, open: bool, soloed: Option<(usize, bool)>, tracked: bool) -> impl Iterator<Item = Rect> {
        let square = if entry.is_core() { tracked && soloed.is_some_and(|(soloed, _)| soloed == row) } else { entry.bypassed() };
        cells.into_iter().zip([self.grouped(row), open, square]).filter(|(_, on)| *on).map(|(cell, _)| cell)
    }

    fn draw_row(&self, list: &mut DrawList, [grip, name, square]: [Rect; 3], row: usize, entry: &Row, open: bool, soloed: Option<(usize, bool)>, curved: bool, tracked: bool) {
        let dragging = self.mix.is_some_and(|mix| mix.row == row && mix.moved);
        let label = if dragging { format!("{:.0}%", entry.mix() * 100.0) } else { String::from(entry.label()) };
        if entry.is_midi() {
            face::draw_word_cell(list, name, &label, open, !open && self.hover == Hit::Name(row));
        } else if open {
            face::draw_word_cell(list, name, &label, true, false);
        } else {
            face::draw_value_cell(list, name, false, entry.mix(), &label, self.hover == Hit::Name(row) || dragging);
        }
        if curved {
            let inner = face::interior(name);
            face::draw_dot(list, inner.x + inner.w - face::DOT_INSET - face::DOT, inner.y + face::DOT_INSET);
        }
        if entry.is_core() && !tracked {
            face::draw_word_cell(list, square, "", false, false);
        } else if entry.is_core() {
            let state = soloed.filter(|(soloed, _)| *soloed == row);
            face::draw_word_cell(list, square, "S", state.is_some(), self.hover == Hit::Solo(row));
            if state.is_some_and(|(_, fx)| fx) {
                let inner = face::interior(square);
                list.frame(Rect::new(inner.x + FRAME, inner.y + FRAME, inner.w - 2 * FRAME, inner.h - 2 * FRAME), FRAME, Tone::Ground);
            }
        } else {
            face::draw_word_cell(list, square, "B", entry.bypassed(), self.hover == Hit::Bypass(row));
        }
        let held = self.grip.is_some_and(|grip| grip.row == row);
        let lifted = self.lifting().is_some_and(|grip| grip.row == row && grip.armed);
        let hovered = held || self.hover == Hit::Grip(row);
        let (inner, ink) = face::lit(list, grip, self.grouped(row), hovered);
        if numbered(entry.times(), hovered, lifted) {
            list.text_centered(inner, &entry.times().to_string(), ink, face::LABEL_SCALE);
        } else {
            face::draw_grip(list, inner, ink);
        }
    }
}
