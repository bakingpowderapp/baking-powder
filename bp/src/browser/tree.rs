use sdk::json::{self, Json};
use crate::project::{Document, Preset};
use std::collections::BTreeMap;

const JSON: &str = ".json";
const WAV: &str = ".wav";
const MID: &str = ".mid";
const MIDI: &str = ".midi";
const ALC: &str = ".alc";
const UNREADABLE: &str = "?";
pub(super) const HEARTS: &str = "Hearts.json";

#[derive(Clone)]
pub enum Item {
    Folder,
    Json(Parsed),
    Wave(u64),
    Midi(u64),
}

#[derive(Clone)]
pub struct Parsed(Body);

impl Item {
    pub fn read(text: &str) -> Item {
        let json = json::parse(text);
        let (author, about, description) = (field(json.as_ref(), "author"), about_of(json.as_ref()), field(json.as_ref(), "description"));
        Item::Json(Parsed(match json.as_ref().and_then(Document::from_json) {
            Some(Document::Preset(preset)) => Body::Preset { preset: Box::new(preset), author, about, description },
            Some(Document::Project(project)) => Body::Project { tracks: project.tracks.len(), length: project.end() },
            None => Body::Unreadable,
        }))
    }
}

pub struct Seen(BTreeMap<String, ((u64, u128), Parsed)>);

impl Seen {
    pub const NONE: Seen = Seen(BTreeMap::new());

    pub fn known(&self, path: &str, stamp: (u64, u128)) -> Option<Item> {
        self.0.get(path).filter(|(kept, _)| *kept == stamp).map(|(_, parsed)| Item::Json(parsed.clone()))
    }

    pub fn keep(&mut self, path: &str, stamp: (u64, u128), item: &Item) {
        if let Item::Json(parsed @ Parsed(Body::Project { .. } | Body::Unreadable)) = item {
            self.0.insert(String::from(path), (stamp, parsed.clone()));
        }
    }
}

#[derive(Clone)]
pub struct Found {
    pub path: String,
    pub name: String,
    pub item: Item,
}

impl Found {
    pub(super) fn is_folder(&self) -> bool {
        matches!(self.item, Item::Folder)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Folder,
    Document,
    Wave,
    Midi,
}

pub fn kind(path: &str, folder: bool) -> Option<Kind> {
    let name = last_component(path);
    if name.starts_with('.') || name == HEARTS {
        return None;
    }
    if folder {
        return Some(Kind::Folder);
    }
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(JSON) {
        Some(Kind::Document)
    } else if lower.ends_with(WAV) {
        Some(Kind::Wave)
    } else if lower.ends_with(MID) || lower.ends_with(MIDI) || lower.ends_with(ALC) {
        Some(Kind::Midi)
    } else {
        None
    }
}

#[derive(Clone)]
pub(super) struct Folder {
    pub(super) open: bool,
    pub(super) listed: bool,
    pub(super) fixed: bool,
    pub(super) children: Vec<Node>,
}

#[derive(Clone)]
pub(super) enum Body {
    Folder(Folder),
    Preset { preset: Box<Preset>, author: String, about: String, description: String },
    Project { tracks: usize, length: i32 },
    Unreadable,
    Wave(u64),
    Midi { extension: &'static str, bytes: u64 },
}

#[derive(Clone)]
pub(super) struct Node {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) body: Body,
}

fn last_component(path: &str) -> String {
    std::path::Path::new(path).file_name().and_then(|name| name.to_str()).map_or_else(|| String::from(path), String::from)
}

pub fn parent_of(path: &str) -> String {
    std::path::Path::new(path).parent().and_then(|parent| parent.to_str()).map_or_else(|| String::from(path), String::from)
}

pub(super) fn recall(remembered: &mut Vec<String>, path: &str) -> bool {
    remembered.iter().position(|kept| kept == path).map(|at| remembered.swap_remove(at)).is_some()
}

fn stem(name: &str, extension: &str) -> String {
    let cut = name.len().checked_sub(extension.len()).filter(|&at| name.is_char_boundary(at) && name[at..].eq_ignore_ascii_case(extension));
    cut.map_or_else(|| String::from(name), |at| String::from(&name[..at]))
}

fn field(json: Option<&Json>, key: &str) -> String {
    json.and_then(|json| json.get(key)).and_then(Json::as_str).map(String::from).unwrap_or_default()
}

fn host(url: &str) -> &str {
    url.trim_start_matches("https://").trim_start_matches("http://").trim_start_matches("www.").trim_end_matches('/')
}

fn about_of(json: Option<&Json>) -> String {
    let (description, website) = (field(json, "description"), field(json, "website"));
    let parts: Vec<&str> = [description.as_str(), host(&website)].into_iter().filter(|part| !part.is_empty()).collect();
    parts.join(" · ")
}

impl Node {
    pub(super) fn folder(path: String, open: bool, fixed: bool) -> Node {
        let name = last_component(&path);
        Node { path, name, body: Body::Folder(Folder { open, listed: false, fixed, children: Vec::new() }) }
    }

    pub(super) fn found(found: Found, before: Option<Node>, remembered: &mut Vec<String>, fixed: bool) -> Node {
        let Found { path, name, item } = found;
        match item {
            Item::Json(Parsed(body)) => Node { path, name: stem(&name, JSON), body },
            Item::Wave(bytes) => Node { path, name: stem(&name, WAV), body: Body::Wave(bytes) },
            Item::Midi(bytes) => {
                let lower = name.to_ascii_lowercase();
                let extension = [MIDI, ALC].into_iter().find(|extension| lower.ends_with(extension)).unwrap_or(MID);
                Node { path, name: stem(&name, extension), body: Body::Midi { extension, bytes } }
            }
            Item::Folder => {
                let carried = match before {
                    Some(Node { body: Body::Folder(folder), .. }) => Some(folder),
                    _ => None,
                };
                let folder = carried.unwrap_or_else(|| Folder { open: recall(remembered, &path), listed: false, fixed, children: Vec::new() });
                Node { path, name, body: Body::Folder(folder) }
            }
        }
    }

    pub(super) fn is_folder(&self) -> bool {
        matches!(self.body, Body::Folder(_))
    }

    pub(super) fn fixed(&self) -> bool {
        matches!(&self.body, Body::Folder(folder) if folder.fixed)
    }

    pub(super) fn kind(&self) -> &'static str {
        match &self.body {
            Body::Preset { preset, .. } => match preset.as_ref() {
                Preset::Rack(..) => "",
                preset => preset.name(),
            },
            Body::Folder(_) | Body::Project { .. } | Body::Wave(_) | Body::Midi { .. } => "",
            Body::Unreadable => UNREADABLE,
        }
    }

    pub(super) fn aside(&self) -> &str {
        match &self.body {
            Body::Preset { author, .. } if !author.is_empty() => author,
            _ => self.kind(),
        }
    }

    pub(super) fn extension(&self) -> &'static str {
        match &self.body {
            Body::Folder(_) => "",
            Body::Preset { .. } | Body::Project { .. } | Body::Unreadable => JSON,
            Body::Wave(_) => WAV,
            Body::Midi { extension, .. } => extension,
        }
    }

    pub(super) fn disk_name(&self) -> String {
        format!("{}{}", self.name, self.extension())
    }
}

pub(super) fn find<'a>(nodes: &'a [Node], path: &str) -> Option<&'a Node> {
    nodes.iter().find_map(|node| {
        if node.path == path {
            Some(node)
        } else if let Body::Folder(folder) = &node.body {
            find(&folder.children, path)
        } else {
            None
        }
    })
}

pub(super) fn inside<'a>(path: &'a str, folder: &str) -> Option<&'a str> {
    path.strip_prefix(folder).filter(|rest| rest.is_empty() || rest.starts_with('/'))
}

pub(super) fn order(nodes: &mut [Node]) {
    nodes.sort_by(|a, b| b.is_folder().cmp(&a.is_folder()).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

pub(super) fn fill(nodes: &mut [Node], path: &str, found: &[Found], remembered: &mut Vec<String>) {
    for node in nodes {
        let Body::Folder(folder) = &mut node.body else {
            continue;
        };
        if node.path == path {
            let fixed = folder.fixed;
            let mut before: BTreeMap<String, Node> = std::mem::take(&mut folder.children).into_iter().map(|child| (child.path.clone(), child)).collect();
            folder.listed = true;
            folder.children = found.iter().map(|found| Node::found(found.clone(), before.remove(&found.path), remembered, fixed)).collect();
        }
        fill(&mut folder.children, path, found, remembered);
    }
}

pub(super) fn fold(nodes: &mut [Node]) -> bool {
    let mut changed = false;
    for node in nodes {
        if let Body::Folder(folder) = &mut node.body {
            changed |= folder.open;
            folder.open = false;
            changed |= fold(&mut folder.children);
        }
    }
    changed
}

pub(super) fn shed(nodes: &mut [Node], remembered: &mut Vec<String>) {
    for node in nodes {
        let Body::Folder(folder) = &mut node.body else {
            continue;
        };
        if folder.open {
            shed(&mut folder.children, remembered);
        } else {
            keep_open(&folder.children, remembered);
            folder.children.clear();
            folder.listed = false;
        }
    }
}

fn keep_open(nodes: &[Node], remembered: &mut Vec<String>) {
    for node in nodes {
        let Body::Folder(folder) = &node.body else {
            continue;
        };
        if folder.open && !remembered.contains(&node.path) {
            remembered.push(node.path.clone());
        }
        keep_open(&folder.children, remembered);
    }
}

pub(super) fn unfold(nodes: &mut [Node], path: &str, remembered: &mut Vec<String>) {
    for node in nodes {
        let Body::Folder(folder) = &mut node.body else {
            continue;
        };
        let Some(rest) = inside(path, &node.path).filter(|rest| !rest.is_empty()) else {
            continue;
        };
        folder.open = true;
        if folder.listed {
            return unfold(&mut folder.children, path, remembered);
        }
        for (end, _) in rest.match_indices('/').skip(1) {
            let deeper = format!("{}{}", node.path, &rest[..end]);
            if !remembered.contains(&deeper) {
                remembered.push(deeper);
            }
        }
        return;
    }
}

pub(super) fn find_mut<'a>(nodes: &'a mut [Node], path: &str) -> Option<&'a mut Node> {
    nodes.iter_mut().find_map(|node| {
        if node.path == path {
            Some(node)
        } else if let Body::Folder(folder) = &mut node.body {
            find_mut(&mut folder.children, path)
        } else {
            None
        }
    })
}

#[derive(Clone, Copy)]
pub(super) struct Line<'a> {
    pub(super) node: &'a Node,
    pub(super) depth: i32,
    pub(super) parent: Option<&'a str>,
}

pub(super) fn walk<'a>(nodes: &'a [Node], depth: i32, parent: Option<&'a str>, lines: &mut Vec<Line<'a>>) {
    for node in nodes {
        lines.push(Line { node, depth, parent });
        if let Body::Folder(folder) = &node.body
            && folder.open
        {
            walk(&folder.children, depth + 1, Some(&node.path), lines);
        }
    }
}
