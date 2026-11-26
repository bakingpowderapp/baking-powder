use sdk::keyboard::Key;
use sdk::draw::{Fill, Rect};
use sdk::face::{CELL, FRAME, PITCH};
use sdk::font;
use crate::project::Preset;

mod hearts;
mod paint;
mod search;
mod tree;

pub use tree::{Found, Item, Kind, Seen, kind, parent_of};
use hearts::shelf_of;
use tree::{Body, HEARTS, Line, Node, fill, find, find_mut, fold, inside, order, recall, shed, unfold, walk};

const ROWS_MIN: i32 = 2;
const ARM: i32 = 4;
const NAME_SCALE: i32 = 2;
const ASIDE_SCALE: i32 = 1;
const PAD: i32 = 6;
const NAME_MIN_CELLS: i32 = 3;
const EXTRAS: i32 = 3;
const SEARCH: &str = "Search";
const BIG: &str = "BIG";
const HEARTED: &str = "hearts";
const PENDING: &str = "…";
const COUNT_ROOM: i32 = 6 * font::ADVANCE * ASIDE_SCALE;

type Owed = Vec<(String, bool)>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Nothing,
    Mark(usize),
    Name(usize),
    Grip(usize),
    Remove(usize),
    Reveal(usize),
    Copy(usize),
    Heart(usize),
    Add,
    Search,
    Clear,
    Refresh,
    Big,
    Locate,
    Collapse,
    Hearts,
}

struct Bar {
    mark: Rect,
    name: Rect,
    refresh: Option<Rect>,
    grow: Option<Rect>,
    target: Option<Rect>,
    square: Rect,
    heart: Rect,
}

struct Grip {
    path: String,
    origin: (i32, i32),
    at: (i32, i32),
    grab: (i32, i32),
    w: i32,
    armed: bool,
}

struct Hold {
    path: String,
    parent: Option<String>,
    grab: i32,
    origin: i32,
    at: i32,
    armed: bool,
}

#[derive(Clone, Copy)]
struct Cells {
    mark: Rect,
    name: Rect,
    remove: Option<Rect>,
    grip: Option<Rect>,
    heart: Option<Rect>,
    copy: Rect,
    square: Rect,
}

struct Rename {
    text: String,
    limit: usize,
}

struct Moving {
    parent: String,
    from: String,
    to: Option<String>,
}

struct Shelf {
    root: String,
    fixed: bool,
    asked: bool,
    done: bool,
    entries: Vec<Node>,
}

#[derive(Default)]
struct Search {
    text: String,
    focused: bool,
    scroll: usize,
    shelves: Vec<Shelf>,
    hits: Vec<(usize, usize)>,
}

enum Pressed {
    Folder(String),
    Preset { path: String, cell: Rect },
    Project { path: String, name: String, cell: Rect },
    File { path: String, kind: Kind, cell: Rect },
}

pub enum Action {
    Add,
    Search,
    List(String),
    Lists(Vec<String>),
    Load { name: String, description: String, preset: Option<Preset>, dropped: bool },
    Open { path: String, name: String },
    Place { path: String, kind: Kind, dropped: bool },
    Preview { path: String, kind: Kind, toggle: bool },
    Refresh,
    Big,
    Locate,
    Rename { path: String, name: String, parent: String },
    Delete { path: String, parent: String },
    Forget { path: String, name: String },
    Reveal(String),
    Copy(String),
    Exists(String),
    Kept(String),
}

pub enum Under<'a> {
    Add,
    Search { text: &'a str, found: Option<usize> },
    Clear,
    Refresh,
    Big,
    Locate,
    Collapse,
    Hearts(bool),
    Heart { name: &'a str, on: bool },
    Remove { name: &'a str },
    Reveal { name: &'a str },
    Copy { name: &'a str, done: bool },
    Grip { name: &'a str },
    Folder { name: &'a str, open: bool, root: bool, fixed: bool },
    Preset { name: &'a str, preset: &'a Preset, author: &'a str, about: &'a str },
    Project { name: &'a str, tracks: usize, length: i32 },
    Unreadable { name: &'a str },
    Wave { name: &'a str, path: &'a str },
    Midi { name: &'a str, path: &'a str, extension: &'static str },
}

pub enum Lifted<'a> {
    Preset { name: &'a str, preset: Option<&'a Preset> },
    Project { name: &'a str },
    Wave { name: &'a str },
    Midi { name: &'a str },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ghost {
    pub fills: Vec<Fill>,
    pub w: i32,
    pub h: i32,
    pub grab: (i32, i32),
}

pub struct Browser {
    w: i32,
    h: i32,
    roots: Vec<Node>,
    remembered: Vec<String>,
    arranged: Vec<String>,
    hearts: Vec<String>,
    loved: Vec<Node>,
    owed: Owed,
    asked: Vec<String>,
    moving: Vec<Moving>,
    parked: Option<usize>,
    scroll: usize,
    wheel: i32,
    hover: Hit,
    copied: bool,
    selected: Option<String>,
    wanted: Option<String>,
    rename: Option<Rename>,
    search: Option<Search>,
    grip: Option<Grip>,
    hold: Option<Hold>,
}

impl Default for Browser {
    fn default() -> Browser {
        Browser::new()
    }
}

impl Browser {
    pub fn new() -> Browser {
        Browser { w: 0, h: 0, roots: Vec::new(), remembered: Vec::new(), arranged: Vec::new(), hearts: Vec::new(), loved: Vec::new(), owed: Vec::new(), asked: Vec::new(), moving: Vec::new(), parked: None, scroll: 0, wheel: 0, hover: Hit::Nothing, copied: false, selected: None, wanted: None, rename: None, search: None, grip: None, hold: None }
    }

    pub fn layout(&mut self, w: i32, h: i32) {
        (self.w, self.h) = (w, h);
        self.settle();
    }

    fn rows(&self) -> usize {
        (self.h / PITCH).max(ROWS_MIN) as usize
    }

    fn list_rows(&self) -> usize {
        self.rows() - 1
    }

    pub fn close(&mut self) {
        self.hover = Hit::Nothing;
        self.selected = None;
        self.wanted = None;
        self.rename = None;
        self.grip = None;
        self.hold = None;
        self.blur();
    }

    pub fn resume(&mut self, folders: Vec<String>, unfolded: Vec<String>, arranged: Vec<String>) {
        self.roots = folders.into_iter().map(|path| Node::folder(path.clone(), unfolded.contains(&path), false)).collect();
        self.remembered = unfolded;
        self.arranged = arranged;
        self.hearts.clear();
        self.loved.clear();
        self.owed.clear();
        self.asked.clear();
        self.moving.clear();
        self.parked = None;
        self.scroll = 0;
        self.close();
    }

    pub fn add(&mut self, path: String) {
        match self.roots.iter_mut().find(|root| root.path == path) {
            Some(Node { body: Body::Folder(folder), .. }) => folder.open = true,
            _ => self.roots.push(Node::folder(path, true, false)),
        }
    }

    pub fn pin(&mut self, at: usize, path: String) -> bool {
        if let Some(Node { body: Body::Folder(folder), .. }) = self.roots.get(at).filter(|root| root.path == path) {
            return folder.open && !folder.listed;
        }
        self.roots.retain(|root| root.path != path);
        let open = recall(&mut self.remembered, &path);
        self.roots.insert(at.min(self.roots.len()), Node::folder(path, open, true));
        self.settle();
        open
    }

    pub fn unpin(&mut self, path: &str) {
        let Some(at) = self.roots.iter().position(|root| root.path == path && root.fixed()) else {
            return;
        };
        let root = self.roots.remove(at);
        let mut lines = Vec::new();
        walk(std::slice::from_ref(&root), 0, None, &mut lines);
        self.remembered.extend(lines.iter().filter(|line| matches!(&line.node.body, Body::Folder(folder) if folder.open)).map(|line| line.node.path.clone()));
        drop(lines);
        self.settle();
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn folders(&self) -> Vec<String> {
        self.roots.iter().filter(|root| !root.fixed()).map(|root| root.path.clone()).collect()
    }

    pub fn bytes(&self, path: &str) -> Option<u64> {
        match self.node(path)?.body {
            Body::Wave(bytes) | Body::Midi { bytes, .. } => Some(bytes),
            Body::Folder(_) | Body::Preset { .. } | Body::Project { .. } | Body::Unreadable => None,
        }
    }

    pub fn arranged(&self) -> &[String] {
        &self.arranged
    }

    fn hearting(&self) -> bool {
        self.parked.is_some()
    }

    fn adding(&self) -> bool {
        !self.filtering() && !self.hearting()
    }

    fn top(&self) -> &[Node] {
        if self.hearting() { &self.loved } else { &self.roots }
    }

    pub fn unfolded(&self) -> Vec<String> {
        self.tree().into_iter().filter(|line| matches!(&line.node.body, Body::Folder(folder) if folder.open)).map(|line| line.node.path.clone()).collect()
    }

    pub fn unlisted(&self) -> Vec<String> {
        self.tree().into_iter().filter(|line| matches!(&line.node.body, Body::Folder(folder) if folder.open && !folder.listed)).map(|line| line.node.path.clone()).collect()
    }

    pub fn listed(&mut self, path: &str, mut found: Vec<Found>) {
        let (moved, moving): (Vec<Moving>, Vec<Moving>) = std::mem::take(&mut self.moving).into_iter().partition(|moving| moving.parent == path && !found.iter().any(|found| found.path == moving.from));
        self.moving = moving;
        for moved in &moved {
            self.rehome(&moved.from, moved.to.as_deref());
        }
        let rank = |found: &Found| self.arranged.iter().position(|path| *path == found.path).unwrap_or(usize::MAX);
        found.sort_by(|a, b| b.is_folder().cmp(&a.is_folder()).then_with(|| rank(a).cmp(&rank(b))).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        fill(&mut self.roots, path, &found, &mut self.remembered);
        self.loved.retain(|kept| parent_of(&kept.path) != path);
        self.loved.extend(found.into_iter().filter(|found| !found.is_folder() && self.hearts.contains(&found.path)).map(|found| Node::found(found, None, &mut Vec::new(), false)));
        order(&mut self.loved);
        self.settle();
        if self.wanted.as_deref().is_some_and(|wanted| inside(wanted, path).is_some()) {
            self.arrive();
            self.wanted = self.wanted.take().filter(|wanted| parent_of(wanted) != path);
        }
    }

    fn settle(&mut self) {
        let lines = self.lines();
        let most = self.most(lines.len());
        let lost = self.selected.as_ref().is_some_and(|selected| !lines.iter().any(|line| line.node.path == *selected));
        let held = self.grip.as_ref().is_some_and(|grip| !lines.iter().any(|line| line.node.path == grip.path));
        let dropped = self.hold.as_ref().is_some_and(|hold| !lines.iter().any(|line| line.node.path == hold.path));
        drop(lines);
        self.scroll = self.scroll.min(most);
        if lost {
            self.selected = None;
            self.rename = None;
        }
        if held {
            self.grip = None;
        }
        if dropped {
            self.hold = None;
        }
    }

    fn most(&self, lines: usize) -> usize {
        (lines + usize::from(self.adding())).saturating_sub(self.list_rows())
    }

    fn filtering(&self) -> bool {
        self.search.as_ref().is_some_and(|search| !search.text.is_empty())
    }

    fn hits(&self) -> Option<&Search> {
        self.search.as_ref().filter(|search| !search.text.is_empty())
    }

    fn tree(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::new();
        walk(&self.roots, 0, None, &mut lines);
        lines
    }

    fn lines(&self) -> Vec<Line<'_>> {
        match self.hits() {
            Some(search) => search.hits.iter().map(|&(shelf, entry)| Line { node: &search.shelves[shelf].entries[entry], depth: 0, parent: None }).collect(),
            None if self.hearting() => {
                let mut lines = Vec::new();
                walk(&self.loved, 0, None, &mut lines);
                lines
            }
            None => self.tree(),
        }
    }

    fn node(&self, path: &str) -> Option<&Node> {
        match self.hits() {
            Some(search) => search.hits.iter().map(|&(shelf, entry)| &search.shelves[shelf].entries[entry]).find(|node| node.path == path),
            None => find(self.top(), path),
        }
    }

    fn slot(&self, at: i32, y: i32) -> Rect {
        Rect::new(self.w - CELL - at * PITCH, y, CELL, CELL)
    }

    fn row(&self, depth: i32, y: i32, squares: i32) -> (Rect, Rect) {
        let x = depth.min((self.w / PITCH - NAME_MIN_CELLS - 3).max(0)) * PITCH;
        (Rect::new(x, y, CELL, CELL), Rect::new(x + PITCH, y, self.slot(squares - 1, y).x + FRAME - x - PITCH, CELL))
    }

    fn cells(&self, line: Line, y: i32) -> Cells {
        let (folder, third) = (line.node.is_folder(), self.slot(2, y));
        let (grip, heart) = ((folder && !line.node.fixed()).then_some(third), (!folder).then_some(third));
        let remove = (grip.is_some() && line.parent.is_none()).then(|| self.slot(3, y));
        let (mark, name) = self.row(line.depth, y, 2 + i32::from(grip.or(heart).is_some()) + i32::from(remove.is_some()));
        Cells { mark, name, remove, grip, heart, copy: self.slot(1, y), square: self.slot(0, y) }
    }

    fn bar(&self) -> Bar {
        let spare = (self.w / PITCH - NAME_MIN_CELLS - 3).clamp(0, EXTRAS);
        let (mark, name) = self.row(0, 0, 2 + spare);
        let seat = |needs: i32, at: i32| (spare >= needs).then(|| self.slot(at, 0));
        Bar { mark, name, refresh: seat(3, 4), grow: seat(1, (spare + 1).min(3)), target: seat(2, 2), square: self.slot(1, 0), heart: self.slot(0, 0) }
    }

    fn landing(&self, hold: &Hold) -> usize {
        let centre = hold.at - hold.grab + CELL / 2 + (self.scroll as i32 - 1) * PITCH;
        let (siblings, start) = match hold.parent.as_deref() {
            None => (self.roots.as_slice(), 0),
            Some(parent) => {
                let Some(Node { body: Body::Folder(folder), .. }) = find(&self.roots, parent) else {
                    return 0;
                };
                let start = self.tree().iter().position(|line| line.node.path == parent).map_or(0, |at| at + 1);
                (folder.children.as_slice(), start as i32 * PITCH)
            }
        };
        let (mut top, mut landing) = (start, 0);
        for sibling in siblings.iter().filter(|sibling| sibling.is_folder() && sibling.path != hold.path) {
            let h = rows_of(sibling) as i32 * PITCH;
            if sibling.fixed() || top + h / 2 < centre {
                landing += 1;
            }
            top += h;
        }
        landing
    }

    fn shown(&self) -> Vec<Line<'_>> {
        let Some(hold) = self.hold.as_ref().filter(|hold| hold.armed) else {
            return self.lines();
        };
        let mut lines = Vec::new();
        preview(&self.roots, 0, None, hold, self.landing(hold), &mut lines);
        lines
    }

    fn arrange(&mut self, hold: &Hold, to: usize) {
        let Some(parent) = hold.parent.as_deref() else {
            if let Some(from) = self.roots.iter().position(|root| root.path == hold.path) {
                let root = self.roots.remove(from);
                self.roots.insert(to, root);
            }
            return;
        };
        let Some(Node { body: Body::Folder(folder), .. }) = find_mut(&mut self.roots, parent) else {
            return;
        };
        let Some(from) = folder.children.iter().position(|child| child.path == hold.path) else {
            return;
        };
        let child = folder.children.remove(from);
        folder.children.insert(to, child);
        let order: Vec<String> = folder.children.iter().filter(|child| child.is_folder()).map(|child| child.path.clone()).collect();
        self.arranged.retain(|path| !order.contains(path));
        self.arranged.extend(order);
    }

    fn hit(&self, x: i32, y: i32) -> Hit {
        if !(0..self.w).contains(&x) || !(0..self.h).contains(&y) {
            return Hit::Nothing;
        }
        let visible = (y / PITCH) as usize;
        if visible >= self.rows() {
            return Hit::Nothing;
        }
        if visible == 0 {
            let bar = self.bar();
            let over = |cell: Option<Rect>| cell.is_some_and(|cell| cell.contains(x, y));
            return if bar.heart.contains(x, y) {
                Hit::Hearts
            } else if bar.square.contains(x, y) {
                if self.filtering() { Hit::Clear } else { Hit::Collapse }
            } else if over(bar.target) {
                Hit::Locate
            } else if over(bar.grow) {
                Hit::Big
            } else if over(bar.refresh) {
                Hit::Refresh
            } else if bar.mark.contains(x, y) || bar.name.contains(x, y) {
                Hit::Search
            } else {
                Hit::Nothing
            };
        }
        let index = self.scroll + visible - 1;
        let lines = self.lines();
        if index == lines.len() {
            return if self.adding() { Hit::Add } else { Hit::Nothing };
        }
        let Some(&line) = lines.get(index) else {
            return Hit::Nothing;
        };
        let cells = self.cells(line, visible as i32 * PITCH);
        if line.node.is_folder() && cells.mark.contains(x, y) {
            Hit::Mark(index)
        } else if cells.square.contains(x, y) {
            Hit::Reveal(index)
        } else if cells.copy.contains(x, y) {
            Hit::Copy(index)
        } else if cells.heart.is_some_and(|heart| heart.contains(x, y)) {
            Hit::Heart(index)
        } else if cells.grip.is_some_and(|grip| grip.contains(x, y)) {
            Hit::Grip(index)
        } else if cells.remove.is_some_and(|remove| remove.contains(x, y)) {
            Hit::Remove(index)
        } else if cells.name.contains(x, y) {
            Hit::Name(index)
        } else {
            Hit::Nothing
        }
    }

    fn built_in(&self, line: Line) -> bool {
        line.node.fixed() || line.parent.and_then(|parent| find(self.top(), parent)).is_some_and(Node::fixed)
    }

    pub fn dragging(&self) -> bool {
        self.grip.is_some() || self.hold.is_some()
    }

    pub fn typing(&self) -> bool {
        self.rename.is_some() || self.search.as_ref().is_some_and(|search| search.focused)
    }

    pub fn lifted(&self) -> Option<(Lifted<'_>, (i32, i32))> {
        let grip = self.grip.as_ref().filter(|grip| grip.armed)?;
        let node = self.node(&grip.path)?;
        match &node.body {
            Body::Preset { preset, .. } => Some((Lifted::Preset { name: &node.name, preset: Some(preset.as_ref()) }, grip.at)),
            Body::Unreadable => Some((Lifted::Preset { name: &node.name, preset: None }, grip.at)),
            Body::Project { .. } => Some((Lifted::Project { name: &node.name }, grip.at)),
            Body::Wave(_) => Some((Lifted::Wave { name: &node.name }, grip.at)),
            Body::Midi { .. } => Some((Lifted::Midi { name: &node.name }, grip.at)),
            Body::Folder(_) => None,
        }
    }

    pub fn under(&self) -> Option<Under<'_>> {
        let index = match self.hover {
            Hit::Add => return Some(Under::Add),
            Hit::Search => return Some(Under::Search { text: self.search.as_ref().map_or("", |search| search.text.as_str()), found: self.found() }),
            Hit::Clear => return Some(Under::Clear),
            Hit::Refresh => return Some(Under::Refresh),
            Hit::Big => return Some(Under::Big),
            Hit::Locate => return Some(Under::Locate),
            Hit::Collapse => return Some(Under::Collapse),
            Hit::Hearts => return Some(Under::Hearts(self.hearting())),
            Hit::Mark(index) | Hit::Name(index) | Hit::Grip(index) | Hit::Remove(index) | Hit::Reveal(index) | Hit::Copy(index) | Hit::Heart(index) => index,
            Hit::Nothing => return None,
        };
        let line = *self.lines().get(index)?;
        let name = line.node.name.as_str();
        Some(match (self.hover, &line.node.body) {
            (Hit::Remove(_), _) => Under::Remove { name },
            (Hit::Reveal(_), _) => Under::Reveal { name },
            (Hit::Copy(_), _) => Under::Copy { name, done: self.copied },
            (Hit::Heart(_), _) => Under::Heart { name, on: self.hearts.contains(&line.node.path) },
            (Hit::Grip(_), _) => Under::Grip { name },
            (_, Body::Folder(folder)) => Under::Folder { name, open: folder.open, root: line.parent.is_none(), fixed: folder.fixed },
            (_, Body::Preset { preset, author, about, .. }) => Under::Preset { name, preset, author, about },
            (_, Body::Project { tracks, length }) => Under::Project { name, tracks: *tracks, length: *length },
            (_, Body::Unreadable) => Under::Unreadable { name },
            (_, Body::Wave(_)) => Under::Wave { name, path: &line.node.path },
            (_, Body::Midi { extension, .. }) => Under::Midi { name, path: &line.node.path, extension },
        })
    }

    pub fn hover(&mut self, point: Option<(i32, i32)>) -> bool {
        let hit = point.map_or(Hit::Nothing, |(x, y)| self.hit(x, y));
        let changed = hit != self.hover;
        self.hover = hit;
        self.copied &= !changed;
        changed
    }

    fn toggle(&mut self, index: usize) -> Option<Action> {
        let path = self.path_at(index)?;
        let Some(Node { body: Body::Folder(folder), .. }) = find_mut(&mut self.roots, &path) else {
            return None;
        };
        folder.open = !folder.open;
        folder.open.then_some(Action::List(path))
    }

    fn rest(&mut self) -> &mut usize {
        match &mut self.search {
            Some(search) if !search.text.is_empty() => &mut search.scroll,
            _ => &mut self.scroll,
        }
    }

    fn filter(&mut self) -> Option<Action> {
        let parked = self.parked.take();
        let rest = std::mem::replace(self.rest(), parked.unwrap_or(0));
        self.parked = parked.is_none().then_some(rest);
        if self.filtering() {
            self.scroll = 0;
        }
        self.refilter();
        self.hearting().then(|| Action::Lists(self.homes()))
    }

    fn expect_move(&mut self, parent: &str, from: &str, to: Option<String>) {
        self.moving.retain(|moving| moving.from != from);
        if to.as_deref() != Some(from) {
            self.moving.push(Moving { parent: String::from(parent), from: String::from(from), to });
        }
    }

    fn path_at(&self, index: usize) -> Option<String> {
        Some(self.lines().get(index)?.node.path.clone())
    }

    fn removal(&self, index: usize) -> Option<Action> {
        let lines = self.lines();
        let node = lines.get(index)?.node;
        Some(Action::Forget { path: node.path.clone(), name: node.name.clone() })
    }

    pub fn forget(&mut self, path: &str) {
        let (shelves, file) = (self.shelves(), format!("{path}/{HEARTS}"));
        let gone = |heart: &str| shelf_of(&shelves, heart).is_some_and(|(_, shelf)| *shelf == file);
        self.roots.retain(|root| root.path != path);
        self.arranged.retain(|kept| self.roots.iter().any(|root| inside(kept, &root.path).is_some()));
        self.moving.retain(|moving| inside(&moving.from, path).is_none());
        self.hearts.retain(|heart| !gone(heart));
        self.loved.retain(|loved| !gone(&loved.path));
        self.asked.retain(|asked| *asked != file);
        self.settle();
    }

    fn shows(&self, path: &str) -> bool {
        self.lines().iter().any(|line| line.node.path == path)
    }

    pub fn locate(&mut self, path: &str) -> Vec<String> {
        if !self.shows(path) && self.hearting() && !self.hearts.iter().any(|heart| heart == path) {
            self.filter();
        }
        if !self.shows(path) && self.filtering() {
            self.clear_search();
        }
        if !self.shows(path) && self.hearting() {
            self.filter();
        }
        let folded = !self.shows(path);
        if folded {
            unfold(&mut self.roots, path, &mut self.remembered);
        }
        self.wanted = Some(String::from(path));
        self.arrive();
        let lists = if folded { self.unlisted() } else { Vec::new() };
        self.wanted = self.wanted.take().filter(|_| !lists.is_empty());
        lists
    }

    fn arrive(&mut self) {
        let lines = self.lines();
        let most = self.most(lines.len());
        let at = self.wanted.as_ref().and_then(|wanted| lines.iter().position(|line| line.node.path == *wanted));
        drop(lines);
        if let Some(at) = at {
            self.scroll = at.saturating_sub((self.list_rows() - 1) / 2).min(most);
            self.wanted = None;
        }
    }

    pub fn renew(&mut self) -> Vec<String> {
        shed(&mut self.roots, &mut self.remembered);
        self.asked.clear();
        if !self.hearting() {
            self.loved.clear();
        }
        let shelves = self.search.iter_mut().flat_map(|search| &mut search.shelves);
        for shelf in shelves.filter(|shelf| shelf.done) {
            (shelf.asked, shelf.done) = (false, false);
        }
        self.roots.iter().map(|root| root.path.clone()).collect()
    }

    fn collapse(&mut self) -> bool {
        let folded = !self.hearting() && (fold(&mut self.roots) | !std::mem::take(&mut self.remembered).is_empty());
        let changed = folded || self.scroll > 0;
        self.scroll = 0;
        self.settle();
        changed
    }

    pub fn down(&mut self, x: i32, y: i32, clicks: i32) -> (bool, Option<Action>) {
        let hit = self.hit(x, y);
        self.hover = hit;
        self.copied = matches!(hit, Hit::Copy(_));
        self.wanted = None;
        let index = match hit {
            Hit::Add => return (false, Some(Action::Add)),
            Hit::Nothing => return (self.unpick(), None),
            Hit::Mark(index) => return (true, self.toggle(index)),
            Hit::Grip(index) => {
                self.lift_folder(index, y);
                return (true, None);
            }
            Hit::Remove(index) => return (false, self.removal(index)),
            Hit::Reveal(index) => return (false, self.path_at(index).map(Action::Reveal)),
            Hit::Copy(index) => return (true, self.path_at(index).map(Action::Copy)),
            Hit::Heart(index) => return (self.heart(index), None),
            Hit::Hearts => return (true, self.filter()),
            Hit::Search => return (true, self.focus()),
            Hit::Clear => return (self.clear_search(), None),
            Hit::Refresh => return (false, Some(Action::Refresh)),
            Hit::Big => return (false, Some(Action::Big)),
            Hit::Locate => return (false, Some(Action::Locate)),
            Hit::Collapse => return (self.collapse(), None),
            Hit::Name(index) => index,
        };
        let pressed = {
            let lines = self.lines();
            let Some(&line) = lines.get(index) else {
                return (false, None);
            };
            let cell = self.cells(line, (index - self.scroll + 1) as i32 * PITCH).name;
            let (path, name) = (line.node.path.clone(), line.node.name.clone());
            match &line.node.body {
                Body::Folder(_) => Pressed::Folder(path),
                Body::Preset { .. } | Body::Unreadable => Pressed::Preset { path, cell },
                Body::Project { .. } => Pressed::Project { path, name, cell },
                Body::Wave(_) => Pressed::File { path, kind: Kind::Wave, cell },
                Body::Midi { .. } => Pressed::File { path, kind: Kind::Midi, cell },
            }
        };
        self.rename = None;
        match pressed {
            Pressed::Folder(path) => {
                self.selected = Some(path);
                if clicks >= 2 {
                    return (true, self.toggle(index));
                }
                (true, None)
            }
            Pressed::Preset { path, cell } => {
                if clicks < 2 {
                    self.lift(path, (x, y), cell);
                }
                (true, None)
            }
            Pressed::Project { path, name, cell } => {
                if clicks >= 2 {
                    self.selected = Some(path.clone());
                    return (true, Some(Action::Open { path, name }));
                }
                self.lift(path, (x, y), cell);
                (true, None)
            }
            Pressed::File { path, kind, cell } => {
                if clicks >= 2 {
                    return (true, Some(Action::Place { path, kind, dropped: false }));
                }
                self.lift(path.clone(), (x, y), cell);
                (true, Some(Action::Preview { path, kind, toggle: true }))
            }
        }
    }

    fn lift(&mut self, path: String, at: (i32, i32), cell: Rect) {
        self.grip = Some(Grip { path: path.clone(), origin: at, at, grab: (at.0 - cell.x, at.1 - cell.y), w: cell.w, armed: false });
        self.selected = Some(path);
    }

    fn lift_folder(&mut self, index: usize, y: i32) {
        let lines = self.lines();
        let Some(line) = lines.get(index) else {
            return;
        };
        let (path, parent) = (line.node.path.clone(), line.parent.map(String::from));
        drop(lines);
        let top = (index - self.scroll + 1) as i32 * PITCH;
        self.rename = None;
        self.selected = Some(path.clone());
        self.hold = Some(Hold { path, parent, grab: y - top, origin: y, at: y, armed: false });
    }

    pub fn moved(&mut self, x: i32, y: i32, lifts: bool) -> bool {
        if let Some(hold) = &mut self.hold {
            hold.at = y;
            hold.armed |= (y - hold.origin).abs() >= ARM;
            return hold.armed;
        }
        if let Some(grip) = &mut self.grip {
            grip.at = (x, y);
            grip.armed |= lifts && ((x - grip.origin.0).abs() >= ARM || (y - grip.origin.1).abs() >= ARM);
            return grip.armed;
        }
        self.hover(Some((x, y)))
    }

    pub fn up(&mut self, x: i32, y: i32, over: bool) -> (bool, Option<Action>) {
        if let Some(hold) = self.hold.take() {
            if hold.armed {
                let to = self.landing(&hold);
                self.arrange(&hold, to);
                self.settle();
            }
            self.hover = self.hit(x, y);
            return (true, None);
        }
        let Some(grip) = self.grip.take() else {
            return (false, None);
        };
        self.hover = self.hit(x, y);
        let loads = over || !grip.armed;
        let action = self.node(&grip.path).and_then(|node| match &node.body {
            Body::Preset { preset, description, .. } if loads => Some(Action::Load { name: node.name.clone(), description: description.clone(), preset: Some(preset.as_ref().clone()), dropped: grip.armed }),
            Body::Unreadable if loads => Some(Action::Load { name: node.name.clone(), description: String::new(), preset: None, dropped: grip.armed }),
            Body::Wave(_) if grip.armed => Some(Action::Place { path: node.path.clone(), kind: Kind::Wave, dropped: true }),
            Body::Midi { .. } if grip.armed => Some(Action::Place { path: node.path.clone(), kind: Kind::Midi, dropped: true }),
            Body::Preset { .. } | Body::Unreadable | Body::Wave(_) | Body::Midi { .. } | Body::Project { .. } | Body::Folder(_) => None,
        });
        (true, action)
    }

    pub fn scroll(&mut self, dy: i32) -> bool {
        self.wanted = None;
        self.wheel += dy;
        let steps = self.wheel / PITCH;
        self.wheel -= steps * PITCH;
        let most = self.most(self.lines().len()) as i32;
        let scroll = (self.scroll as i32 - steps).clamp(0, most) as usize;
        let changed = scroll != self.scroll;
        self.scroll = scroll;
        changed
    }

    pub fn has_selection(&self) -> bool {
        self.selected.is_some()
    }

    pub fn step(&mut self, key: Key) -> (bool, Option<Action>) {
        let lines = self.lines();
        let Some(index) = self.selected.as_ref().and_then(|selected| lines.iter().position(|line| line.node.path == *selected)) else {
            return (false, None);
        };
        let line = lines[index];
        let open = matches!(&line.node.body, Body::Folder(folder) if folder.open);
        let landing = match key {
            Key::Up => index.checked_sub(1),
            Key::Down => (index + 1 < lines.len()).then_some(index + 1),
            Key::Right if line.node.is_folder() && !open => {
                drop(lines);
                return (true, self.toggle(index));
            }
            Key::Right if open => lines.get(index + 1).filter(|next| next.parent == Some(line.node.path.as_str())).map(|_| index + 1),
            Key::Left if open => {
                drop(lines);
                return (true, self.toggle(index));
            }
            Key::Left => line.parent.and_then(|parent| lines.iter().position(|line| line.node.path == parent)),
            _ => None,
        };
        drop(lines);
        match landing {
            Some(landing) => self.land(landing),
            None => (false, None),
        }
    }

    fn land(&mut self, landing: usize) -> (bool, Option<Action>) {
        let lines = self.lines();
        let Some(&line) = lines.get(landing) else {
            return (false, None);
        };
        let node = line.node;
        let path = node.path.clone();
        let action = match &node.body {
            Body::Wave(_) => Some(Action::Preview { path: path.clone(), kind: Kind::Wave, toggle: false }),
            Body::Midi { .. } => Some(Action::Preview { path: path.clone(), kind: Kind::Midi, toggle: false }),
            Body::Preset { preset, description, .. } => Some(Action::Load { name: node.name.clone(), description: description.clone(), preset: Some(preset.as_ref().clone()), dropped: false }),
            Body::Unreadable => Some(Action::Load { name: node.name.clone(), description: String::new(), preset: None, dropped: false }),
            Body::Project { .. } | Body::Folder(_) => None,
        };
        drop(lines);
        self.selected = Some(path);
        self.rename = None;
        self.scroll = self.scroll.clamp(landing.saturating_sub(self.list_rows() - 1), landing);
        (true, action)
    }

    pub fn unpick(&mut self) -> bool {
        self.rename = None;
        self.selected.take().is_some()
    }

    pub fn escape(&mut self) -> bool {
        if self.rename.take().is_some() {
            return true;
        }
        self.unpick() || self.clear_search()
    }

    pub fn begin_rename(&mut self) -> bool {
        let Some(selected) = self.selected.clone() else {
            return false;
        };
        let lines = self.lines();
        let Some(&line) = lines.iter().find(|line| line.node.path == selected) else {
            return false;
        };
        if line.parent.is_none() || self.built_in(line) {
            return false;
        }
        let cell = self.cells(line, 0).name;
        let room = cell.w - 2 * FRAME - 3 * PAD - font::width(line.node.aside(), ASIDE_SCALE);
        self.rename = Some(Rename { text: String::new(), limit: font::glyphs_in(room, NAME_SCALE) });
        true
    }

    pub fn rename_key(&mut self, key: Key, shift: bool, command: bool) -> (bool, Option<Action>) {
        let typed = match key {
            Key::Return => return self.finish_rename(),
            Key::Escape => {
                self.rename = None;
                return (true, None);
            }
            Key::Delete => None,
            Key::Space => Some(' '),
            Key::Char(c) => Some(if shift { c.to_ascii_uppercase() } else { c }),
            Key::Left | Key::Right | Key::Up | Key::Down => return (false, None),
        };
        let Some(rename) = &mut self.rename else {
            return (false, None);
        };
        match typed {
            None => (rename.text.pop().is_some(), None),
            Some(_) if command || rename.text.chars().count() >= rename.limit => (false, None),
            Some('/' | '\\' | ':') => (false, None),
            Some(c) => {
                rename.text.push(c);
                (true, None)
            }
        }
    }

    pub fn finish_rename(&mut self) -> (bool, Option<Action>) {
        let Some(rename) = self.rename.take() else {
            return (false, None);
        };
        let name = String::from(rename.text.trim());
        let Some(selected) = self.selected.clone() else {
            return (true, None);
        };
        if name.is_empty() {
            return (true, None);
        }
        let action = {
            let lines = self.lines();
            let Some(&line) = lines.iter().find(|line| line.node.path == selected) else {
                return (true, None);
            };
            let Some(parent) = line.parent else {
                return (true, None);
            };
            let Some(Node { body: Body::Folder(folder), .. }) = find(self.top(), parent) else {
                return (true, None);
            };
            let file = format!("{name}{}", line.node.extension());
            if folder.children.iter().any(|sibling| sibling.path != selected && sibling.disk_name().eq_ignore_ascii_case(&file)) {
                Action::Exists(name)
            } else {
                Action::Rename { path: selected, name: file, parent: String::from(parent) }
            }
        };
        if let Action::Rename { path, name, parent } = &action {
            self.expect_move(parent, path, Some(format!("{parent}/{name}")));
            self.selected = None;
        }
        (true, Some(action))
    }

    pub fn delete(&mut self) -> (bool, Option<Action>) {
        let Some(selected) = self.selected.clone() else {
            return (false, None);
        };
        self.rename = None;
        if self.filtering() {
            return self.delete_hit(&selected);
        }
        let (built_in, parent, name) = {
            let lines = self.lines();
            let Some(&line) = lines.iter().find(|line| line.node.path == selected) else {
                return (false, None);
            };
            (self.built_in(line), line.parent.map(String::from), line.node.name.clone())
        };
        if built_in {
            return (true, Some(Action::Kept(name)));
        }
        let Some(parent) = parent else {
            return (true, None);
        };
        self.selected = None;
        self.expect_move(&parent, &selected, None);
        (true, Some(Action::Delete { path: selected, parent }))
    }
}

fn rows_of(root: &Node) -> usize {
    let mut lines = Vec::new();
    walk(std::slice::from_ref(root), 0, None, &mut lines);
    lines.len()
}

fn preview<'a>(nodes: &'a [Node], depth: i32, parent: Option<&'a str>, hold: &Hold, landing: usize, lines: &mut Vec<Line<'a>>) {
    if parent != hold.parent.as_deref() {
        for node in nodes {
            lines.push(Line { node, depth, parent });
            if let Body::Folder(folder) = &node.body
                && folder.open
            {
                preview(&folder.children, depth + 1, Some(&node.path), hold, landing, lines);
            }
        }
        return;
    }
    let Some(held) = nodes.iter().find(|node| node.path == hold.path) else {
        return walk(nodes, depth, parent, lines);
    };
    let mut order: Vec<&Node> = nodes.iter().filter(|node| node.path != hold.path).collect();
    order.insert(landing.min(order.len()), held);
    for node in order {
        if node.path == hold.path {
            lines.push(Line { node, depth, parent });
        } else {
            walk(std::slice::from_ref(node), depth, parent, lines);
        }
    }
}
