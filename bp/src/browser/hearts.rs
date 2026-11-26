use sdk::json::{self, Json};
use super::tree::{HEARTS, Node, inside, order, parent_of};
use super::{Browser, HEARTED, Owed};

impl Browser {
    pub fn hearts(&self) -> &[String] {
        &self.hearts
    }

    pub fn set_heart(&mut self, path: &str, on: bool) -> bool {
        let at = self.hearts.iter().position(|heart| heart == path);
        if at.is_some() == on {
            return false;
        }
        self.owed.push((String::from(path), on));
        match at {
            Some(at) => {
                self.hearts.remove(at);
                self.loved.retain(|loved| loved.path != path);
            }
            None => self.hearts.push(String::from(path)),
        }
        self.refilter();
        true
    }

    pub(super) fn shelves(&self) -> Vec<(String, String)> {
        let shelve = |root: &Node| {
            let base = if root.fixed() { parent_of(&root.path) } else { root.path.clone() };
            let beside = if root.fixed() { parent_of(&base) } else { base.clone() };
            (base, format!("{beside}/{HEARTS}"))
        };
        let mut shelves: Vec<(String, String)> = self.roots.iter().map(shelve).collect();
        shelves.sort();
        shelves.dedup();
        shelves
    }

    pub fn unread(&mut self) -> Vec<String> {
        let fresh: Vec<String> = self.shelves().into_iter().map(|(_, file)| file).filter(|file| !self.asked.contains(file)).collect();
        self.asked.extend(fresh.iter().cloned());
        fresh
    }

    pub fn owing(&mut self) -> Vec<String> {
        let shelves = self.shelves();
        self.owed.retain(|(path, _)| shelf_of(&shelves, path).is_some());
        let mut files: Vec<String> = self.owed.iter().filter_map(|(path, _)| shelf_of(&shelves, path)).map(|(_, file)| file.clone()).collect();
        files.sort();
        files.dedup();
        files
    }

    pub fn shelved(&mut self, file: &str, bytes: Option<&[u8]>) -> Option<Option<String>> {
        let shelves = self.shelves();
        let (base, _) = shelves.iter().find(|(_, shelf)| shelf == file)?;
        let owns = |path: &str| shelf_of(&shelves, path).is_some_and(|(owner, _)| owner == base);
        let (owed, rest): (Owed, Owed) = std::mem::take(&mut self.owed).into_iter().partition(|(path, _)| owns(path));
        self.owed = rest;
        let mut kept: Vec<String> = match bytes.and_then(|bytes| std::str::from_utf8(bytes).ok()).and_then(json::parse) {
            Some(index) => index.get(HEARTED).and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(Json::as_str).map(|rest| if rest == "." { base.clone() } else { format!("{base}/{rest}") }).filter(|heart| owns(heart)).collect(),
            None => self.hearts.iter().filter(|heart| owns(heart)).cloned().collect(),
        };
        for (path, on) in &owed {
            kept.retain(|heart| heart != path);
            if *on {
                kept.push(path.clone());
            }
        }
        kept.sort();
        kept.dedup();
        self.hearts.retain(|heart| !owns(heart));
        self.hearts.extend(kept.iter().cloned());
        self.loved.retain(|loved| self.hearts.contains(&loved.path));
        self.refilter();
        Some((!owed.is_empty()).then(|| spell(base, &kept)))
    }

    pub fn homes(&self) -> Vec<String> {
        if !self.hearting() {
            return Vec::new();
        }
        let mut homes: Vec<String> = self.hearts.iter().map(|heart| parent_of(heart)).collect();
        homes.sort();
        homes.dedup();
        homes
    }

    pub fn unhomed(&self) -> Vec<String> {
        if !self.hearting() {
            return Vec::new();
        }
        let mut homes: Vec<String> = self.hearts.iter().filter(|heart| !self.loved.iter().any(|loved| loved.path == **heart)).map(|heart| parent_of(heart)).collect();
        homes.sort();
        homes.dedup();
        homes
    }

    pub(super) fn heart(&mut self, index: usize) -> bool {
        let Some(kept) = self.lines().get(index).map(|line| line.node.clone()) else {
            return false;
        };
        let (path, on) = (kept.path.clone(), !self.hearts.contains(&kept.path));
        if on {
            self.loved.push(kept);
            order(&mut self.loved);
        }
        self.set_heart(&path, on)
    }

    pub(super) fn rehome(&mut self, from: &str, to: Option<&str>) {
        self.loved.retain(|kept| inside(&kept.path, from).is_none());
        for heart in &mut self.hearts {
            let Some(rest) = inside(heart, from) else {
                continue;
            };
            let moved = to.map(|to| format!("{to}{rest}"));
            self.owed.push((heart.clone(), false));
            self.owed.extend(moved.iter().map(|moved| (moved.clone(), true)));
            *heart = moved.unwrap_or_default();
        }
        self.hearts.retain(|heart| !heart.is_empty());
        for kept in &mut self.arranged {
            if let Some(rest) = inside(kept, from) {
                *kept = to.map(|to| format!("{to}{rest}")).unwrap_or_default();
            }
        }
        self.arranged.retain(|kept| !kept.is_empty());
    }
}

pub(super) fn shelf_of<'a>(shelves: &'a [(String, String)], path: &str) -> Option<&'a (String, String)> {
    shelves.iter().filter(|(base, _)| inside(path, base).is_some()).max_by_key(|(base, _)| base.len())
}

fn spell(base: &str, kept: &[String]) -> String {
    let mut hearts: Vec<&str> = kept.iter().filter_map(|heart| inside(heart, base)).map(|rest| rest.strip_prefix('/').unwrap_or(".")).collect();
    hearts.sort_unstable();
    Json::Object(vec![(String::from(HEARTED), Json::Array(hearts.into_iter().map(Json::from).collect()))]).write()
}
