use sdk::face::FRAME;
use sdk::font;
use sdk::keyboard::Key;
use super::tree::{Found, Node, parent_of};
use super::{Action, Browser, COUNT_ROOM, NAME_SCALE, PAD, Search, Shelf};

impl Browser {
    pub(super) fn focus(&mut self) -> Option<Action> {
        self.rename = None;
        self.search.get_or_insert_with(Search::default).focused = true;
        Some(Action::Search)
    }

    pub fn blur(&mut self) -> bool {
        match &mut self.search {
            Some(search) if search.focused => {
                search.focused = false;
                true
            }
            _ => false,
        }
    }

    pub(super) fn clear_search(&mut self) -> bool {
        let Some(search) = self.search.take() else {
            return false;
        };
        if !search.text.is_empty() {
            self.scroll = search.scroll;
        }
        self.settle();
        true
    }

    pub fn scans(&mut self) -> Vec<String> {
        let Some(search) = &mut self.search else {
            return Vec::new();
        };
        let mut shelves = std::mem::take(&mut search.shelves);
        search.shelves = self
            .roots
            .iter()
            .map(|root| {
                let mut shelf = shelves.iter().position(|shelf| shelf.root == root.path).map_or_else(|| Shelf { root: root.path.clone(), fixed: false, asked: false, done: false, entries: Vec::new() }, |at| shelves.swap_remove(at));
                shelf.fixed = root.fixed();
                shelf
            })
            .collect();
        let asking = search
            .shelves
            .iter_mut()
            .filter(|shelf| !shelf.asked)
            .map(|shelf| {
                shelf.asked = true;
                shelf.root.clone()
            })
            .collect();
        self.refilter();
        asking
    }

    pub fn scanned(&mut self, root: &str, found: Vec<Found>) {
        let Some(search) = &mut self.search else {
            return;
        };
        let Some(shelf) = search.shelves.iter_mut().find(|shelf| shelf.root == root && !shelf.done) else {
            return;
        };
        let fixed = shelf.fixed;
        shelf.entries = found.into_iter().map(|found| Node::found(found, None, &mut self.remembered, fixed)).collect();
        shelf.done = true;
        self.refilter();
    }

    pub fn scanning(&self, root: &str) -> bool {
        self.search.as_ref().is_some_and(|search| search.shelves.iter().any(|shelf| shelf.root == root && shelf.asked && !shelf.done))
    }

    pub(super) fn found(&self) -> Option<usize> {
        let search = self.hits()?;
        search.shelves.iter().all(|shelf| shelf.done).then_some(search.hits.len())
    }

    pub(super) fn refilter(&mut self) {
        if let Some(search) = &mut self.search {
            let words: Vec<&str> = search.text.split_whitespace().collect();
            let (hearts, hearting) = (&self.hearts, self.parked.is_some());
            let fits = |node: &Node| words.iter().all(|word| has(&node.name, word)) && (!hearting || hearts.contains(&node.path));
            search.hits = if words.is_empty() {
                Vec::new()
            } else {
                search.shelves.iter().enumerate().flat_map(|(at, shelf)| shelf.entries.iter().enumerate().filter(|(_, node)| fits(node)).map(move |(entry, _)| (at, entry))).collect()
            };
        }
        self.settle();
    }

    fn search_limit(&self) -> usize {
        font::glyphs_in(self.bar().name.w - 2 * FRAME - 3 * PAD - COUNT_ROOM, NAME_SCALE)
    }

    fn typed(&mut self, c: char, command: bool) -> (bool, Option<Action>) {
        let limit = self.search_limit();
        let scroll = self.scroll;
        let Some(search) = &mut self.search else {
            return (false, None);
        };
        if command || search.text.chars().count() >= limit {
            return (false, None);
        }
        if search.text.is_empty() {
            search.scroll = scroll;
        }
        search.text.push(c);
        self.scroll = 0;
        self.refilter();
        (true, None)
    }

    fn untyped(&mut self) -> (bool, Option<Action>) {
        let Some(search) = &mut self.search else {
            return (false, None);
        };
        if search.text.pop().is_none() {
            return (false, None);
        }
        self.scroll = if search.text.is_empty() { search.scroll } else { 0 };
        self.refilter();
        (true, None)
    }

    fn step_hits(&mut self, key: Key) -> (bool, Option<Action>) {
        if self.selected.is_some() {
            return self.step(key);
        }
        if key == Key::Down && !self.lines().is_empty() {
            return self.land(0);
        }
        (false, None)
    }

    pub fn type_key(&mut self, key: Key, shift: bool, command: bool) -> (bool, Option<Action>) {
        if self.rename.is_some() {
            return self.rename_key(key, shift, command);
        }
        match key {
            Key::Return => (self.blur(), None),
            Key::Escape => (self.clear_search(), None),
            Key::Up | Key::Down => self.step_hits(key),
            Key::Left | Key::Right => (false, None),
            Key::Delete => self.untyped(),
            Key::Space => self.typed(' ', command),
            Key::Char(c) => self.typed(if shift { c.to_ascii_uppercase() } else { c }, command),
        }
    }

    pub(super) fn delete_hit(&mut self, selected: &str) -> (bool, Option<Action>) {
        let Some(search) = &mut self.search else {
            return (false, None);
        };
        let Some(&(at, entry)) = search.hits.iter().find(|&&(shelf, entry)| search.shelves[shelf].entries[entry].path == selected) else {
            return (false, None);
        };
        let shelf = &mut search.shelves[at];
        if shelf.fixed {
            return (true, Some(Action::Kept(shelf.entries[entry].name.clone())));
        }
        let path = shelf.entries.remove(entry).path;
        let parent = parent_of(&path);
        self.selected = None;
        self.expect_move(&parent, &path, None);
        self.refilter();
        (true, Some(Action::Delete { parent, path }))
    }
}

fn has(name: &str, word: &str) -> bool {
    let (name, word) = (name.as_bytes(), word.as_bytes());
    word.is_empty() || name.windows(word.len()).any(|window| window.eq_ignore_ascii_case(word))
}
