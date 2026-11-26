use super::*;
use crate::project::warp::{self, ENVELOPE_MAX, Warped};

pub(super) const NO_NOTES: &str = "NO NOTES";
const ENTRY_DEPTH: usize = 2;
const PEEK_BYTES: u64 = 3_000_000;
const PEEKS: usize = 8;
const ONE_SHOT_SECONDS: f64 = 3.0;

pub(super) struct Pool {
    pub(super) waves: Vec<Arc<Wave>>,
    pending: Vec<(String, Landing)>,
    scores: Vec<(String, Landing)>,
    peeks: Vec<(String, Option<Peek>)>,
    peeking: Vec<String>,
    pub(super) origins: Vec<(String, String)>,
    pub(super) sources: Vec<(String, String)>,
    tried: Vec<(String, String)>,
    pub(super) unread: Vec<String>,
    sought: Vec<Sought>,
    pub(super) audition: Option<Audition>,
    pub(super) previewed: Option<String>,
    pub(super) kept: u64,
    made: Vec<String>,
    stowing: Vec<(String, String, u64)>,
    unstowed: Vec<(String, String, u64)>,
}

pub(super) struct Lost {
    shown: String,
    path: Option<String>,
    forget: bool,
}

enum Scan {
    Awaited,
    Empty,
    Holds(String),
}

struct Sought {
    name: String,
    roots: Vec<(String, Scan)>,
    lost: Lost,
}

pub(super) struct Audition {
    path: String,
    kind: browser::Kind,
    left: f32,
    awaited: bool,
}

#[derive(Clone)]
pub(super) enum Peek {
    Wave(Arc<Wave>),
    Score(Arc<Score>),
}

pub(super) struct Score {
    pub(super) notes: Vec<SongNote>,
    length: i32,
}

impl Score {
    fn parse(bytes: &[u8], bpm: f64) -> Option<Score> {
        let parts = midi::parse(bytes, bpm)?;
        let mut notes: Vec<SongNote> = parts.iter().flat_map(|part| part.notes.iter().map(|note| SongNote { start: f64::from(note.start), end: f64::from(note.start + note.length), pitch: note.pitch, velocity: note.velocity, articulation: note.articulation })).collect();
        let length = parts.iter().map(|part| Clip::midi(0, String::new(), part.notes.clone(), part.length).length).max()?;
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        Some(Score { notes, length })
    }

    pub(super) fn beats(&self) -> f64 {
        beats(self.length)
    }

    pub(super) fn pitch_range(&self) -> (u8, u8) {
        self.notes.iter().fold((u8::MAX, u8::MIN), |(lo, hi), note| (lo.min(note.pitch), hi.max(note.pitch)))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Landing {
    pub(super) track: Option<usize>,
    at: usize,
    joins: bool,
    tick: i32,
}

pub(super) fn refused(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::Unreadable => UNREADABLE,
        Refusal::TooLong => TOO_LONG,
    }
}

impl Pool {
    pub(super) fn new() -> Pool {
        Pool { waves: Vec::new(), pending: Vec::new(), scores: Vec::new(), peeks: Vec::new(), peeking: Vec::new(), origins: Vec::new(), sources: Vec::new(), tried: Vec::new(), unread: Vec::new(), sought: Vec::new(), audition: None, previewed: None, kept: 0, made: Vec::new(), stowing: Vec::new(), unstowed: Vec::new() }
    }

    pub(super) fn wave(&self, name: &str) -> Option<&Arc<Wave>> {
        self.waves.iter().find(|wave| wave.name == name)
    }

    pub(super) fn wave_at(&self, path: &str) -> Option<&Arc<Wave>> {
        self.waves.iter().find(|wave| wave.path.as_deref() == Some(path))
    }

    pub(super) fn wave_of(&self, clip: &Clip) -> Option<Arc<Wave>> {
        clip.file.as_ref().and_then(|file| self.wave(&file.name)).cloned()
    }

    pub(super) fn fresh_name(&self, stem: &str) -> String {
        numbered(stem, |name| self.wave(name).is_some())
    }

    pub(super) fn knows(&self, name: &str) -> bool {
        self.wave(name).is_some() || self.origins.iter().chain(&self.sources).any(|(known, _)| known == name)
    }

    fn claimable_name(&self, stem: &str) -> String {
        if self.wave(stem).is_some_and(|wave| wave.frames() == 0) { String::from(stem) } else { self.fresh_name(stem) }
    }

    pub(super) fn keep(&mut self, wave: impl Into<Arc<Wave>>, origin: Option<&str>) {
        let wave = wave.into();
        if let Some(origin) = origin {
            self.origins.retain(|(name, _)| *name != wave.name);
            self.origins.push((wave.name.clone(), String::from(origin)));
        }
        if wave.frames() > 0
            && let Some(path) = wave.path.as_deref().filter(|path| wave_file(path))
        {
            self.sources.retain(|(name, _)| *name != wave.name);
            self.sources.push((wave.name.clone(), String::from(path)));
        }
        if origin.is_none() && wave.path.is_none() && wave.frames() > 0 && !self.made.contains(&wave.name) {
            self.made.push(wave.name.clone());
        }
        self.waves.retain(|kept| kept.name != wave.name);
        self.waves.push(wave);
        self.kept += 1;
    }

    pub(super) fn change_song(&mut self) {
        self.pending.clear();
        self.scores.clear();
        self.tried.clear();
        self.sought.clear();
    }

    fn forget(&mut self, name: &str) {
        self.waves.retain(|wave| wave.name != name);
        self.origins.retain(|(known, _)| known != name);
        self.sources.retain(|(known, _)| known != name);
    }

    pub(super) fn renumber(&mut self, place: impl Fn(&mut usize) -> bool) {
        for (_, landing) in self.pending.iter_mut().chain(&mut self.scores) {
            if landing.track.as_mut().is_some_and(|track| !place(track)) {
                *landing = Landing { track: None, at: usize::MAX, joins: false, tick: landing.tick };
            } else if let Some(mut above) = landing.at.checked_sub(1) {
                landing.at = if place(&mut above) { above.saturating_add(1) } else { above };
            }
        }
    }

    fn seek(&mut self, name: &str, lost: Lost, roots: Vec<String>, scanning: impl Fn(&str) -> bool) -> Result<Vec<String>, Lost> {
        if roots.is_empty() {
            return Err(lost);
        }
        if let Some(sought) = self.sought.iter_mut().find(|sought| sought.name == name) {
            sought.lost = lost;
            return Ok(Vec::new());
        }
        let awaited = |root: &String| self.sought.iter().flat_map(|sought| &sought.roots).any(|(kept, scan)| kept == root && matches!(scan, Scan::Awaited));
        let asking = roots.iter().filter(|root| !scanning(root) && !awaited(root)).cloned().collect();
        self.sought.push(Sought { name: String::from(name), roots: roots.into_iter().map(|root| (root, Scan::Awaited)).collect(), lost });
        Ok(asking)
    }

    fn sought(&mut self, root: &str, found: &[Found]) -> Vec<(Sought, Option<String>)> {
        let mut done = Vec::new();
        for mut sought in std::mem::take(&mut self.sought) {
            let wanted = format!("{}.wav", sought.name).to_lowercase();
            if let Some((_, scan)) = sought.roots.iter_mut().find(|(kept, scan)| kept == root && matches!(scan, Scan::Awaited)) {
                let hit = found.iter().find(|found| matches!(found.item, browser::Item::Wave(_)) && found.name.to_lowercase() == wanted && !self.tried.iter().any(|(known, tried)| *known == sought.name && *tried == found.path));
                *scan = hit.map_or(Scan::Empty, |found| Scan::Holds(found.path.clone()));
            }
            let first = sought.roots.iter().rev().map(|(_, scan)| scan).find(|scan| !matches!(scan, Scan::Empty));
            match first {
                Some(Scan::Awaited) => self.sought.push(sought),
                Some(Scan::Holds(path)) => {
                    let path = path.clone();
                    done.push((sought, Some(path)));
                }
                Some(Scan::Empty) | None => done.push((sought, None)),
            }
        }
        done
    }

    fn name_for(&self, path: &str) -> String {
        self.wave_at(path).map_or_else(|| self.claimable_name(&stem_of(path)), |wave| wave.name.clone())
    }

    fn absorb(&mut self, wave: Wave, path: &str) {
        if self.wave(&wave.name).is_some_and(|kept| kept.frames() > 0) {
            return;
        }
        self.keep(wave, Some(path));
    }

    pub(super) fn import_wave(&mut self, path: &str) -> Result<Json, String> {
        let bytes = std::fs::read(path).map_err(|error| format!("{path} would not read: {error}"))?;
        let wave = Wave::parse(self.name_for(path), String::from(path), &bytes).map_err(|refusal| match refusal {
            Refusal::Unreadable => format!("{path} is not a WAV the app can read"),
            Refusal::TooLong => format!("{path} is over {} seconds, the longest wave the pool takes", crate::wav::LONGEST_SECONDS),
        })?;
        let summary = self.wave(&wave.name).filter(|kept| kept.frames() > 0).map_or_else(|| wave.summary(), |kept| kept.summary());
        self.previewed.take_if(|previewed| *previewed == wave.name);
        self.absorb(wave, path);
        Ok(summary)
    }

    pub(super) fn waves_json(&self, names: &[String]) -> Vec<Json> {
        names.iter().filter_map(|name| match self.wave(name) {
            Some(wave) if wave.frames() > 0 => Some(wave.to_json_at(ENTRY_DEPTH)),
            _ => self.link(name),
        }).collect()
    }

    fn link(&self, name: &str) -> Option<Json> {
        let (_, path) = self.sources.iter().find(|(known, _)| known == name)?;
        Some(Json::Object(vec![(String::from("name"), Json::String(String::from(name))), (String::from("sourcePath"), Json::String(path.clone()))]))
    }

    pub(super) fn waves_summary(&self, names: &[String]) -> Vec<Json> {
        names.iter().filter_map(|name| self.wave(name)).filter(|wave| wave.frames() > 0).map(|wave| wave.summary()).collect()
    }

    pub(super) fn peeked(&self, path: &str) -> Option<Peek> {
        if let Some(wave) = self.wave_at(path).filter(|wave| wave.frames() > 0) {
            return Some(Peek::Wave(Arc::clone(wave)));
        }
        self.peeks.iter().find(|(known, _)| known == path).and_then(|(_, peek)| peek.clone())
    }

    fn remember_peek(&mut self, path: &str, peek: Option<Peek>) {
        self.peeks.retain(|(known, _)| known != path);
        if self.peeks.len() >= PEEKS {
            self.peeks.remove(0);
        }
        self.peeks.push((String::from(path), peek));
    }

    pub(super) fn auditioning(&self, path: &str) -> bool {
        self.audition.as_ref().is_some_and(|audition| audition.path == path)
    }

    pub(super) fn auditioned(&self) -> Option<&str> {
        self.audition.as_ref().map(|audition| audition.path.as_str())
    }

    pub(super) fn tick_audition(&mut self, seconds: f32) -> bool {
        let Some(audition) = &mut self.audition else {
            return false;
        };
        audition.left -= seconds;
        if audition.left > 0.0 {
            return false;
        }
        self.audition = None;
        true
    }
}

impl App {
    pub(super) fn recover(&mut self, name: &str, failed: Option<&str>, lost: Lost) {
        if let Some(failed) = failed {
            self.pool.tried.push((String::from(name), String::from(failed)));
        }
        match self.pool.sources.iter().find(|(known, _)| known == name).map(|(_, path)| path.clone()).filter(|path| !self.pool.tried.iter().any(|(known, tried)| known == name && tried == path)) {
            Some(path) => self.recover_from(name, path),
            None => match self.pool.seek(name, lost, self.browser.folders(), |root| self.browser.scanning(root)) {
                Ok(roots) => self.requests.extend(roots.into_iter().map(|path| Request::Scan { path })),
                Err(lost) => self.lose(String::from(name), lost),
            },
        }
    }

    fn recover_from(&mut self, name: &str, path: String) {
        self.pool.tried.push((String::from(name), path.clone()));
        self.pool.origins.retain(|(known, _)| known != name);
        self.pool.origins.push((String::from(name), path.clone()));
        self.requests.push(Request::Read { path });
    }

    fn lose(&mut self, name: String, Lost { shown, path, forget }: Lost) {
        if forget {
            self.pool.sources.retain(|(known, _)| *known != name);
        }
        self.notice = Some(Notice { band: AUDIO, name: shown, reason: UNREADABLE });
        self.pool.pending.retain(|(pending, _)| Some(pending) != path.as_ref());
        self.pool.keep(Wave::missing(name, path), None);
    }

    pub(super) fn sought(&mut self, root: &str, found: &[Found]) {
        let done = self.pool.sought(root, found);
        let missed = done.iter().any(|(_, hit)| hit.is_none());
        for (Sought { name, lost, .. }, hit) in done {
            match hit {
                _ if self.pool.wave(&name).is_some_and(|wave| wave.frames() > 0) => {}
                Some(path) => self.recover_from(&name, path),
                None => self.lose(name, lost),
            }
        }
        if missed {
            self.sync_audio();
        }
    }

    pub(super) fn reread(&mut self) {
        for (name, from) in &self.pool.origins {
            if self.pool.wave(name).is_none() && !self.pool.unread.contains(from) {
                self.pool.unread.push(from.clone());
                self.requests.push(Request::Read { path: from.clone() });
            }
        }
    }

    pub(super) fn recover_missing(&mut self) {
        let mut names: Vec<String> = self.project.tracks.iter().flat_map(|track| &track.clips).filter_map(|clip| clip.file.as_ref()).map(|file| file.name.clone()).collect();
        names.sort();
        names.dedup();
        for name in names {
            let coming = self.pool.origins.iter().any(|(known, from)| *known == name && self.pool.unread.contains(from));
            if !coming && self.pool.wave(&name).is_none() && !self.pool.tried.iter().any(|(known, _)| *known == name) {
                self.recover(&name, None, Lost { shown: name.clone(), path: None, forget: true });
            }
        }
    }

    pub(super) fn stow_made(&mut self) {
        for (file, name, song) in std::mem::take(&mut self.pool.unstowed) {
            self.stow(file, name, song);
        }
        for (at, name) in std::mem::take(&mut self.pool.made).into_iter().enumerate() {
            let file = format!("{}-{}-{at}.wav", history::now(), self.pool.kept);
            self.stow(file, name, self.disk.song);
        }
    }

    fn stow(&mut self, file: String, name: String, song: u64) {
        let held = if song == self.disk.song { self.pool.wave(&name) } else { self.disk.shelved_wave(song, &name) };
        let Some(wave) = held.filter(|wave| wave.frames() > 0 && wave.path.is_none()) else {
            return;
        };
        let bytes = wave.wav();
        self.pool.stowing.push((file.clone(), name, song));
        self.requests.push(Request::Stow { file, bytes });
    }

    pub fn stowed(&mut self, path: String, written: bool) {
        let Some(at) = self.pool.stowing.iter().position(|(file, _, _)| path.ends_with(file.as_str())) else {
            return;
        };
        let (file, name, song) = self.pool.stowing.remove(at);
        if !written {
            self.pool.unstowed.push((file, name, song));
            self.not_saved();
            return;
        }
        if song != self.disk.song {
            self.disk.stowed(song, name, path);
        } else if self.pool.wave(&name).is_some_and(|wave| wave.path.is_none()) {
            self.pool.sources.retain(|(known, _)| *known != name);
            self.pool.sources.push((name, path));
        }
        self.remember();
    }

    pub fn holds(&self, path: &str) -> bool {
        self.pool.sources.iter().any(|(_, kept)| kept == path) || self.disk.holds(path)
    }

    pub(super) fn adopt_pool(&mut self, json: Option<&Json>, origin: &str) {
        for entry in json.and_then(Json::as_array).unwrap_or(&[]) {
            let Some(name) = entry.get("name").and_then(Json::as_str) else {
                continue;
            };
            if let Some(source) = entry.get("sourcePath").and_then(Json::as_str) {
                self.pool.sources.retain(|(known, _)| known != name);
                self.pool.sources.push((String::from(name), String::from(source)));
            }
            let encoded = entry.get("pcm16Base64").is_some();
            let wave = if encoded { Wave::from_json(entry) } else { None };
            let lost = Lost { shown: String::from(name), path: None, forget: true };
            match wave {
                Some(wave) => self.pool.keep(self.disk.shared(wave), Some(origin)),
                None if !encoded => self.recover(name, None, lost),
                None => self.lose(String::from(name), lost),
            }
        }
    }

    pub(super) fn peek(&mut self, path: &str) {
        if self.pool.peeks.iter().any(|(known, _)| known == path) || self.pool.peeking.iter().any(|known| known == path) {
            return;
        }
        if let Some(wave) = self.pool.wave_at(path).filter(|wave| wave.frames() > 0).cloned() {
            self.pool.remember_peek(path, Some(Peek::Wave(wave)));
            return;
        }
        if self.browser.bytes(path).is_some_and(|bytes| bytes <= PEEK_BYTES) {
            self.pool.peeking.push(String::from(path));
            self.requests.push(Request::Read { path: String::from(path) });
        } else {
            self.pool.remember_peek(path, None);
        }
    }

    pub(super) fn peek_hovered(&mut self) {
        if self.hover != Hover::Browser {
            return;
        }
        let path = match self.browser.under() {
            Some(Under::Wave { path, .. } | Under::Midi { path, .. }) => String::from(path),
            _ => return,
        };
        self.peek(&path);
    }

    fn peek_read(&mut self, path: &str, bytes: Option<&[u8]>) -> bool {
        let Some(at) = self.pool.peeking.iter().position(|pending| pending == path) else {
            return false;
        };
        self.pool.peeking.remove(at);
        let peek = bytes.and_then(|bytes| self.glance(path, bytes));
        self.pool.remember_peek(path, peek);
        true
    }

    fn glance(&self, path: &str, bytes: &[u8]) -> Option<Peek> {
        match browser::kind(path, false)? {
            browser::Kind::Midi => Score::parse(bytes, self.project.bpm).map(|score| Peek::Score(Arc::new(score))),
            browser::Kind::Wave if bytes.len() as u64 <= PEEK_BYTES => Wave::parse(stem_of(path), String::from(path), bytes).ok().map(|wave| Peek::Wave(Arc::new(wave))),
            browser::Kind::Wave | browser::Kind::Folder | browser::Kind::Document => None,
        }
    }

    pub fn read(&mut self, path: String, bytes: Option<Vec<u8>>) {
        if let Some(kept) = self.browser.shelved(&path, bytes.as_deref()) {
            self.requests.extend(kept.map(|text| Request::Keep { path, text }));
            self.requests.extend(self.browser.unhomed().into_iter().map(|path| Request::List { path }));
            return;
        }
        self.pool.unread.retain(|known| *known != path);
        if self.disk.exporting.is_some() {
            self.pool.pending.retain(|(pending, _)| *pending != path);
            self.pool.scores.retain(|(pending, _)| *pending != path);
        }
        let peeked = self.peek_read(&path, bytes.as_deref());
        let awaited = self.pool.audition.as_mut().filter(|audition| audition.path == path && audition.awaited).map(|audition| {
            audition.awaited = false;
            audition.kind
        });
        let rehearsing = awaited == Some(browser::Kind::Midi);
        if rehearsing {
            self.rehearse(&path, bytes.as_deref());
        }
        let scores: Vec<Landing> = self.pool.scores.iter().filter(|(pending, _)| *pending == path).map(|(_, landing)| *landing).collect();
        self.pool.scores.retain(|(pending, _)| *pending != path);
        if !scores.is_empty() {
            self.score_read(&path, bytes.as_deref(), scores);
            return;
        }
        if rehearsing {
            return;
        }
        let wanted: Vec<String> = self.pool.origins.iter().filter(|(name, from)| *from == path && self.pool.wave(name).is_none()).map(|(name, _)| name.clone()).collect();
        let origin = self.pool.origins.iter().any(|(_, from)| *from == path);
        let landing = self.pool.pending.iter().any(|(pending, _)| *pending == path);
        let auditioning = awaited == Some(browser::Kind::Wave);
        if !landing && !auditioning && (!origin || (peeked && wanted.is_empty())) {
            return;
        }
        let previewing = auditioning && !landing && !origin && self.pool.wave_at(&path).is_none();
        match bytes {
            Some(bytes) if landing || auditioning || bytes.starts_with(b"RIFF") => {
                let name = wanted.into_iter().next().unwrap_or_else(|| self.pool.name_for(&path));
                match Wave::parse(name.clone(), path.clone(), &bytes) {
                    Ok(wave) => {
                        self.pool.absorb(wave, &path);
                        if previewing {
                            self.keep_one_preview(name);
                        }
                    }
                    Err(refusal) => {
                        self.notice = Some(Notice { band: AUDIO, name: stem_of(&path), reason: refused(refusal) });
                        if self.pool.wave(&name).is_some_and(|wave| wave.frames() > 0) {
                            self.pool.pending.retain(|(pending, _)| *pending != path);
                        } else {
                            self.pool.keep(Wave::missing(name, Some(path.clone())), None);
                        }
                    }
                }
            }
            Some(bytes) => {
                let pool = std::str::from_utf8(&bytes).ok().and_then(json::parse);
                let entries = pool.as_ref().and_then(|json| json.get("waves")).and_then(Json::as_array).unwrap_or(&[]);
                for name in wanted {
                    match entries.iter().find(|entry| entry.get("name").and_then(Json::as_str) == Some(name.as_str())).and_then(Wave::from_json) {
                        Some(wave) => self.pool.keep(wave, Some(&path)),
                        None => self.recover(&name, Some(&path), Lost { shown: name.clone(), path: None, forget: false }),
                    }
                }
            }
            None if previewing => self.notice = Some(Notice { band: AUDIO, name: stem_of(&path), reason: UNREADABLE }),
            None => {
                let mut names = wanted;
                if names.is_empty() {
                    names.push(self.pool.claimable_name(&stem_of(&path)));
                }
                for name in names {
                    self.recover(&name, Some(&path), Lost { shown: stem_of(&path), path: Some(path.clone()), forget: false });
                }
            }
        }
        if let Some(name) = self.pool.wave_at(&path).map(|wave| wave.name.clone()) {
            let landings: Vec<Landing> = self.pool.pending.iter().filter(|(pending, _)| *pending == path).map(|(_, landing)| *landing).collect();
            self.pool.pending.retain(|(pending, _)| *pending != path);
            for landing in landings {
                self.land(landing, &name);
            }
        }
        if auditioning {
            self.start_audition();
        }
        self.sync_audio();
    }

    fn keep_one_preview(&mut self, name: String) {
        if let Some(last) = self.pool.previewed.take().filter(|last| *last != name && !self.wave_used(last)) {
            self.pool.forget(&last);
        }
        self.pool.previewed = Some(name);
    }

    pub(super) fn drop_glances(&mut self) {
        self.pool.peeks.clear();
        if let Some(last) = self.pool.previewed.take().filter(|last| !self.wave_used(last)) {
            self.pool.forget(&last);
        }
    }

    fn wave_used(&self, name: &str) -> bool {
        let named = |clip: &Clip| clip.file.as_ref().is_some_and(|file| file.name == name);
        self.project.tracks.iter().flat_map(|track| &track.clips).any(named) || self.clipboard.clips().into_iter().any(named) || self.song_names().iter().any(|used| used == name)
    }

    pub(super) fn audition(&mut self, path: String, kind: browser::Kind, toggle: bool) {
        if toggle && self.pool.auditioning(&path) {
            self.stop_audition();
            return;
        }
        if let Some(playing) = self.pool.audition.take()
            && (playing.kind, kind) != (browser::Kind::Wave, browser::Kind::Wave)
        {
            self.silence(playing.kind);
        }
        let cached = kind == browser::Kind::Wave && self.pool.wave_at(&path).is_some_and(|wave| wave.frames() > 0);
        self.pool.audition = Some(Audition { path: path.clone(), kind, left: f32::INFINITY, awaited: !cached });
        if cached {
            self.start_audition();
        } else if !self.pool.peeking.contains(&path) {
            self.requests.push(Request::Read { path });
        }
    }

    pub(super) fn stop_audition(&mut self) {
        if let Some(playing) = self.pool.audition.take() {
            self.silence(playing.kind);
        }
    }

    fn silence(&self, kind: browser::Kind) {
        let Some(audio) = &self.audio else {
            return;
        };
        match kind {
            browser::Kind::Wave => audio.preview(None),
            browser::Kind::Midi => audio.rehearse(None),
            browser::Kind::Folder | browser::Kind::Document => {}
        }
    }

    pub(super) fn start_audition(&mut self) {
        let wave = self.pool.audition.as_ref().and_then(|audition| self.pool.wave_at(&audition.path)).cloned();
        let Some(wave) = wave.filter(|wave| wave.frames() > 0) else {
            self.stop_audition();
            return;
        };
        let once = wave.seconds() < ONE_SHOT_SECONDS;
        if once && let Some(audition) = &mut self.pool.audition {
            audition.left = wave.seconds() as f32;
        }
        let bpm = self.project.bpm;
        let tempo = if once { None } else { warp::tempo_of(&stem_of_wave(&wave), wave.seconds(), bpm) };
        if let Some(audio) = &mut self.audio {
            let warp = tempo.map(|tempo| Warped { beats: audio.beats_of(&wave), ratio: tempo / bpm, mode: warp::Loop::Rest, tau: warp::decay_frames(ENVELOPE_MAX, wave.rate) });
            audio.preview(Some(Box::new(audio::Cue { wave, warp, once })));
        }
    }

    fn rehearse(&mut self, path: &str, bytes: Option<&[u8]>) {
        let score = bytes.and_then(|bytes| Score::parse(bytes, self.project.bpm)).map(Arc::new);
        self.pool.remember_peek(path, score.clone().map(Peek::Score));
        let Some(score) = score else {
            self.pool.audition = None;
            return;
        };
        if let Some(audio) = &self.audio {
            let tracks = self.project.leading(self.selection.picked_tracks());
            audio.rehearse(Some(Box::new(audio::Rehearsal { tracks, notes: score.notes.clone(), beats: score.beats() })));
        }
    }

    fn landing_below(&self) -> Landing {
        Landing { track: None, at: self.project.tracks.len(), joins: false, tick: self.play_from }
    }

    fn checked(&self, landing: Landing, kind: browser::Kind) -> Landing {
        let taken = landing.track.is_none_or(|track| self.project.tracks.get(track).is_some_and(|track| takes(track, kind)));
        if taken { landing } else { Landing { tick: landing.tick, ..self.landing_below() } }
    }

    pub(super) fn file_dropped(&mut self, path: String, kind: browser::Kind, x: i32, y: i32) {
        let landing = self.landing_at(x, y, kind).unwrap_or_else(|| self.landing_below());
        self.place(path, kind, landing);
    }

    pub(super) fn place_from_browser(&mut self, path: String, kind: browser::Kind, dropped: bool, at: Option<(i32, i32)>) {
        let landing = at.and_then(|(x, y)| self.landing_at(x, y, kind));
        match landing {
            Some(landing) => self.place(path, kind, landing),
            None if !dropped => self.place(path, kind, self.landing_below()),
            None => {}
        }
    }

    fn place(&mut self, path: String, kind: browser::Kind, landing: Landing) {
        match kind {
            browser::Kind::Wave => self.place_wave(path, landing),
            browser::Kind::Midi => {
                self.pool.scores.push((path.clone(), landing));
                self.requests.push(Request::Read { path });
            }
            browser::Kind::Folder | browser::Kind::Document => {}
        }
    }

    fn place_wave(&mut self, path: String, landing: Landing) {
        if let Some(name) = self.pool.wave_at(&path).filter(|wave| wave.frames() > 0).map(|wave| wave.name.clone()) {
            self.land(landing, &name);
            return;
        }
        self.pool.pending.push((path.clone(), landing));
        self.requests.push(Request::Read { path });
    }

    fn score_read(&mut self, path: &str, bytes: Option<&[u8]>, landings: Vec<Landing>) {
        let stem = stem_of(path);
        match bytes.and_then(|bytes| midi::parse(bytes, self.project.bpm)) {
            None => self.notice = Some(Notice { band: MIDI, name: stem, reason: UNREADABLE }),
            Some(parts) if parts.is_empty() => self.notice = Some(Notice { band: MIDI, name: stem, reason: NO_NOTES }),
            Some(parts) => {
                for landing in landings {
                    self.land_score(landing, &parts, &stem);
                }
            }
        }
    }

    fn land_score(&mut self, landing: Landing, parts: &[midi::Part], stem: &str) {
        let landing = self.checked(landing, browser::Kind::Midi);
        let start = landing.tick.max(0);
        if start >= LONGEST_TICKS {
            return;
        }
        let clip = |name: String, part: &midi::Part| Clip::midi(start, name, part.notes.clone(), part.length);
        let at = landing.at.min(self.project.tracks.len());
        let mut parts = parts.iter();
        let mut placed = Vec::new();
        if let Some(track) = landing.track
            && let Some(part) = parts.next()
        {
            placed.push((track, clip(String::from(stem), part)));
        }
        for (opened, part) in parts.enumerate() {
            if !self.insert_track(at + opened, Track::blank(), landing.joins) {
                break;
            }
            placed.push((at + opened, clip(part.label(stem), part)));
        }
        if placed.is_empty() {
            return;
        }
        if landing.track.is_none() {
            self.select(at);
        }
        self.selection.clips = placed.iter().map(|(track, clip)| (*track, clip.start)).collect();
        let bpm = self.project.bpm;
        for (track, clip) in placed {
            self.project.tracks[track].place(clip, bpm);
        }
        self.finish_edit();
    }

    pub(super) fn land(&mut self, landing: Landing, name: &str) {
        let landing = self.checked(landing, browser::Kind::Wave);
        let Some(wave) = self.pool.wave(name).cloned() else {
            return;
        };
        if wave.frames() == 0 {
            return;
        }
        let bpm = self.project.bpm;
        let tempo = warp::tempo_of(&stem_of_wave(&wave), wave.seconds(), bpm);
        let length = match tempo {
            Some(tempo) => ticks(wave.seconds() * tempo / 60.0).max(1),
            None => (wave.seconds() * bpm / 60.0 * f64::from(TICKS_PER_BEAT)).ceil().max(1.0) as i32,
        };
        let mut clip = Clip::audio(landing.tick.max(0), length, String::from(name), String::from(name));
        if clip.start > LONGEST_TICKS - clip.length {
            return;
        }
        if let (Some(tempo), Some(file)) = (tempo, &mut clip.file) {
            file.tempo = tempo;
            file.warp = true;
        }
        let track = match landing.track {
            Some(track) => track,
            None => {
                let at = landing.at.min(self.project.tracks.len());
                if !self.insert_track(at, Track::audio(), landing.joins) {
                    return;
                }
                self.select(at);
                at
            }
        };
        let start = clip.start;
        let bpm = self.project.bpm;
        self.project.tracks[track].place(clip, bpm);
        self.selection.clips = vec![(track, start)];
        self.finish_edit();
    }

    pub(super) fn track_under(&self, x: i32, y: i32) -> Option<usize> {
        let viewport = self.track_viewport();
        if self.browser_big() || !viewport.contains(x, y) {
            return None;
        }
        let row = |slot: usize| {
            let tile = self.track_tile(slot);
            Rect::new(0, tile.y, self.width, tile.h).intersect(viewport)
        };
        self.track_slots().into_iter().find(|&(slot, _)| row(slot).contains(x, y)).map(|(_, track)| track)
    }

    pub(super) fn landing_at(&self, x: i32, y: i32, kind: browser::Kind) -> Option<Landing> {
        if self.browser_big() || !self.track_viewport().contains(x, y) {
            return None;
        }
        let lane = self.lane(self.track_tile(0));
        let tick = if x >= lane.rect.x { lane.snapped(x).max(0) } else { self.play_from };
        let under = self.track_under(x, y);
        let track = under.filter(|&track| takes(&self.project.tracks[track], kind));
        let (at, joins) = match under {
            Some(group) if self.project.tracks[group].group && self.project.tracks[group].folded => (group + self.project.members_of(group).len() + 1, false),
            Some(track) => (track + 1, true),
            None => (self.project.tracks.len(), false),
        };
        Some(Landing { track, at, joins, tick })
    }
}

pub(super) fn numbered(stem: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(stem) {
        return String::from(stem);
    }
    let mut count = 2;
    loop {
        let name = format!("{stem} {count}");
        if !taken(&name) {
            return name;
        }
        count += 1;
    }
}

pub(super) fn wave_file(path: &str) -> bool {
    browser::kind(path, false) == Some(browser::Kind::Wave)
}

fn stem_of_wave(wave: &Wave) -> String {
    wave.path.as_deref().map_or_else(|| wave.name.clone(), stem_of)
}

fn takes(track: &Track, kind: browser::Kind) -> bool {
    match kind {
        browser::Kind::Wave => track.audio,
        browser::Kind::Midi => track.generates(),
        browser::Kind::Folder | browser::Kind::Document => false,
    }
}
