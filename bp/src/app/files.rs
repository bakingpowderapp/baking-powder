use super::*;
use super::settings::EDITS;
use crate::factory;
use crate::wav;

pub(super) const SAVED_SECONDS: f32 = 1.5;
const FAILED_SECONDS: f32 = 4.0;
const APP_NAME: &str = "Baking Powder";
const EXPORT: &str = "Export";
const EXPORT_LOOP: &str = "Export Loop";
const SAVED: &str = "SAVED!";
const DEFAULT_SAVED: &str = "DEFAULT SAVED!";
const EXPORTED: &str = "EXPORTED!";
const NOT_EXPORTED: &str = "NOT EXPORTED!";
const CLEARED: &str = "CLEARED!";
const NOT_SAVED: &str = "NOT SAVED!";
const NOT_CLEARED: &str = "NOT CLEARED!";
const NOT_WRITTEN: &str = "NOT WRITTEN!";
const EXISTS: &str = "EXISTS";
const RENAME: &str = "RENAME";
const FACTORY: &str = "FACTORY";
const BUILT_IN: &str = "BUILT IN";
const OPEN_PROJECT: &str = "OPEN PROJECT";
const OPEN_DESC: &str = "Replaces the project you have; Undo brings it back";
const OPEN: &str = "OPEN";
const REMOVE_FOLDER: &str = "REMOVE FOLDER";
const REMOVE_DESC: &str = "Leaves the browser; nothing on disk changes";
const REMOVE: &str = "REMOVE";

pub(super) const VERSIONS: usize = 50;
const RECENT: usize = 5;
pub(super) const EXPORT_AUDIO: &str = "Export Audio";
pub(super) const EXPORTING: &str = "EXPORTING";
const BOUNCING: &str = "BOUNCING";
pub(super) const FLATTENING: &str = "FLATTENING";
const FLATTEN_SILENCE: f32 = 1e-3;
const BOUNCE: &str = "Bounce";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum FileAction {
    NewProject,
    NewDefaultProject,
    OpenProject,
    OpenRecent,
    SaveProject,
    SaveAs,
    SaveAsDefault,
    ClearDefault,
    ExportProject,
    ExportAudio,
    Settings,
}

pub(super) const FILE_ITEMS: [(FileAction, &str); 11] = [
    (FileAction::NewProject, "New Project"),
    (FileAction::NewDefaultProject, "New Default Project"),
    (FileAction::OpenProject, "Open Project"),
    (FileAction::OpenRecent, "Open Recent"),
    (FileAction::SaveProject, "Save Project"),
    (FileAction::SaveAs, "Save As"),
    (FileAction::SaveAsDefault, "Save As Default"),
    (FileAction::ClearDefault, "Clear Default"),
    (FileAction::ExportProject, "Export Project"),
    (FileAction::ExportAudio, EXPORT_AUDIO),
    (FileAction::Settings, "Settings"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Export {
    History,
    Waves,
    Bare,
    Manual,
}

pub(super) const EXPORT_ITEMS: [(Export, &str); 4] = [
    (Export::History, "Without Waves"),
    (Export::Waves, "Without History"),
    (Export::Bare, "Without Both"),
    (Export::Manual, "Manual"),
];

const VERSIONS_MANUAL: &str = "a save file's snapshots of the document, each with savedAt; an export may drop them";
const WAVES_MANUAL: &str = "wave pool: name, sampleRateHz, channels, sourcePath, pcm16Base64. Unloaded waves keep name/sourcePath; reads and audio-free exports give seconds in place of samples. Missing samples load from sourcePath or a matching WAV in browser folders; resaving embeds them.";
const FOLDERS_MANUAL: &str = "absolute paths of the browser's folders, in order; left out for none";
pub(super) type Fresh = fn() -> Track;
pub(super) const ADD_ITEMS: [(Fresh, &str); 2] = [(Track::blank, MIDI), (Track::audio, AUDIO)];
pub(super) const FOLLOW: &str = "FOLLOW";
pub(super) const DELAY: &str = "DELAY";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Saved {
    Project,
    Default,
    Effect(&'static str),
    Cleared,
    Failed,
    DefaultFailed,
    ClearFailed,
    FactoryFailed,
    Exported,
    ExportFailed,
}

pub(super) enum Ask {
    Open { path: String, name: String },
    Forget { path: String, name: String },
}

impl Ask {
    pub(super) fn name(&self) -> &str {
        match self {
            Ask::Open { name, .. } | Ask::Forget { name, .. } => name,
        }
    }

    pub(super) fn words(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            Ask::Open { .. } => (OPEN_PROJECT, OPEN_DESC, OPEN),
            Ask::Forget { .. } => (REMOVE_FOLDER, REMOVE_DESC, REMOVE),
        }
    }
}

pub(super) struct Flattening {
    head: usize,
    start: i32,
    song: i32,
}

pub(super) enum Bound {
    File(String),
    Track { at: usize, start: i32, length: i32 },
    Flatten { flattening: Flattening, rest: Vec<Flattening>, total: usize, unflattened: Box<Project> },
}

fn mix_lane(aim: &Aim) -> bool {
    matches!(aim, Aim::Gain | Aim::Send(_))
}

fn mix_lanes(automation: &Automation) -> Automation {
    Automation { curves: automation.curves.iter().filter(|curve| mix_lane(&curve.aim)).cloned().collect(), lfos: automation.lfos.iter().filter(|lfo| mix_lane(&lfo.aim)).cloned().collect() }
}

fn strip_mix_lanes(automation: &mut Automation) {
    automation.curves.retain(|curve| !mix_lane(&curve.aim));
    automation.lfos.retain(|lfo| !mix_lane(&lfo.aim));
}

fn trimmed(wave: Wave, least: usize) -> Wave {
    let floor = wav::quantized(FLATTEN_SILENCE).unsigned_abs();
    let channels = wave.channels.max(1);
    let sounding = wave.pcm.iter().rposition(|sample| sample.unsigned_abs() > floor).map_or(0, |at| at / channels + 1);
    let frames = sounding.max(least).min(wave.frames());
    let Wave { name, rate, mut pcm, .. } = wave;
    pcm.truncate(frames * channels);
    Wave::sound(name, rate, channels, pcm)
}

pub(super) enum Rendering {
    Sound(Bound, audio::Export),
    Signal(Host, usize, audio::duck::Tracing),
}

impl Rendering {
    pub(super) fn progress(&self) -> f32 {
        match self {
            Rendering::Sound(_, export) => export.progress(),
            Rendering::Signal(_, _, tracing) => tracing.progress(),
        }
    }

    fn finished(&self) -> bool {
        match self {
            Rendering::Sound(_, export) => export.finished(),
            Rendering::Signal(_, _, tracing) => tracing.finished(),
        }
    }

    fn cancel(&self) {
        match self {
            Rendering::Sound(_, export) => export.cancel(),
            Rendering::Signal(_, _, tracing) => tracing.cancel(),
        }
    }
}

pub(super) struct Exporting {
    pub(super) rendering: Rendering,
    pub(super) name: String,
    pub(super) title: String,
    pub(super) age: f32,
}

pub(super) struct Version {
    pub(super) at: u64,
    pub(super) tracks: usize,
    pub(super) length: i32,
    document: Json,
}

pub(super) struct Spelled {
    key: u64,
    text: Arc<str>,
    names: Vec<String>,
}

pub(super) struct Saving {
    save: u64,
    sure: bool,
    fresh: bool,
    versions: Vec<Version>,
    written: Vec<String>,
}

#[derive(Default)]
struct Shelved {
    song: u64,
    waves: Vec<Arc<Wave>>,
    origins: Vec<(String, String)>,
    sources: Vec<(String, String)>,
    versions: Vec<Version>,
    spelled: Option<Arc<str>>,
}

pub(super) struct Disk {
    pub(super) path: Option<String>,
    pub(super) song: u64,
    songs: u64,
    carried: Shelved,
    shelf: Vec<Shelved>,
    saves: u64,
    saving: Vec<Saving>,
    pub(super) recent: Vec<String>,
    pub(super) versions: Vec<Version>,
    versions_changed: u64,
    edits_spelled: Option<Spelled>,
    versions_spelled: Option<Spelled>,
    pub(super) remembered: String,
    pub(super) exporting: Option<Exporting>,
    export_stem: Option<String>,
    pub(super) saved_note: Option<(Saved, f32)>,
}

impl Version {
    pub(super) fn to_json(&self) -> Json {
        Json::Object(vec![(String::from("savedAt"), Json::Number(self.at as f64)), (String::from("project"), self.document.clone())])
    }

    pub(super) fn from_json(json: &Json) -> Option<Version> {
        let document = json.get("project")?.clone();
        let project = Project::from_json(&document)?;
        Some(Version { at: json.get("savedAt").and_then(Json::as_f64).unwrap_or(0.0).max(0.0) as u64, tracks: project.tracks.len(), length: project.end(), document })
    }
}

impl Shelved {
    fn from_json(json: &Json) -> Option<Shelved> {
        let mut shelved = Shelved { song: json.get("song")?.as_f64()?.max(0.0) as u64, origins: pairs(json, "origins", "from"), sources: pairs(json, "sources", "path"), versions: versions_from(json.get("versions")), ..Shelved::default() };
        shelved.spell();
        Some(shelved)
    }

    fn spell(&mut self) {
        let mut fields = Fields::new();
        fields.lead("song", Json::Number(self.song as f64), true);
        fields.list("origins", spell_pairs(&self.origins, "from"));
        fields.list("sources", spell_pairs(&self.sources, "path"));
        fields.list("versions", self.versions.iter().map(Version::to_json).collect());
        self.spelled = fields.into_object().map(|shelved| shelved.write_at(2).into());
    }
}

impl Disk {
    pub(super) fn new() -> Disk {
        Disk { path: None, song: 0, songs: 0, carried: Shelved::default(), shelf: Vec::new(), saves: 0, saving: Vec::new(), recent: Vec::new(), versions: Vec::new(), versions_changed: 0, edits_spelled: None, versions_spelled: None, remembered: String::new(), exporting: None, export_stem: None, saved_note: None }
    }

    pub(super) fn replace(&mut self, path: Option<String>) {
        self.path = path;
        self.saving.clear();
    }

    pub(super) fn reset_versions(&mut self, json: Option<&Json>) {
        self.versions = versions_from(json);
        self.versions_changed += 1;
    }

    pub(super) fn resume(&mut self, json: &Json, history: &History) {
        self.song = history.recent(1).first().map_or(0, |step| step.song);
        self.songs = history.every().map(|step| step.song).max().unwrap_or(0);
        self.shelf = items(Some(json), "shelved").iter().filter_map(Shelved::from_json).collect();
        self.reset_versions(json.get("versions"));
    }

    pub(super) fn prune(&mut self, history: &History) {
        self.shelf.retain(|shelved| history.every().any(|step| step.song == shelved.song));
    }

    pub(super) fn shared(&self, wave: Wave) -> Arc<Wave> {
        let held = self.shelf.iter().flat_map(|shelved| &shelved.waves).find(|held| held.name == wave.name && held.path == wave.path && held.same_sound(&wave));
        held.cloned().unwrap_or_else(|| Arc::new(wave))
    }

    pub(super) fn holds(&self, path: &str) -> bool {
        self.shelf.iter().flat_map(|shelved| &shelved.sources).any(|(_, kept)| kept == path)
    }

    pub(super) fn shelved_wave(&self, song: u64, name: &str) -> Option<&Arc<Wave>> {
        self.shelf.iter().find(|shelved| shelved.song == song)?.waves.iter().find(|wave| wave.name == name)
    }

    pub(super) fn stowed(&mut self, song: u64, name: String, path: String) {
        if let Some(shelved) = self.shelf.iter_mut().find(|shelved| shelved.song == song) {
            shelved.sources.retain(|(known, _)| *known != name);
            shelved.sources.push((name, path));
            shelved.spell();
        }
    }

    pub(super) fn shelved(&self) -> Vec<Json> {
        self.shelf.iter().filter_map(|shelved| shelved.spelled.clone()).map(Json::Raw).collect()
    }

    pub(super) fn versions_spelled(&mut self) -> (Arc<str>, Vec<String>) {
        let key = self.versions_changed;
        if self.versions_spelled.as_ref().is_none_or(|spelled| spelled.key != key) {
            self.versions_spelled = None;
        }
        let versions = &self.versions;
        let spelled = self.versions_spelled.get_or_insert_with(|| {
            let mut names = Vec::new();
            for version in versions {
                wave_names(&version.document, &mut names);
            }
            Spelled { key, text: Json::Array(versions.iter().map(Version::to_json).collect()).write_at(1).into(), names }
        });
        (spelled.text.clone(), spelled.names.clone())
    }

    pub(super) fn recall(&mut self) {
        if let Some(path) = self.path.clone() {
            self.recent.retain(|known| *known != path);
            self.recent.insert(0, path);
        }
        self.recent.truncate(RECENT);
    }

    pub(super) fn stem(&self) -> Option<&str> {
        self.path.as_deref().and_then(|path| std::path::Path::new(path).file_stem()).and_then(|stem| stem.to_str())
    }

    pub(super) fn export_name(&self) -> String {
        match self.stem() {
            Some(stem) => format!("{stem}.wav"),
            None => format!("{APP_NAME} {EXPORT}.wav"),
        }
    }

    pub(super) fn export_json_name(&self) -> String {
        format!("{} {EXPORT}.json", self.stem().unwrap_or(APP_NAME))
    }

    pub(super) fn saved_text(&self, room: i32) -> String {
        let (name, word) = match self.saved_note.map(|(saved, _)| saved) {
            Some(Saved::Default) => (Some(DEFAULT), SAVED),
            Some(Saved::Effect(label)) => (Some(label), DEFAULT_SAVED),
            Some(Saved::Cleared) => (Some(DEFAULT), CLEARED),
            Some(Saved::Failed) => (self.stem(), NOT_SAVED),
            Some(Saved::DefaultFailed) => (Some(DEFAULT), NOT_SAVED),
            Some(Saved::ClearFailed) => (Some(DEFAULT), NOT_CLEARED),
            Some(Saved::FactoryFailed) => (Some(FACTORY), NOT_WRITTEN),
            Some(Saved::Exported) => (self.export_stem.as_deref(), EXPORTED),
            Some(Saved::ExportFailed) => (self.export_stem.as_deref(), NOT_EXPORTED),
            _ => (self.stem(), SAVED),
        };
        let spare = font::glyphs_in(room, TEXT_SCALE).saturating_sub(word.chars().count() + 1);
        match name {
            Some(name) if spare > 0 => format!("{} {word}", font::shortened(name, spare)),
            _ => word.to_string(),
        }
    }

    pub(super) fn linger_saved(&mut self, seconds: f32) -> bool {
        let Some((saved, left)) = self.saved_note else {
            return false;
        };
        self.saved_note = (left > seconds).then_some((saved, left - seconds));
        true
    }
}

impl App {
    pub fn opened(&mut self, path: String, text: Option<&str>, at: Option<(i32, i32)>) -> Opened {
        if self.disk.exporting.is_some() {
            return Opened::Refused;
        }
        let name = std::path::Path::new(&path).file_name().and_then(|name| name.to_str()).map_or_else(|| path.clone(), String::from);
        let Some(json) = text.and_then(json::parse) else {
            self.disk.recent.retain(|known| *known != path);
            self.remember();
            return self.refuse(name, UNREADABLE);
        };
        match Document::from_json(&json) {
            Some(Document::Project(project)) => {
                self.next_song();
                self.adopt_pool(json.get("waves"), &path);
                self.replace_project(project, Some(path));
                self.transport_from_json(&json);
                self.recover_missing();
                self.follow_playhead();
                self.disk.reset_versions(json.get("versions"));
                self.commit();
                self.disk.recall();
                self.remember();
                self.sync_audio();
                Opened::Project
            }
            Some(Document::Preset(preset)) => {
                let stem = std::path::Path::new(&path).file_stem().and_then(|stem| stem.to_str()).unwrap_or(&name);
                let description = json.get("description").and_then(Json::as_str).unwrap_or("");
                if let Some(denial) = self.land_preset(stem, description, preset, at) {
                    return self.refuse(turned_away(&name, denial), denial.reason());
                }
                self.finish_edit();
                Opened::Preset
            }
            None => self.refuse(name, UNREADABLE),
        }
    }

    pub(super) fn ask_open(&mut self, path: String, name: String) {
        if self.disk.path.is_none() && self.project == self.fresh {
            self.requests.push(Request::Open { path: Some(path) });
        } else {
            self.ask = Some(Ask::Open { path, name });
        }
    }

    pub(super) fn answer(&mut self, yes: bool) {
        match self.ask.take() {
            Some(Ask::Open { path, .. }) if yes => self.requests.push(Request::Open { path: Some(path) }),
            Some(Ask::Forget { path, .. }) if yes => {
                self.browser.forget(&path);
                self.scan_browser();
                self.remember();
            }
            _ => {}
        }
    }

    pub(super) fn refuse(&mut self, name: String, reason: &'static str) -> Opened {
        self.notice = Some(Notice { band: LOAD, name, reason });
        Opened::Refused
    }

    pub(super) fn edits_spelled(&mut self) -> (Arc<str>, Vec<String>) {
        let key = self.history.changes();
        if self.disk.edits_spelled.as_ref().is_none_or(|spelled| spelled.key != key) {
            self.disk.edits_spelled = None;
        }
        let spelled = self.disk.edits_spelled.get_or_insert_with(|| spell_edits(&self.history, key));
        (spelled.text.clone(), spelled.names.clone())
    }

    pub fn saved(&mut self, path: String, save: u64) {
        let Some(at) = self.disk.saving.iter().position(|saving| saving.save == save) else {
            return;
        };
        let Some(saving) = self.disk.saving.drain(..=at).next_back() else {
            return;
        };
        if saving.fresh {
            self.disk.versions.clear();
            self.settings.versions_scroll = 0;
        }
        for version in saving.versions {
            if self.disk.versions.last().is_none_or(|last| last.document != version.document) {
                self.disk.versions.push(version);
            }
        }
        self.disk.versions.drain(..self.disk.versions.len().saturating_sub(VERSIONS));
        self.disk.versions_changed += 1;
        self.history.rename(self.disk.song, &path);
        self.pool.origins.retain(|(name, _)| !saving.written.contains(name));
        self.pool.origins.extend(saving.written.iter().map(|name| (name.clone(), path.clone())));
        self.disk.path = Some(path);
        self.disk.saved_note = Some((Saved::Project, SAVED_SECONDS));
        self.disk.recall();
        self.remember();
    }

    pub fn dropped(&mut self, path: String, kind: browser::Kind, x: i32, y: i32) -> Dropped {
        match kind {
            browser::Kind::Folder => {
                self.add_folder(path);
                Dropped::Kept
            }
            browser::Kind::Wave | browser::Kind::Midi | browser::Kind::Document if self.browser_big() => Dropped::Read,
            browser::Kind::Wave | browser::Kind::Midi if self.disk.exporting.is_some() => Dropped::Read,
            browser::Kind::Wave => {
                self.file_dropped(path, kind, x, y);
                Dropped::Kept
            }
            browser::Kind::Midi => {
                self.file_dropped(path, kind, x, y);
                Dropped::Read
            }
            browser::Kind::Document => Dropped::Document,
        }
    }

    pub fn add_folder(&mut self, path: String) {
        self.browser.add(path.clone());
        self.set_aside(Aside::Browser);
        self.requests.push(Request::List { path });
        self.scan_browser();
        self.remember();
    }

    pub fn factory(&mut self, path: String) {
        self.factory = Some(path);
        self.show_factory();
    }

    pub(super) fn factory_folders(&self) -> Vec<String> {
        self.factory.iter().flat_map(|root| factory::FOLDERS.iter().map(move |folder| format!("{root}/{folder}"))).collect()
    }

    pub(super) fn show_factory(&mut self) {
        for (at, path) in self.factory_folders().into_iter().enumerate() {
            if !self.settings.factory {
                self.browser.unpin(&path);
            } else if self.browser.pin(at, path.clone()) {
                self.requests.push(Request::List { path });
            }
        }
        self.scan_browser();
    }

    pub fn listed(&mut self, path: &str, found: Vec<Found>) {
        if self.pool.auditioned().is_some_and(|playing| browser::parent_of(playing) == path && !found.iter().any(|found| found.path == playing)) {
            self.stop_audition();
            self.drop_glances();
        }
        self.browser.listed(path, found);
        self.keep_hearts();
        for path in self.browser.unlisted() {
            self.requests.push(Request::List { path });
        }
    }

    pub fn scanned(&mut self, root: String, found: Vec<Found>) {
        self.sought(&root, &found);
        self.browser.scanned(&root, found);
    }

    pub(super) fn scan_browser(&mut self) {
        for path in self.browser.scans() {
            self.requests.push(Request::Scan { path });
        }
        for path in self.browser.unread() {
            self.requests.push(Request::Read { path });
        }
    }

    pub(super) fn keep_hearts(&mut self) {
        for path in self.browser.owing() {
            self.requests.push(Request::Read { path });
        }
    }

    pub(super) fn refresh(&mut self) {
        for path in self.browser.unfolded().into_iter().chain(self.browser.homes()) {
            self.requests.push(Request::List { path });
        }
        self.scan_browser();
    }

    fn renew(&mut self) {
        let mut folders = self.browser.renew();
        folders.extend(self.browser.unfolded());
        folders.extend(self.browser.homes());
        folders.extend(self.pool.auditioned().map(browser::parent_of));
        folders.sort();
        folders.dedup();
        self.requests.extend(folders.into_iter().map(|path| Request::List { path }));
        self.drop_glances();
        self.scan_browser();
    }

    pub(super) fn browser_act(&mut self, action: browser::Action) {
        match action {
            browser::Action::Add => self.requests.push(Request::Folder { at: None }),
            browser::Action::Search => self.scan_browser(),
            browser::Action::List(path) => self.requests.push(Request::List { path }),
            browser::Action::Lists(paths) => self.requests.extend(paths.into_iter().map(|path| Request::List { path })),
            browser::Action::Load { .. } if self.browser_big() => {}
            browser::Action::Load { name, description, preset, .. } => self.load_preset(name, description, preset, None),
            browser::Action::Open { path, name } => self.ask_open(path, name),
            browser::Action::Rename { path, name, parent } => {
                self.requests.push(Request::Rename { path, name });
                self.requests.push(Request::List { path: parent });
            }
            browser::Action::Delete { path, parent } => {
                self.requests.push(Request::Delete { path });
                self.requests.push(Request::List { path: parent });
            }
            browser::Action::Forget { path, name } => self.ask = Some(Ask::Forget { path, name }),
            browser::Action::Reveal(path) => self.requests.push(Request::Reveal { path }),
            browser::Action::Copy(text) => self.requests.push(Request::Copy { text }),
            browser::Action::Exists(name) => self.notice = Some(Notice { band: RENAME, name, reason: EXISTS }),
            browser::Action::Kept(name) => self.notice = Some(Notice { band: FACTORY, name, reason: BUILT_IN }),
            browser::Action::Place { path, kind, dropped } => self.place_from_browser(path, kind, dropped, None),
            browser::Action::Preview { path, kind, toggle } => self.audition(path, kind, toggle),
            browser::Action::Refresh => self.renew(),
            browser::Action::Big => {
                self.view.browser_big = !self.view.browser_big;
                self.lay_browser();
            }
            browser::Action::Locate => {
                let lists = self.pool.auditioned().map(|path| self.browser.locate(path)).unwrap_or_default();
                self.requests.extend(lists.into_iter().map(|path| Request::List { path }));
            }
        }
        self.keep_hearts();
    }

    pub(super) fn file_json(&mut self, fresh: bool) -> (String, u64) {
        self.disk.saves += 1;
        let document = self.project.to_json();
        let landing = self.disk.saving.iter().filter(|saving| saving.sure).flat_map(|saving| saving.versions.iter());
        let lineage: Vec<&Version> = if fresh { Vec::new() } else { self.disk.versions.iter().chain(landing).collect() };
        let changed = lineage.last().is_none_or(|last| last.document != document);
        let version = changed.then(|| Version { at: history::now(), tracks: self.project.tracks.len(), length: self.project.end(), document: document.clone() });
        let (json, written) = {
            let all: Vec<&Version> = lineage.iter().copied().chain(&version).collect();
            self.assemble(self.with_state(document), &all[all.len().saturating_sub(VERSIONS)..], true)
        };
        self.disk.saving.push(Saving { save: self.disk.saves, sure: !fresh, fresh, versions: version.into_iter().collect(), written });
        (json, self.disk.saves)
    }

    pub(super) fn copy_json(&self) -> String {
        let document = self.project.to_json();
        let version = Version { at: history::now(), tracks: self.project.tracks.len(), length: self.project.end(), document: document.clone() };
        self.assemble(self.with_state(document), &[&version], true).0
    }

    pub fn assured(&mut self, save: u64) {
        if let Some(saving) = self.disk.saving.iter_mut().find(|saving| saving.save == save) {
            saving.sure = true;
        }
    }

    pub fn unsaved(&mut self, save: u64) {
        let Some(at) = self.disk.saving.iter().position(|saving| saving.save == save) else {
            return;
        };
        let gone = self.disk.saving.remove(at);
        if gone.sure && let Some(next) = self.disk.saving.get_mut(at) && !next.fresh {
            next.versions.splice(0..0, gone.versions);
        }
    }

    pub fn unwritten(&mut self, save: u64) {
        self.unsaved(save);
        self.not_saved();
    }

    pub fn not_saved(&mut self) {
        self.disk.saved_note = Some((Saved::Failed, FAILED_SECONDS));
    }

    pub fn default_not_saved(&mut self, cleared: bool) {
        self.disk.saved_note = Some((if cleared { Saved::ClearFailed } else { Saved::DefaultFailed }, FAILED_SECONDS));
    }

    pub fn factory_not_written(&mut self) {
        self.disk.saved_note = Some((Saved::FactoryFailed, FAILED_SECONDS));
    }

    pub(super) fn export(&mut self, export: Export) {
        let (history, waves) = match export {
            Export::History => (true, false),
            Export::Waves => (false, true),
            Export::Bare => (false, false),
            Export::Manual => {
                self.requests.push(Request::Preset { name: format!("{APP_NAME} Manual.json"), json: manual().write() });
                return;
            }
        };
        let json = {
            let versions: Vec<&Version> = if history { self.disk.versions.iter().collect() } else { Vec::new() };
            self.assemble(self.with_state(self.project.to_json()), &versions, waves).0
        };
        self.requests.push(Request::Preset { name: self.disk.export_json_name(), json });
    }

    pub(super) fn with_state(&self, document: Json) -> Json {
        let mut state = Fields::new();
        self.transport_fields(&mut state);
        before_tracks(document, state)
    }

    fn assemble(&self, mut document: Json, versions: &[&Version], waves: bool) -> (String, Vec<String>) {
        let mut names = Vec::new();
        wave_names(&document, &mut names);
        for version in versions {
            wave_names(&version.document, &mut names);
        }
        let pool = if waves { self.pool.waves_json(&names) } else { self.pool.waves_summary(&names) };
        if let Json::Object(fields) = &mut document {
            if !waves && !names.is_empty() {
                fields.insert(0, (String::from("note"), Json::String(without_audio(&names))));
            }
            if !versions.is_empty() {
                fields.push((String::from("versions"), Json::Array(versions.iter().map(|version| version.to_json()).collect())));
            }
            if !pool.is_empty() {
                fields.push((String::from("waves"), Json::Array(pool)));
            }
        }
        (document.write(), names)
    }

    pub(super) fn next_song(&mut self) {
        self.print_take();
        self.commit();
        self.disk.songs = self.disk.songs.saturating_add(1);
        self.change_song(self.disk.songs);
    }

    pub(super) fn change_song(&mut self, song: u64) {
        self.pool.change_song();
        if song != self.disk.song {
            if self.disk.carried.song == self.disk.song {
                self.carry_waves();
            }
            let names = self.song_names();
            let mut shelved = self.disk.shelf.iter().position(|shelved| shelved.song == song).map(|at| self.disk.shelf.remove(at)).unwrap_or_default();
            self.pool.waves.retain(|wave| names.contains(&wave.name));
            self.pool.origins.retain(|(name, _)| names.contains(name));
            self.pool.sources.retain(|(name, _)| names.contains(name));
            std::mem::swap(&mut self.pool.waves, &mut shelved.waves);
            std::mem::swap(&mut self.pool.origins, &mut shelved.origins);
            std::mem::swap(&mut self.pool.sources, &mut shelved.sources);
            self.pool.previewed = None;
            std::mem::swap(&mut self.disk.versions, &mut shelved.versions);
            shelved.song = std::mem::replace(&mut self.disk.song, song);
            shelved.spell();
            self.disk.shelf.push(shelved);
            self.disk.versions_changed += 1;
            self.settings.versions_scroll = 0;
        }
        self.disk.prune(&self.history);
    }

    pub(super) fn carry_waves(&mut self) {
        let names: Vec<&String> = self.clipboard.clips().into_iter().filter_map(|clip| clip.file.as_ref()).map(|file| &file.name).collect();
        let named = |pairs: &[(String, String)]| pairs.iter().filter(|(name, _)| names.contains(&name)).cloned().collect();
        self.disk.carried = Shelved { song: self.disk.song, waves: self.pool.waves.iter().filter(|wave| names.contains(&&wave.name)).cloned().collect(), origins: named(&self.pool.origins), sources: named(&self.pool.sources), ..Shelved::default() };
    }

    pub(super) fn borrow_waves(&mut self) -> bool {
        let mut names: Vec<String> = self.clipboard.clips().into_iter().filter_map(|clip| clip.file.as_ref()).map(|file| file.name.clone()).collect();
        names.sort();
        names.dedup();
        if names.is_empty() {
            return false;
        }
        let owned = self.song_names();
        let foreign = self.disk.carried.song != self.disk.song;
        let mut aims: Vec<(String, String)> = Vec::new();
        let mut brought = Vec::new();
        for name in &names {
            let twin = if foreign { self.twin(name) } else { None };
            let free = self.pool.wave(name).is_none() && !owned.contains(name);
            let taken = |fresh: &str| names.iter().chain(&owned).any(|known| known == fresh) || aims.iter().any(|(_, aim)| aim == fresh) || self.pool.knows(fresh);
            let under = match twin {
                None if free => Some(name.clone()),
                None if foreign => Some(pool::numbered(name, taken)),
                _ => None,
            };
            if let Some(under) = &under {
                self.bring(name, under);
                brought.push(under.clone());
            }
            aims.push((name.clone(), twin.or(under).unwrap_or_else(|| name.clone())));
        }
        if foreign {
            for file in self.clipboard.clips_mut().into_iter().filter_map(|clip| clip.file.as_mut()) {
                if let Some((_, aim)) = aims.iter().find(|(name, _)| *name == file.name) {
                    file.name.clone_from(aim);
                }
            }
            self.carry_waves();
        }
        brought.iter().any(|name| self.pool.wave(name).is_none())
    }

    fn twin(&self, name: &str) -> Option<String> {
        let carried = &self.disk.carried;
        let wave = carried.waves.iter().find(|wave| wave.name == name).filter(|wave| wave.frames() > 0);
        let (origin, source) = (linked(&carried.origins, name), linked(&carried.sources, name));
        let same = |other: &str| match (wave, self.pool.wave(other)) {
            (Some(wave), Some(held)) => held.same_sound(wave),
            _ => {
                let read_alike = origin.is_some_and(|from| linked(&self.pool.origins, other) == Some(from) && (other == name || pool::wave_file(from)));
                read_alike || source.is_some_and(|path| linked(&self.pool.sources, other) == Some(path))
            }
        };
        let known = self.pool.waves.iter().map(|wave| &wave.name).chain(self.pool.origins.iter().chain(&self.pool.sources).map(|(known, _)| known));
        std::iter::once(name).chain(known.map(String::as_str)).find(|other| same(other)).map(String::from)
    }

    fn bring(&mut self, name: &str, under: &str) {
        let carried = &self.disk.carried;
        let renamed = name != under;
        self.pool.origins.retain(|(known, _)| known != under);
        self.pool.sources.retain(|(known, _)| known != under);
        self.pool.origins.extend(carried.origins.iter().filter(|(known, from)| known == name && (!renamed || pool::wave_file(from))).map(|(_, from)| (String::from(under), from.clone())));
        self.pool.sources.extend(carried.sources.iter().filter(|(known, _)| known == name).map(|(_, path)| (String::from(under), path.clone())));
        self.pool.waves.extend(carried.waves.iter().filter(|wave| wave.name == name).map(|wave| if renamed { Arc::new(wave.renamed(under)) } else { Arc::clone(wave) }));
    }

    pub(super) fn song_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for version in &self.disk.versions {
            wave_names(&version.document, &mut names);
        }
        step_names(self.history.every().filter(|step| step.song == self.disk.song), &mut names);
        names
    }

    pub(super) fn adopt(&mut self, song: u64, project: Project, path: Option<String>) -> bool {
        self.print_take();
        self.commit();
        self.turn_to(song, project, path);
        self.commit();
        self.screen = Screen::Main;
        self.remember();
        true
    }

    pub(super) fn restore_version(&mut self, index: usize) -> bool {
        match self.disk.versions.get(index).and_then(|version| Project::from_json(&version.document)) {
            Some(project) => self.adopt(self.disk.song, project, self.disk.path.clone()),
            None => false,
        }
    }

    pub(super) fn restore_edit(&mut self, index: usize) -> bool {
        match self.history.peek(index).cloned() {
            Some(step) => self.adopt(step.song, step.project, step.path),
            None => false,
        }
    }

    pub(super) fn perform(&mut self, action: FileAction) {
        match action {
            FileAction::Settings => self.open_settings(),
            FileAction::NewProject => {
                self.next_song();
                self.start(Project::new());
            }
            FileAction::NewDefaultProject => self.requests.push(Request::Fresh),
            FileAction::OpenProject => self.requests.push(Request::Open { path: None }),
            FileAction::OpenRecent => {
                if !self.disk.recent.is_empty() {
                    self.menu = Some(Menu::Recent);
                }
            }
            FileAction::SaveProject => {
                let (json, save) = self.file_json(self.disk.path.is_none());
                self.requests.push(match &self.disk.path {
                    Some(path) => Request::Write { path: path.clone(), json, save },
                    None => Request::SaveAs { json, save },
                });
            }
            FileAction::SaveAs => {
                let (json, save) = self.file_json(true);
                self.requests.push(Request::SaveAs { json, save });
            }
            FileAction::SaveAsDefault => {
                let json = self.assemble(self.project.to_json(), &[], true).0;
                self.disk.saved_note = Some((Saved::Default, SAVED_SECONDS));
                self.requests.push(Request::Default { json: Some(json) });
            }
            FileAction::ClearDefault => {
                self.disk.saved_note = Some((Saved::Cleared, SAVED_SECONDS));
                self.requests.push(Request::Default { json: None });
            }
            FileAction::ExportProject => self.menu = Some(Menu::Export),
            FileAction::ExportAudio => {
                if self.export_span().is_some() {
                    self.requests.push(Request::Export { name: self.disk.export_name(), message: self.export_label(EXPORT_AUDIO) });
                }
            }
        }
    }

    pub fn export_to(&mut self, path: String) {
        let Some(span) = self.export_span() else {
            return;
        };
        let tail = if self.ruler.range().is_some() { audio::Tail::Cut } else { audio::Tail::Rings };
        self.export_between(path, span, tail);
    }

    pub(super) fn export_between(&mut self, path: String, span: (i32, i32), tail: audio::Tail) {
        if self.playing {
            self.stop();
        }
        let name = stem_of(&path);
        match audio::Export::start(&self.project, &self.pool.waves, span, tail) {
            Some(export) => self.disk.exporting = Some(Exporting { rendering: Rendering::Sound(Bound::File(path), export), name, title: format!("{EXPORTING} · {}", spanned(span.1 - span.0).to_uppercase()), age: 0.0 }),
            None => self.exported(path, false),
        }
    }

    pub(super) fn bounce_span(&self) -> Option<(i32, i32)> {
        let (start, end) = match &self.selection.region {
            Some(region) => (region.start, region.end),
            None => self.ruler.range().map(|range| (range.start, range.end))?,
        };
        (end > start).then_some((start, end))
    }

    pub(super) fn start_bounce(&mut self) -> bool {
        let Some((start, end)) = self.bounce_span() else {
            return false;
        };
        if self.playing {
            self.stop();
        }
        let Some(export) = audio::Export::start(&self.project, &self.pool.waves, (start, end), audio::Tail::Cut) else {
            return false;
        };
        let at = self.project.tracks.len();
        let name = self.pool.fresh_name(&format!("Master {BOUNCE}"));
        let title = format!("{BOUNCING} · {}", spanned(end - start).to_uppercase());
        self.disk.exporting = Some(Exporting { rendering: Rendering::Sound(Bound::Track { at, start, length: end - start }, export), name, title, age: 0.0 });
        true
    }

    pub(super) fn flatten_targets(&self) -> Vec<Flattening> {
        self.selection.picked_tracks().into_iter().rev().filter(|&track| self.project.tracks.get(track).is_some_and(|picked| !picked.group)).filter_map(|track| self.flattening_of(track)).collect()
    }

    fn flattening_of(&self, head: usize) -> Option<Flattening> {
        let fed = 1 << head | self.project.fed_by(head);
        let clips = self.project.tracks.iter().enumerate().filter(|(track, _)| fed & (1 << track) != 0).flat_map(|(_, track)| &track.clips);
        let (start, end) = clips.fold((i32::MAX, i32::MIN), |(start, end), clip| (start.min(clip.start), end.max(clip.end())));
        (end > start).then(|| Flattening { head, start, song: end - start })
    }

    pub(super) fn start_flatten(&mut self) -> bool {
        let targets = self.flatten_targets();
        if targets.is_empty() {
            return false;
        }
        if self.playing {
            self.stop();
        }
        let total = targets.len();
        self.run_flatten(targets, total, Box::new(self.project.clone()))
    }

    fn run_flatten(&mut self, mut queue: Vec<Flattening>, total: usize, unflattened: Box<Project>) -> bool {
        while !queue.is_empty() {
            let flattening = queue.remove(0);
            let project = Self::flattened_project(&unflattened, &flattening);
            let span = (flattening.start, flattening.start + flattening.song);
            let Some(export) = audio::Export::start(&project, &self.pool.waves, span, audio::Tail::Rings) else {
                continue;
            };
            let name = self.project.tracks[flattening.head].name.clone();
            let length = spanned(flattening.song).to_uppercase();
            let title = if total > 1 { format!("{FLATTENING} {} OF {total} · {length}", total - queue.len()) } else { format!("{FLATTENING} · {length}") };
            self.disk.exporting = Some(Exporting { rendering: Rendering::Sound(Bound::Flatten { flattening, rest: queue, total, unflattened }, export), name, title, age: 0.0 });
            return true;
        }
        false
    }

    fn flattened_project(unflattened: &Project, flattening: &Flattening) -> Project {
        let mut project = unflattened.clone();
        project.master = crate::project::Return::DEFAULT;
        for (index, track) in project.tracks.iter_mut().enumerate() {
            track.solo = index == flattening.head;
            track.sends = [0.0; SEND_COUNT];
        }
        let head = &mut project.tracks[flattening.head];
        head.mute = false;
        head.gain = 1.0;
        head.delay_ms = 0.0;
        strip_mix_lanes(&mut head.automation);
        if let Some(group) = unflattened.group_of(flattening.head) {
            let group = &mut project.tracks[group];
            group.gain = 1.0;
            group.delay_ms = 0.0;
            group.mute = false;
            group.rack.clear();
            group.automation = Automation::NONE;
        }
        project
    }

    fn land_flatten(&mut self, flattening: Flattening, rest: Vec<Flattening>, total: usize, unflattened: Box<Project>, bytes: Option<Vec<u8>>) {
        let Some(original) = self.project.tracks.get(flattening.head).cloned() else {
            return;
        };
        let bpm = self.project.bpm;
        let least = (f64::from(wav::SAMPLE_RATE) * 60.0 / bpm * beats(flattening.song)).round() as usize;
        let Some(wave) = bytes.and_then(|bytes| Wave::parse(self.pool.fresh_name(&original.name), String::new(), &bytes).ok()).map(|wave| trimmed(wave, least)) else {
            self.disk.export_stem = Some(original.name);
            self.disk.saved_note = Some((Saved::ExportFailed, FAILED_SECONDS));
            return;
        };
        let name = wave.name.clone();
        let length = (wave.seconds() * bpm / 60.0 * f64::from(TICKS_PER_BEAT)).ceil().max(1.0) as i32;
        self.pool.keep(wave, None);
        let clip = Clip::audio(flattening.start, length, original.name.clone(), name);
        self.project.tracks[flattening.head] = Track { name: original.name, brief: original.brief, solo: original.solo, mute: original.mute, gain: original.gain, delay_ms: original.delay_ms, sends: original.sends, clips: vec![clip], automation: mix_lanes(&original.automation), inside: original.inside, ..Track::audio() };
        if let Some(generator) = self.generators.get_mut(flattening.head) {
            *generator = Default::default();
        }
        self.select(flattening.head);
        self.show(None);
        self.selection.clips = vec![(flattening.head, flattening.start)];
        self.finish_edit();
        if !rest.is_empty() {
            self.run_flatten(rest, total, unflattened);
        }
    }

    fn land_bounce(&mut self, name: String, at: usize, start: i32, length: i32, bytes: Option<Vec<u8>>) {
        let Some(mut wave) = bytes.and_then(|bytes| Wave::parse(name.clone(), String::new(), &bytes).ok()) else {
            self.disk.export_stem = Some(name);
            self.disk.saved_note = Some((Saved::ExportFailed, FAILED_SECONDS));
            return;
        };
        wave.path = None;
        self.pool.keep(wave, None);
        let at = at.min(self.project.tracks.len());
        if !self.insert_track(at, Track { name: name.clone(), ..Track::audio() }, false) {
            return;
        }
        self.select(at);
        let bpm = self.project.bpm;
        self.project.tracks[at].place(Clip::audio(start, length, name.clone(), name), bpm);
        self.selection.clips = vec![(at, start)];
        self.scroll_to_selected();
        self.finish_edit();
    }

    pub(super) fn cancel_export(&mut self) -> bool {
        let Some(exporting) = self.disk.exporting.take() else {
            return false;
        };
        exporting.rendering.cancel();
        if let Rendering::Sound(Bound::File(path), _) = &exporting.rendering {
            self.agent.unland(path);
        }
        true
    }

    pub(super) fn tick_export(&mut self, seconds: f32) -> bool {
        let Some(exporting) = &mut self.disk.exporting else {
            return false;
        };
        exporting.age += seconds;
        if exporting.rendering.finished()
            && let Some(exporting) = self.disk.exporting.take()
        {
            match exporting.rendering {
                Rendering::Sound(bound, mut export) => {
                    let bytes = export.take();
                    match bound {
                        Bound::File(path) => match bytes {
                            Some(bytes) => self.requests.push(Request::Exported { path, bytes }),
                            None => self.exported(path, false),
                        },
                        Bound::Track { at, start, length } => self.land_bounce(exporting.name, at, start, length, bytes),
                        Bound::Flatten { flattening, rest, total, unflattened } => self.land_flatten(flattening, rest, total, unflattened, bytes),
                    }
                }
                Rendering::Signal(host, row, mut tracing) => {
                    if let Some(trace) = tracing.take() {
                        self.land_duck(host, row, &trace);
                        self.finish_edit();
                    }
                }
            }
        }
        true
    }

    pub fn exported(&mut self, path: String, written: bool) {
        self.disk.export_stem = Some(stem_of(&path));
        self.disk.saved_note = Some(if written { (Saved::Exported, SAVED_SECONDS) } else { (Saved::ExportFailed, FAILED_SECONDS) });
        self.agent.landed(&path, written);
    }

    pub(super) fn export_span(&self) -> Option<(i32, i32)> {
        match self.ruler.range() {
            Some(range) => Some((range.start, range.end)),
            None => Some((0, self.project.end())).filter(|(_, end)| *end > 0),
        }
    }

    pub(super) fn export_label(&self, label: &str) -> String {
        let Some((start, end)) = self.export_span() else {
            return String::from(label);
        };
        let what = if self.ruler.range().is_some() { EXPORT_LOOP } else { label };
        format!("{what} · {}", spanned(end - start))
    }

    pub(super) fn menu_inert(&self, menu: Menu, index: usize) -> bool {
        menu == Menu::File
            && match FILE_ITEMS[index].0 {
                FileAction::ExportAudio => self.export_span().is_none(),
                FileAction::OpenRecent => self.disk.recent.is_empty(),
                _ => false,
            }
    }

    pub(super) fn menu_current(&self, menu: Menu) -> Option<usize> {
        match menu {
            Menu::File => self.menu.and_then(Menu::opener).map(file_row),
            Menu::Export => None,
            Menu::Recent => self.disk.recent.iter().position(|path| Some(path) == self.disk.path.as_ref()),
            Menu::Snap => grid::SNAPS.iter().position(|snap| *snap == self.timeline.snap),
            Menu::Key => Some(self.project.root.index()),
            Menu::Scale => Some(self.project.scale.index()),
            Menu::Fit => self.fits.iter().position(|fit| (fit.root, fit.scale) == (self.project.root, self.project.scale)),
            Menu::Add(track) => (self.menu == Some(Menu::Follow(track))).then_some(ADD_ITEMS.len()),
            Menu::Follow(track) => self.project.tracks.get(track).and_then(|owner| owner.follows).and_then(|source| self.project.follow_sources(track).iter().position(|&listed| listed == source)),
        }
    }

    pub(super) fn browser_down(&mut self, x: i32, y: i32, clicks: i32) -> bool {
        let (bx, by) = self.browser_point(x, y);
        let (changed, action) = self.browser.down(bx, by, clicks);
        let acted = action.is_some();
        if let Some(action) = action {
            self.browser_act_at(action, x, y);
        }
        self.keep_hearts();
        changed || acted
    }

    pub(super) fn browser_hover(&mut self, x: i32, y: i32) -> bool {
        let point = (self.hover == Hover::Browser).then(|| self.browser_point(x, y));
        self.browser.hover(point)
    }

    pub(super) fn browser_act_at(&mut self, action: browser::Action, x: i32, y: i32) {
        match action {
            browser::Action::Place { .. } | browser::Action::Open { .. } | browser::Action::Load { .. } if self.browser_big() => {}
            browser::Action::Place { path, kind, dropped } => self.place_from_browser(path, kind, dropped, Some((x, y))),
            browser::Action::Load { name, description, preset, dropped: true } => self.load_preset(name, description, preset, Some((x, y))),
            action => self.browser_act(action),
        }
    }

    pub(super) fn browser_finish(&mut self) -> bool {
        let (changed, action) = self.browser.finish_rename();
        if let Some(action) = action {
            self.browser_act(action);
        }
        changed | self.browser.blur()
    }

    pub(super) fn browser_delete(&mut self) -> bool {
        if !self.browser_shown() {
            return false;
        }
        let (changed, action) = self.browser.delete();
        if let Some(action) = action {
            self.browser_act(action);
        }
        changed
    }
}

fn spanned(ticks: i32) -> String {
    let beats = beats(ticks);
    match (ticks / TICKS_PER_BAR, ticks % TICKS_PER_BAR) {
        (1, 0) => String::from("1 Bar"),
        (bars, 0) => format!("{bars} Bars"),
        _ if ticks == TICKS_PER_BEAT => String::from("1 Beat"),
        _ if beats.fract() == 0.0 => format!("{} Beats", beats as i32),
        _ => format!("{beats:.2} Beats"),
    }
}

pub(super) fn before_tracks(mut document: Json, extra: Fields) -> Json {
    if let (Json::Object(fields), Json::Object(extra)) = (&mut document, extra.into_json()) {
        let at = fields.iter().position(|(key, _)| key == "tracks").unwrap_or(fields.len());
        fields.splice(at..at, extra);
    }
    document
}

pub(super) fn manual() -> Json {
    let mut state = Fields::new();
    App::transport_manual(&mut state);
    App::session_manual(&mut state);
    state.push("folders", Json::from(FOLDERS_MANUAL));
    let mut document = before_tracks(crate::project::manual::document(), state);
    if let Json::Object(fields) = &mut document {
        fields.push((String::from("versions"), Json::from(VERSIONS_MANUAL)));
        fields.push((String::from("waves"), Json::from(WAVES_MANUAL)));
        fields.push((String::from("preset"), crate::project::manual::preset()));
    }
    document
}

fn without_audio(names: &[String]) -> String {
    format!("Audio left out: {}. Each wave.name needs a sourcePath or a matching WAV in a browser folder, or it plays silent.", names.join(", "))
}

fn versions_from(json: Option<&Json>) -> Vec<Version> {
    let mut versions: Vec<Version> = json.and_then(Json::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Version::from_json).collect());
    versions.drain(..versions.len().saturating_sub(VERSIONS));
    versions
}

pub(super) fn pairs(json: &Json, key: &str, to: &str) -> Vec<(String, String)> {
    items(Some(json), key).iter().filter_map(|item| Some((String::from(item.get("name")?.as_str()?), String::from(item.get(to)?.as_str()?)))).collect()
}

fn linked<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs.iter().find(|(known, _)| known == name).map(|(_, to)| to.as_str())
}

pub(super) fn spell_pairs<'a>(pairs: impl IntoIterator<Item = &'a (String, String)>, to: &str) -> Vec<Json> {
    pairs.into_iter().map(|(name, value)| Json::Object(vec![(String::from("name"), Json::String(name.clone())), (String::from(to), Json::String(value.clone()))])).collect()
}

fn step_names<'a>(steps: impl Iterator<Item = &'a Step>, names: &mut Vec<String>) {
    for file in steps.flat_map(|step| &step.project.tracks).flat_map(|track| &track.clips).filter_map(|clip| clip.file.as_ref()) {
        if !names.contains(&file.name) {
            names.push(file.name.clone());
        }
    }
}

fn spell_edits(history: &History, key: u64) -> Spelled {
    let mut names = Vec::new();
    step_names(history.recent(EDITS).iter(), &mut names);
    Spelled { key, text: history.to_json(EDITS).write_at(1).into(), names }
}
