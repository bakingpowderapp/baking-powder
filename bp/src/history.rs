use sdk::json::{self, Json};
use crate::project::Project;
use sdk::schema::{Fields, number};
use std::sync::atomic::{AtomicU64, Ordering};

pub const STEPS: usize = 250;

static CHANGES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, PartialEq, Default)]
struct Splice {
    head: usize,
    tail: usize,
    text: String,
}

impl Splice {
    fn between(from: &str, to: &str) -> Splice {
        let shared = from.len().min(to.len());
        let mut head = from.bytes().zip(to.bytes()).take_while(|(old, new)| old == new).count();
        while !to.is_char_boundary(head) {
            head -= 1;
        }
        let mut tail = from.bytes().rev().zip(to.bytes().rev()).take(shared - head).take_while(|(old, new)| old == new).count();
        while !to.is_char_boundary(to.len() - tail) {
            tail -= 1;
        }
        Splice { head, tail, text: String::from(&to[head..to.len() - tail]) }
    }

    fn onto(&self, from: &str) -> Option<String> {
        let tail = from.len().checked_sub(self.tail).filter(|tail| *tail >= self.head)?;
        Some([from.get(..self.head)?, self.text.as_str(), from.get(tail..)?].concat())
    }
}

#[derive(Clone, PartialEq)]
pub struct Step {
    pub project: Project,
    pub path: Option<String>,
    pub song: u64,
    pub at: u64,
    back: Splice,
}

impl Step {
    fn from_json(json: &Json, newer: &mut String) -> Option<Step> {
        let kept = |key: &str| number(Some(json), key, 0.0) as usize;
        let back = Splice { head: kept("head"), tail: kept("tail"), text: String::from(json.get("text")?.as_str()?) };
        *newer = back.onto(newer)?;
        let project = Project::from_json(&json::parse(newer)?)?;
        let path = json.get("path").and_then(Json::as_str).map(String::from);
        Some(Step { project, path, song: number(Some(json), "song", 0.0).max(0.0) as u64, at: number(Some(json), "at", 0.0).max(0.0) as u64, back })
    }
}

pub struct History {
    past: Vec<Step>,
    future: Vec<Step>,
    tip: String,
    changes: u64,
}

impl History {
    pub fn new(project: &Project, path: Option<&str>, song: u64) -> History {
        History { past: vec![step(project, path, song)], future: Vec::new(), tip: spelled(project), changes: bump() }
    }

    pub fn from_json(json: Option<&Json>) -> Option<History> {
        let mut newer = String::new();
        let mut past: Vec<Step> = json.and_then(Json::as_array).unwrap_or(&[]).iter().take(STEPS).map_while(|edit| Step::from_json(edit, &mut newer)).collect();
        let tip = std::mem::take(&mut past.first_mut()?.back.text);
        past.reverse();
        Some(History { past, future: Vec::new(), tip, changes: bump() })
    }

    pub fn to_json(&self, count: usize) -> Json {
        let edits = self.recent(count).iter().rev().enumerate().map(|(age, step)| {
            let (head, tail, text) = if age == 0 { (0, 0, &self.tip) } else { (step.back.head, step.back.tail, &step.back.text) };
            let mut fields = Fields::new();
            fields.push("at", Json::Number(step.at as f64));
            if let Some(path) = &step.path {
                fields.push("path", Json::String(path.clone()));
            }
            fields.number("song", step.song as f64, 0.0);
            fields.number("head", head as f64, 0.0);
            fields.number("tail", tail as f64, 0.0);
            fields.push("text", Json::String(text.clone()));
            fields.into_json()
        });
        Json::Array(edits.collect())
    }

    pub fn commit(&mut self, project: &Project, path: Option<&str>, song: u64) {
        if self.past.last().is_some_and(|last| last.project == *project && last.path.as_deref() == path && last.song == song) {
            return;
        }
        self.push(step(project, path, song));
        self.future.clear();
        if self.past.len() > STEPS + 1 {
            self.past.remove(0);
        }
        self.changes = bump();
    }

    pub fn changes(&self) -> u64 {
        self.changes
    }

    pub fn rename(&mut self, song: u64, to: &str) {
        for step in self.past.iter_mut().chain(&mut self.future).filter(|step| step.song == song) {
            step.path = Some(String::from(to));
        }
        self.changes = bump();
    }

    pub fn undo(&mut self) -> Option<Step> {
        if self.past.len() < 2 {
            return None;
        }
        let present = self.past.pop()?;
        self.future.push(present);
        let last = self.past.last()?;
        self.tip = last.back.onto(&self.tip).unwrap_or_else(|| spelled(&last.project));
        self.changes = bump();
        Some(last.clone())
    }

    pub fn retake(&mut self) {
        if self.past.len() < 2 {
            return;
        }
        self.past.pop();
        if let Some(last) = self.past.last() {
            self.tip = last.back.onto(&self.tip).unwrap_or_else(|| spelled(&last.project));
        }
        self.changes = bump();
    }

    pub fn redo(&mut self) -> Option<Step> {
        let next = self.future.pop()?;
        self.push(next.clone());
        self.changes = bump();
        Some(next)
    }

    pub fn recent(&self, count: usize) -> &[Step] {
        &self.past[self.past.len().saturating_sub(count)..]
    }

    pub fn peek(&self, index: usize) -> Option<&Step> {
        self.past.get(index)
    }

    pub fn steps(&self) -> usize {
        self.past.len()
    }

    pub fn every(&self) -> impl Iterator<Item = &Step> {
        self.past.iter().chain(&self.future)
    }

    fn push(&mut self, step: Step) {
        let tip = spelled(&step.project);
        if let Some(last) = self.past.last_mut() {
            last.back = Splice::between(&tip, &self.tip);
        }
        self.tip = tip;
        self.past.push(step);
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn bump() -> u64 {
    CHANGES.fetch_add(1, Ordering::Relaxed) + 1
}

fn spelled(project: &Project) -> String {
    project.to_json().write_at(0)
}

fn step(project: &Project, path: Option<&str>, song: u64) -> Step {
    Step { project: project.clone(), path: path.map(String::from), song, at: now(), back: Splice::default() }
}
