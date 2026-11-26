use super::*;
use super::edit::Clipboard;
use super::generator::Generator;
use super::region::Covered;
use super::serve::{followed, landings};
use super::settings::Settings;
use super::sorry::Sorry;

pub(super) fn pick_key(json: &Json) -> Option<(usize, i32)> {
    let pair = json.as_array()?;
    Some((pair.first()?.as_i64()?.try_into().ok()?, ticks(pair.get(1)?.as_f64()?)))
}

fn kin(old: &Track, new: &Track) -> bool {
    let seeded = |seed: u64| old.rack.iter().any(|row| matches!(row, Row::Core(layer) if layer.seed == seed));
    old == new || new.rack.iter().any(|row| matches!(row, Row::Core(layer) if seeded(layer.seed)))
}

fn origins(old: &[Track], new: &[Track]) -> Vec<Option<usize>> {
    let stays = |at: usize| old.get(at).zip(new.get(at)).is_some_and(|(held, track)| kin(held, track));
    let found = |at: usize| if stays(at) { Some(at) } else { (0..old.len()).find(|&from| !stays(from) && kin(&old[from], &new[at])) };
    (0..new.len()).map(|at| found(at).or((old.len() == new.len()).then_some(at))).collect()
}

impl App {
    pub fn new() -> App {
        let project = Project::new();
        let history = History::new(&project, None, 0);
        App {
            width: 0,
            height: 0,
            fresh: project.clone(),
            project,
            history,
            gesture: None,
            defaults: Vec::new(),
            palettes: std::array::from_fn(Palette::new),
            face: Face::new().keyed(),
            lfo_face: Face::new(),

            rack: Rack::new(rack_w()),
            roll: Roll::new(),
            ruler: Ruler::new(),
            roll_note: None,
            open: None,
            save: None,
            stock: None,
            notice: None,
            ask: None,
            picture: Picture::new(),
            tv: Tv::new(),
            lane_sketch: Sketch::new(),
            sketch_lane: None,
            sorry: Sorry::new(),
            command: false,
            requests: Vec::new(),
            disk: files::Disk::new(),
            screen: Screen::Main,
            hover: Hover::Nothing,
            bar_under: None,
            menu: None,
            fits: Vec::new(),
            settings: Settings::new(),
            wave_face: Face::new(),
            wave_view: None,
            rename: None,
            drag: None,
            monitored: None,
            generators: Vec::new(),
            soloed: None,
            timeline: layout::Timeline::new(),
            selection: tracks::Selection::new(),
            live: keys::Live::new(),
            view: view::View::new(),
            audio: None,
            playing: false,
            play_from: 0,
            playhead: 0.0,
            shift: false,
            browser: Browser::new(),
            factory: None,
            lanes: Lanes::new(),
            armed: None,
            armed_returns: Default::default(),
            lfo_strip: None,
            ducks: ducks::Ducks::default(),
            cut_flash: None,
            clipboard: Clipboard::Empty,
            pool: pool::Pool::new(),
            agent: serve::Agent::new(),
            list: DrawList::new(),
        }
    }

    pub fn theme(&self) -> Theme {
        self.settings.theme()
    }

    pub fn buffer(&self) -> u32 {
        self.settings.buffer
    }

    pub fn folders(&self) -> Vec<String> {
        self.browser.folders()
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    pub fn resume(&mut self, text: &str) -> bool {
        let Some(json) = json::parse(text) else {
            return false;
        };
        let Some(project) = json.get("project").and_then(Project::from_json) else {
            return false;
        };
        self.replace_project(project, json.get("path").and_then(Json::as_str).map(String::from));
        self.history = History::from_json(json.get("edits")).unwrap_or_else(|| History::new(&self.project, self.disk.path.as_deref(), 0));
        self.disk.resume(&json, &self.history);
        self.commit();
        self.change_song(self.disk.song);
        self.pool.waves.clear();
        self.pool.origins = files::pairs(&json, "origins", "from");
        for name in self.song_names() {
            if !self.pool.origins.iter().any(|(known, _)| *known == name)
                && let Some(path) = &self.disk.path
            {
                self.pool.origins.push((name, path.clone()));
            }
        }
        self.pool.sources = files::pairs(&json, "sources", "path");
        let preview = self.settings.preview;
        self.settings.resume(&json);
        if self.settings.preview != preview && let Some(audio) = &self.audio {
            audio.preview_gain(self.preview_gain());
        }
        self.settle_server();
        self.selection.resume(&json);
        self.selection.track_picks = items(Some(&json), "picked_tracks").iter().filter_map(Json::as_i64).map(|track| track.max(0) as usize).collect();
        self.selection.settle(self.project.tracks.len());
        self.generators = items(Some(&json), "generators").iter().map(Generator::from_json).collect();
        self.defaults = items(Some(&json), "defaults").iter().filter_map(Row::from_json).filter(Row::is_effect).collect();
        self.soloed = index(Some(&json), "send_solo", SEND_COUNT);
        self.open = json.get("open").and_then(Json::as_i64).map(|index| index.max(0) as usize);
        self.view.resume(&json);
        self.timeline.resume(&json);
        self.transport_from_json(&json);
        self.live.resume(&json);
        let paths = |key: &str| items(Some(&json), key).iter().filter_map(Json::as_str).map(String::from).collect::<Vec<String>>();
        self.browser.resume(paths("folders"), paths("unfolded"), paths("arranged"));
        self.disk.recent = paths("recent");
        self.disk.recall();
        self.refresh();
        self.reread();
        self.recover_missing();
        self.roll.remember(number(Some(&json), "roll_zoom", f64::from(roll::ZOOM_DEFAULT)) as i32, flag(Some(&json), "roll_big", false), flag(Some(&json), "roll_guide", false), flag(Some(&json), "roll_preview", true), number(Some(&json), "roll_time", f64::from(roll::TIME_MIN)) as f32);
        self.selection.clips = items(Some(&json), "picked").iter().filter_map(pick_key).collect();
        self.selection.paste_point = json.get("paste_point").and_then(pick_key).filter(|(track, _)| *track < self.project.tracks.len());
        self.drop_lost_picks();
        self.settle_roll();
        self.sync_audio();
        true
    }

    pub fn begin(&mut self, path: String, text: Option<&str>) {
        let json = text.and_then(json::parse);
        let mut project = Project::new();
        self.next_song();
        if let Some(json) = &json
            && let Some(Document::Project(kept)) = Document::from_json(json)
        {
            self.adopt_pool(json.get("waves"), &path);
            project = kept;
        }
        self.start(project);
        self.recover_missing();
    }

    pub(super) fn start(&mut self, project: Project) {
        let untouched = self.disk.path.is_none() && self.project == self.fresh && self.history.every().nth(1).is_none();
        self.fresh = project.clone();
        self.replace_project(project, None);
        if untouched {
            self.history = History::new(&self.project, None, self.disk.song);
        }
        self.commit();
        self.remember();
    }

    pub(super) fn commit(&mut self) {
        self.history.commit(&self.project, self.disk.path.as_deref(), self.disk.song);
    }

    pub(super) fn commit_gesture(&mut self, rest: f32) {
        let ours = self.gesture.is_some_and(|(step, _)| step == self.history.changes());
        if ours && self.history.recent(1).last().is_some_and(|last| last.project != self.project) {
            self.history.retake();
        }
        let before = self.history.changes();
        self.commit();
        if self.history.changes() != before {
            self.gesture = Some((self.history.changes(), rest));
        }
    }

    pub(super) fn session(&mut self) -> Json {
        let mut fields = Fields::new();
        let document = self.project.to_json();
        let (edits, edit_names) = self.edits_spelled();
        let (versions, version_names) = self.disk.versions_spelled();
        let versions = (!self.disk.versions.is_empty()).then_some(versions);
        let mut names = Vec::new();
        wave_names(&document, &mut names);
        for name in edit_names.into_iter().chain(version_names) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        fields.push("project", document);
        if let Some(path) = &self.disk.path {
            fields.push("path", Json::String(path.clone()));
        }
        fields.list("recent", self.disk.recent.iter().cloned().map(Json::String).collect());
        if let Some(versions) = versions {
            fields.push("versions", Json::Raw(versions));
        }
        fields.push("edits", Json::Raw(edits));
        fields.list("origins", files::spell_pairs(names.iter().filter_map(|name| self.pool.origins.iter().find(|(known, _)| known == name)), "from"));
        fields.list("sources", files::spell_pairs(names.iter().filter_map(|name| self.pool.sources.iter().find(|(known, _)| known == name)), "path"));
        fields.list("shelved", self.disk.shelved());
        self.settings.session(&mut fields);
        self.selection.session(&mut fields);
        fields.list("picked_tracks", self.selection.track_picks.iter().map(|track| Json::Number(*track as f64)).collect());
        if self.generators.iter().any(|generator| *generator != Generator::default()) {
            fields.push("generators", Json::Array(self.generators.iter().map(Generator::to_json).collect()));
        }
        fields.list("defaults", self.defaults.iter().map(|row| row.renumbered(|_| None).to_json()).collect());
        if let Some(send) = self.soloed {
            fields.push("send_solo", Json::Number(send as f64));
        }
        if let Some(open) = self.open.filter(|_| self.monitored.is_none()) {
            fields.push("open", Json::Number(open as f64));
        }
        self.view.session(&mut fields);
        self.timeline.session(&mut fields);
        self.transport_fields(&mut fields);
        self.live.session(&mut fields);
        if !self.browser.is_empty() {
            fields.push("folders", Json::Array(self.browser.folders().into_iter().map(Json::String).collect()));
        }
        fields.list("unfolded", self.browser.unfolded().into_iter().map(Json::String).collect());
        fields.list("arranged", self.browser.arranged().iter().cloned().map(Json::String).collect());
        fields.number("roll_zoom", f64::from(self.roll.zoom()), f64::from(roll::ZOOM_DEFAULT));
        fields.flag("roll_big", self.roll.big(), false);
        fields.flag("roll_guide", self.roll.guide(), false);
        fields.flag("roll_preview", self.roll.preview(), true);
        fields.number("roll_time", f64::from(self.roll.time()), f64::from(roll::TIME_MIN));
        fields.list("picked", self.selection.clips.iter().map(|&(track, start)| Json::Array(vec![Json::Number(track as f64), Json::Number(beats(start))])).collect());
        if let Some((track, tick)) = self.selection.paste_point {
            fields.push("paste_point", Json::Array(vec![Json::Number(track as f64), Json::Number(beats(tick))]));
        }
        fields.into_json()
    }

    pub(super) fn remember(&mut self) {
        self.stow_made();
        self.disk.prune(&self.history);
        let json = self.session().write();
        if json != self.disk.remembered {
            self.disk.remembered = json.clone();
            self.requests.push(Request::Remember { json });
        }
    }

    pub fn not_remembered(&mut self) {
        self.disk.remembered.clear();
        self.not_saved();
    }

    pub(super) fn restore(&mut self, step: Step) {
        if step.song != self.disk.song {
            self.turn_to(step.song, step.project, step.path);
            return;
        }
        let origins = self.carry_clipboard(&step.project);
        let landed = |track: usize| origins.iter().position(|&from| from == Some(track));
        let (old, new) = (&self.project.tracks, &step.project.tracks);
        let generators = origins.iter().enumerate().map(|(at, from)| from.map(|from| followed(self.generator(from), &landings(&old[from].rack, &new[at].rack))).unwrap_or_default()).collect();
        let lost = self.monitored.is_none() && landed(self.selection.track).is_none();
        self.follow_rows(&step.project, |at| origins[at].filter(|&from| landed(from) == Some(at)));
        self.settle_project(step.project, landed);
        self.generators = generators;
        if lost {
            self.armed = self.generator(self.selection.track).armed;
            self.arrive();
            self.settle_armed();
        }
    }

    fn carry_clipboard(&mut self, project: &Project) -> Vec<Option<usize>> {
        let (song, origins) = (self.disk.song, origins(&self.project.tracks, &project.tracks));
        for (index, track) in self.project.tracks.iter().enumerate().filter(|(index, _)| !origins.contains(&Some(*index))) {
            self.clipboard.lose(song, index, track);
        }
        self.clipboard.renumber(song, |track| origins.iter().position(|&from| from == Some(track)));
        self.clipboard.find(song, |gone| (0..project.tracks.len()).find(|&at| origins[at].is_none() && kin(gone, &project.tracks[at])));
        origins
    }

    pub(super) fn turn_to(&mut self, song: u64, project: Project, path: Option<String>) {
        if song == self.disk.song {
            self.carry_clipboard(&project);
        }
        self.change_song(song);
        self.replace_project(project, path);
        self.reread();
        self.recover_missing();
    }

    pub(super) fn settle_project(&mut self, project: Project, landed: impl Fn(usize) -> Option<usize>) {
        if project.tracks.len() != self.project.tracks.len() {
            self.generators.clear();
            self.close_track_menu();
            self.rename = None;
        }
        let (kind, page, band) = (self.shown_kind(), self.face.page(), self.picture.band);
        let held = self.lfo_strip.and_then(|strip| self.automations().get(strip).cloned()).map(|(host, armed)| match host {
            Host::Track(track) => (landed(track).map_or(host, Host::Track), armed),
            Host::Send(_) | Host::Master => (host, armed),
        });
        self.project = project;
        self.selection.track = landed(self.selection.track).unwrap_or(self.selection.track);
        self.selection.renumber(&landed);
        self.selection.settle(self.project.tracks.len());
        self.drop_lost_picks();
        self.settle_armed();
        self.show(self.open);
        if self.shown_kind() == kind {
            self.face.turn(page);
            self.picture.band = band;
        }
        self.lfo_strip = held.and_then(|held| self.automations().into_iter().position(|lane| lane == held));
        if self.lfo_strip.is_none() {
            self.lfo_face.close();
        }
    }

    fn close_track_menu(&mut self) {
        self.menu.take_if(|menu| matches!(menu, Menu::Add(_) | Menu::Follow(_)));
    }

    fn shown_kind(&self) -> Option<&'static str> {
        self.shown().map(|(host, row)| self.project.rack(host)[row].label())
    }

    pub(super) fn drop_lost_picks(&mut self) {
        self.selection.clips.retain(|(track, start)| self.project.tracks.get(*track).is_some_and(|owner| owner.clips.iter().any(|clip| clip.start == *start)));
        let count = self.project.tracks.len();
        if let Some(region) = &mut self.selection.region {
            region.covers.retain(|covered| match covered {
                Covered::Track(track) => *track < count,
                Covered::Curve(host, _) => host.track().is_none_or(|track| track < count),
            });
        }
        self.selection.region.take_if(|region| region.covers.is_empty());
    }

    pub(super) fn dragging(&self) -> bool {
        self.drag.is_some() || self.wave_face.dragging() || self.face.dragging() || self.lfo_face.dragging() || self.picture.dragging() || self.tv.dragging() || self.lane_sketch.dragging() || self.rack.dragging() || self.roll.dragging() || self.ruler.dragging() || self.active_palette().is_some() || self.browser.dragging() || self.lanes.dragging()
    }

    pub(super) fn replace_project(&mut self, project: Project, path: Option<String>) {
        self.project = project;
        self.disk.replace(path);
        self.agent.replace();
        self.timeline.replace();
        self.selection.replace();
        self.rename = None;
        self.close_track_menu();
        self.armed = None;
        self.armed_returns = Default::default();
        self.drag = None;
        self.wave_view = None;
        self.lfo_face.close();
        self.lfo_strip = None;
        self.monitored = None;
        self.generators.clear();
        self.soloed = None;
        self.show(None);
        if let Some(audio) = &mut self.audio {
            audio.clear_clip();
        }
        self.install_audio();
        self.halt();
    }
}
