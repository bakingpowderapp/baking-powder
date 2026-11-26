use super::*;
use super::generator::Generator;
use sdk::dsp::ears::{BANDS, Ear, FLOOR_DB, HOP_SECONDS, Heard, amplitude_db, hop_of};
use crate::audio::{EXPORT_SAMPLE_RATE, Tail, frames_per_beat, render_span};
use crate::wav::Wav;
use std::sync::atomic::AtomicUsize;
use fx::Params;
use crate::mcp::{self, Host, Shelf};
use crate::project::{BEATS_PER_BAR, LEAST_BPM, MOST_BPM, Row, SEND_COUNT};
use std::cmp::Reverse;
use std::hash::{DefaultHasher, Hash, Hasher};

const REMEMBERED: usize = 4;
const CLIPPED_DB: f64 = 0.05;
const PINNED_DB: f64 = 6.0;
const SILENT_DB: f64 = -60.0;
const FLAT_LU: f64 = 2.0;
const HEARD_LUFS: f64 = -70.0;
const MOST_HITS: usize = 32;
const MOST_RANGES: usize = 16;
const REPLACED: &str = "The project was replaced in the app while this call waited; read it again before editing";

pub(super) struct Hearing {
    call: mcp::Call,
    listen: Listen,
    asked: Asked,
    spans: Vec<(f64, f64)>,
    details: bool,
    kept: Option<Kept>,
}

type Kept = (String, bool);
type Asking = (Asked, Project, Option<Project>, Vec<(f64, f64)>, Option<Kept>);

pub(super) struct Remembered {
    asked: Asked,
    listened: Listened,
}

struct Asked {
    key: u64,
    revision: u64,
    ticks: (i32, i32),
    span: (f64, f64),
    bpm: f64,
    track: Option<usize>,
    alone: Option<usize>,
    core: Option<CoreSolo>,
    against: Option<u64>,
    names: Vec<String>,
    scheduled: Vec<bool>,
    hosts: Vec<crate::project::Host>,
    dynamics: Vec<Dynamic>,
}

struct Dynamic {
    host: usize,
    row: usize,
    path: String,
    kind: &'static str,
}

pub(super) struct Landing {
    call: mcp::Call,
    path: String,
    text: String,
}

struct Browsing {
    call: mcp::Call,
    walk: audio::Job<mcp::Reply>,
}

pub(super) struct Agent {
    calls: Option<std::sync::mpsc::Receiver<mcp::Call>>,
    arrived: std::collections::VecDeque<mcp::Call>,
    browsing: Vec<Browsing>,
    hearing: Option<Hearing>,
    heard_before: Vec<Remembered>,
    landing: Vec<Landing>,
    server: Option<mcp::Server>,
    revisions: mcp::Revisions,
    shifts: Vec<mcp::Shift>,
    pub(super) status: mcp::Status,
}

impl Agent {
    pub(super) fn new() -> Agent {
        Agent { calls: None, arrived: std::collections::VecDeque::new(), browsing: Vec::new(), hearing: None, heard_before: Vec::new(), landing: Vec::new(), server: None, revisions: mcp::Revisions::default(), shifts: Vec::new(), status: mcp::Status::default() }
    }

    pub(super) fn replace(&mut self) {
        self.revisions.clear();
        self.heard_before.clear();
        if let Some(hearing) = self.hearing.take() {
            hearing.listen.cancel();
            hearing.call.answer(mcp::Reply::refused(REPLACED));
        }
        let (edits, rest) = std::mem::take(&mut self.arrived).into_iter().partition(|call| mcp::edits(&call.tool));
        self.arrived = rest;
        for call in edits {
            call.answer(mcp::Reply::refused(REPLACED));
        }
    }

    fn stop(&mut self) {
        self.calls = None;
        self.arrived.clear();
        self.landing.clear();
        for browsing in self.browsing.drain(..) {
            browsing.walk.cancel();
        }
        if let Some(hearing) = self.hearing.take() {
            hearing.listen.cancel();
        }
        self.server = None;
        self.status = mcp::Status::default();
    }

    pub(super) fn landed(&mut self, path: &str, written: bool) {
        let Some(at) = self.landing.iter().position(|landing| landing.path == path) else {
            return;
        };
        let landing = self.landing.remove(at);
        landing.call.answer(if written { mcp::Reply::ok(landing.text) } else { mcp::Reply::refused(format!("{path} could not be written")) });
    }

    pub(super) fn unland(&mut self, path: &str) {
        if let Some(at) = self.landing.iter().position(|landing| landing.path == path) {
            self.landing.remove(at).call.answer(mcp::Reply::refused(format!("The export into {path} was cancelled in the app")));
        }
    }

    pub(super) fn serving(&self) -> bool {
        self.server.is_some()
    }

    pub(super) fn label(&self, glyphs: usize) -> String {
        let Some(client) = self.status.client() else {
            return String::from(settings::NONE);
        };
        let calls = self.status.calls().to_string();
        format!("{} {calls}", font::shortened(&client.to_uppercase(), glyphs.saturating_sub(calls.len() + 1)))
    }

    pub(super) fn story(&self) -> String {
        let Some(client) = self.status.client() else {
            return String::from("The program last connected to the server, with the calls it has made since and the last of them; none has connected since the server started");
        };
        let last = self.status.last().map_or_else(String::new, |(tool, since)| format!("; the last, {}, {} ago", tool.to_uppercase(), ago(since.as_secs())));
        format!("{}: {} since it connected{last}", client.to_uppercase(), counted(self.status.calls() as i32, "CALL"))
    }
}

fn ago(seconds: u64) -> String {
    if seconds < 60 { format!("{seconds} S") } else { settings::age(seconds) }
}

impl App {
    pub(super) fn serve_calls(&mut self) -> bool {
        let before = self.agent.status.calls();
        self.tick_browse();
        self.serve_arrived();
        while let Some(call) = self.agent.calls.as_ref().and_then(|calls| calls.try_recv().ok()) {
            self.deliver(call);
        }
        self.agent.status.calls() != before
    }

    pub fn deliver(&mut self, call: mcp::Call) -> bool {
        self.agent.arrived.push_back(call);
        self.serve_arrived();
        self.agent.arrived.is_empty()
    }

    fn serve_arrived(&mut self) {
        while let Some(call) = self.agent.arrived.pop_front() {
            if !call.claim() {
                continue;
            }
            if call.tool == mcp::LISTEN {
                self.listen(call);
                continue;
            }
            if call.tool == mcp::LIBRARY {
                self.browse(call);
                continue;
            }
            let Some(reply) = self.serve_call(&call.tool, &call.args) else {
                call.release();
                self.agent.arrived.push_front(call);
                return;
            };
            if call.tool == mcp::EXPORT && !reply.failed {
                let asked = call.args.get("path").and_then(Json::as_str).unwrap_or_default();
                let path = mcp::whole(&self.shelves(), asked).unwrap_or_else(|_| String::from(asked.trim()));
                call.release();
                self.agent.landing.push(Landing { call, path, text: reply.text });
                continue;
            }
            call.answer(reply);
        }
    }

    pub fn serve_call(&mut self, tool: &str, args: &Json) -> Option<mcp::Reply> {
        let edits = mcp::edits(tool);
        if edits && (self.dragging() || self.disk.exporting.is_some()) {
            return None;
        }
        self.agent.status.called(tool);
        let reply = mcp::serve(self, tool, args);
        if edits {
            self.finish_edit();
        }
        Some(reply)
    }

    fn browse(&mut self, call: mcp::Call) {
        self.agent.status.called(mcp::LIBRARY);
        let walk = match mcp::browsing(self, &call.args) {
            Ok(walk) => walk,
            Err(reply) => return call.answer(reply),
        };
        match audio::Job::spawn("bp-library", 0, move |_, stopped| Some(walk(stopped))) {
            Some(walk) => {
                call.release();
                self.agent.browsing.push(Browsing { call, walk });
            }
            None => call.answer(mcp::Reply::refused("The library would not start reading")),
        }
    }

    fn tick_browse(&mut self) {
        if self.agent.browsing.is_empty() {
            return;
        }
        let (ended, walking): (Vec<Browsing>, Vec<Browsing>) = std::mem::take(&mut self.agent.browsing).into_iter().partition(|browsing| browsing.walk.finished() || !browsing.call.awaited());
        self.agent.browsing = walking;
        for mut browsing in ended {
            if browsing.walk.finished() {
                let reply = browsing.walk.take().unwrap_or_else(|| mcp::Reply::refused("The library stopped before it had read the folders"));
                browsing.call.answer(reply);
            } else {
                browsing.walk.cancel();
            }
        }
    }

    fn listen(&mut self, call: mcp::Call) {
        self.agent.status.called(mcp::LISTEN);
        self.tick_listen();
        let details = call.args.get("details").and_then(Json::as_bool).unwrap_or(false);
        let (asked, project, against, spans, kept) = match mcp::listening(self, &call.args).and_then(|asked| self.asked(&asked, &call.args)) {
            Ok(found) => found,
            Err(text) => return call.answer(mcp::Reply::refused(text)),
        };
        if kept.is_none()
            && let Some(remembered) = self.agent.heard_before.iter().find(|remembered| remembered.asked.key == asked.key)
        {
            return call.answer(mcp::Reply::ok(report(remembered, &spans, details, None)));
        }
        if let Some(hearing) = &self.agent.hearing {
            return call.answer(mcp::Reply::refused(format!("Already listening to beats {} to {}; ask again once it answers", hearing.asked.span.0, hearing.asked.span.1)));
        }
        let solos: Vec<Option<CoreSolo>> = (0..project.tracks.len()).map(|track| asked.core.filter(|_| asked.track == Some(track))).collect();
        match start_listen(&project, against, &self.pool.waves, asked.ticks, asked.hosts.clone(), (asked.alone, solos), kept.as_ref().map(|(_, difference)| *difference)) {
            Some(listen) => {
                if let Some((path, _)) = &kept
                    && let Err(text) = mcp::housed(self, path)
                {
                    listen.cancel();
                    return call.answer(mcp::Reply::refused(text));
                }
                call.release();
                self.agent.hearing = Some(Hearing { call, listen, asked, spans, details, kept });
            }
            None => call.answer(mcp::Reply::refused("The render would not start")),
        }
    }

    fn asked(&self, asked: &mcp::Listening, args: &Json) -> Result<Asking, String> {
        let (start, end) = match asked.span {
            Some((from, until)) => (ticks(from.max(0.0)), ticks(until).min(LONGEST_TICKS)),
            None => (0, self.project.end()),
        };
        if end <= start {
            return Err(String::from("The song is empty; pass fromBeat and untilBeat"));
        }
        let span = (beats(start), beats(end));
        let spans: Vec<(f64, f64)> = asked.spans.iter().map(|&(from, until)| (from.max(span.0), until.min(span.1))).collect();
        if let Some(index) = spans.iter().position(|(from, until)| until <= from) {
            return Err(format!("spans {index}, beats {} to {}, lies outside beats {} to {}; pass fromBeat and untilBeat around it", asked.spans[index].0, asked.spans[index].1, span.0, span.1));
        }
        let track = asked.track.unwrap_or_default();
        let project = soloed(self.project.clone(), asked.track).map_err(|count| format!("There is no tracks/{track}: the song has {}", mcp::counted(count, "track")))?;
        let against = match &asked.against {
            Some((revision, document)) => {
                let reference = Project::from_json(document).ok_or_else(|| format!("Revision {revision} would not read as a project"))?;
                Some(soloed(reference, asked.track).map_err(|count| format!("Revision {revision} has no tracks/{track}: it had {}", mcp::counted(count, "track")))?)
            }
            None => None,
        };
        let revision = asked.against.as_ref().map(|(revision, _)| *revision);
        let core = match args.get("row").filter(|row| !matches!(row, Json::Null)) {
            None if args.get("effects").and_then(Json::as_bool).unwrap_or(false) => return Err(String::from("effects goes with row, the Core row to hear alone; pass row and track")),
            None => None,
            Some(row) => {
                let row = row.as_index(RACK_ROWS).ok_or("row is a rack row's number, a whole number from 0")?;
                let at = asked.track.map(|track| format!("tracks/{track}/rack/{row}")).ok_or("row names a Core row of the track given by track; pass track with it")?;
                let cored = |project: &Project| project.tracks[track].rack.get(row).is_some_and(Row::is_core);
                if !cored(&project) {
                    return Err(format!("{at} is not a Core row; row takes a Core to hear alone"));
                }
                if against.as_ref().is_some_and(|reference| !cored(reference)) {
                    return Err(format!("Revision {} has no Core at {at} to hear alone", revision.unwrap_or_default()));
                }
                Some(CoreSolo { row, effects: args.get("effects").and_then(Json::as_bool).unwrap_or(false) })
            }
        };
        let mut hasher = DefaultHasher::new();
        (self.project.to_json().write(), start, end, asked.track, asked.alone, core.map(|core| (core.row, core.effects)), revision, self.pool.kept).hash(&mut hasher);
        let heard = audio::heard(&project);
        let scheduled = project.tracks.iter().zip(&heard).map(|(track, heard)| *heard && track.clips.iter().any(|clip| clip.start < end && clip.start + clip.length > start)).collect();
        let (hosts, dynamics) = dynamics_of(&project);
        let names = project.tracks.iter().map(|track| track.name.clone()).collect();
        let kept = asked.path.clone().map(|path| (path, false)).or_else(|| asked.difference.clone().map(|path| (path, true)));
        Ok((Asked { key: hasher.finish(), revision: asked.revision, ticks: (start, end), span, bpm: project.bpm, track: asked.track, alone: asked.alone, core, against: revision, names, scheduled, hosts, dynamics }, project, against, spans, kept))
    }

    pub(super) fn tick_listen(&mut self) {
        if !self.agent.hearing.as_ref().is_some_and(|hearing| hearing.listen.finished()) {
            if let Some(gone) = self.agent.hearing.take_if(|hearing| !hearing.call.awaited()) {
                gone.listen.cancel();
            }
            return;
        }
        let Some(mut hearing) = self.agent.hearing.take() else {
            return;
        };
        let Some((listened, bytes)) = hearing.listen.take() else {
            return hearing.call.answer(mcp::Reply::refused("The render stopped before it ended"));
        };
        let remembered = Remembered { asked: hearing.asked, listened };
        if let Some(((path, difference), bytes)) = hearing.kept.zip(bytes) {
            let text = report(&remembered, &hearing.spans, hearing.details, Some((&path, difference)));
            self.requests.push(Request::Exported { path: path.clone(), bytes });
            self.agent.landing.push(Landing { call: hearing.call, path, text });
        } else if hearing.call.awaited() {
            hearing.call.answer(mcp::Reply::ok(report(&remembered, &hearing.spans, hearing.details, None)));
        }
        self.agent.heard_before.retain(|before| before.asked.key != remembered.asked.key);
        self.agent.heard_before.truncate(REMEMBERED - 1);
        self.agent.heard_before.insert(0, remembered);
    }

    pub(super) fn settle_server(&mut self) {
        let wanted = self.settings.mcp.then_some(self.settings.port);
        if let Some(server) = &self.agent.server {
            server.ask(self.settings.feedback);
        }
        if self.agent.server.as_ref().map(mcp::Server::port) == wanted {
            return;
        }
        self.agent.stop();
        if let Some(port) = wanted
            && let Some((server, calls)) = mcp::listen(port, self.settings.feedback)
        {
            self.agent.server = Some(server);
            self.agent.calls = Some(calls);
        }
    }

    fn session_fields(&self, fields: &mut Fields) {
        fields.name("build", if cfg!(debug_assertions) { "debug" } else { "release" }, "release");
        fields.flag("playing", self.playing, false);
        fields.number("endBeat", beats(self.project.end()), 0.0);
        if let Some(audio) = &self.audio {
            let timing = audio.timing();
            let mut latency = Fields::new();
            latency.push("outputSamples", Json::Number(timing.output as f64));
            latency.push("outputMs", Json::Number(timing.ms(timing.output)));
            latency.push("sampleRate", Json::Number(timing.rate));
            latency.list("tracks", timing.tracks.iter().map(|track| Json::Number(track.latency as f64)).collect());
            fields.push("latency", latency.into_json());
        }
        fields.push("selectedTrack", Json::Number(self.selection.track as f64));
        let clip_path = |&(track, start): &(usize, i32)| self.project.tracks.get(track).and_then(|owner| owner.clips.iter().position(|clip| clip.start == start)).map(|index| Json::String(format!("tracks/{track}/clips/{index}")));
        fields.list("selectedClips", self.selection.clips.iter().filter_map(clip_path).collect());
        if let Some(region) = &self.selection.region {
            fields.push("selectionStartBeat", Json::Number(beats(region.start)));
            fields.push("selectionEndBeat", Json::Number(beats(region.end)));
            fields.list("selectedTracks", self.region_tracks(region).into_iter().map(|track| Json::Number(track as f64)).collect());
        }
        if let Some((host, row)) = self.shown().filter(|_| self.core_shown()) {
            let owner = match host {
                crate::project::Host::Track(track) => format!("tracks/{track}"),
                crate::project::Host::Send(send) => format!("returns/{send}"),
                crate::project::Host::Master => String::from("master"),
            };
            fields.push("openRow", Json::String(format!("{owner}/rack/{row}")));
        }
        let Some((track, index)) = self.edited_clip().filter(|_| self.roll_shown()) else {
            return;
        };
        fields.push("openClip", Json::String(format!("tracks/{track}/clips/{index}")));
        let clip = &self.project.tracks[track].clips[index];
        let picked = self.roll.copy_picked(clip);
        fields.list("pickedNotes", clip.notes.iter().enumerate().filter(|(_, note)| picked.contains(note)).map(|(at, _)| Json::Number(at as f64)).collect());
        let (lowest, highest) = self.roll.shown_pitches();
        fields.list("shownPitches", vec![Json::Number(f64::from(lowest)), Json::Number(f64::from(highest))]);
        let (from, until) = self.roll.shown_beats(clip);
        let start = beats(clip.start);
        fields.list("shownBeats", vec![Json::Number(json::tidy(start + f64::from(from))), Json::Number(json::tidy(start + f64::from(until)))]);
    }

    pub(super) fn session_manual(fields: &mut Fields) {
        fields.push("build", Json::from("debug when the app is a debug build; read only, left out for release"));
        fields.push("playing", Json::from("true or false, default false: the transport is running; read only"));
        fields.push("endBeat", Json::from("number: the beat where the last clip ends; read only, left out when empty"));
        fields.push("latency", Json::from("read only: outputSamples and outputMs, how long after the playhead the sound leaves at sampleRate, delayMs included; tracks, each track's latency in samples, delayMs left out. Latency comes from lookahead, lookaheadMs under limiterOn without hardClip, Space, Time, a Suppressor row, a linear crossoverSlope with bandCount above 1, and Freak's oversample, multiplied by the highest times among its bands; limiterOn, hardClip, lookaheadMs, crossoverSlope and times take no lanes"));
        fields.push("selectedTrack", Json::from(format!("whole number 0 to {}: the selected track's index; read only", crate::project::TRACKS - 1)));
        fields.push("selectedClips", Json::from("paths of the clips selected in the arrangement; read only, left out for none"));
        fields.push("selectionStartBeat", Json::from("number: start of the time selection drawn on the arrangement; read only, left out for none"));
        fields.push("selectionEndBeat", Json::from("number: end of that time selection; read only"));
        fields.push("selectedTracks", Json::from("whole numbers: the tracks the time selection covers; read only, left out when it covers automation lanes alone"));
        fields.push("openRow", Json::from("path of the rack row whose face the app shows, on a track, a return or the master; read only, left out when none shows, as under the piano roll, the chord palette or a big view"));
        fields.push("openClip", Json::from("path of the MIDI clip open in the piano roll; read only, left out for none"));
        fields.push("pickedNotes", Json::from("whole numbers: indices of the notes picked in openClip; read only, left out for none"));
        fields.push("shownPitches", Json::from("two whole numbers: the lowest and highest pitches the piano roll shows; read only"));
        fields.push("shownBeats", Json::from("two numbers: the song beats of openClip the piano roll shows; read only"));
    }

    fn follow_tracks(&mut self, shifts: &[mcp::Shift], landed: &Project) {
        for (done, shift) in shifts.iter().enumerate() {
            let count = self.project.tracks.len();
            match *shift {
                mcp::Shift::Insert(at) if at <= count => {
                    let mut track = landed_at(&shifts[done + 1..], at).and_then(|at| landed.tracks.get(at)).cloned().unwrap_or_else(Track::blank);
                    track.renumber(|_| None);
                    self.insert_track(at, track, false);
                }
                mcp::Shift::Remove(at) if at < count => {
                    self.remove_track(at);
                }
                mcp::Shift::Move(from, to) if from.max(to) < count => self.move_track(from, to),
                _ => return,
            }
        }
    }

    pub(super) fn follow_rows(&mut self, landed: &Project, origin: impl Fn(usize) -> Option<usize>) -> Vec<usize> {
        let shown = self.host();
        let mut picked = self.rack.picked();
        let tracks = (0..landed.tracks.len()).filter_map(|at| origin(at).filter(|&from| from < self.project.tracks.len()).map(|from| (crate::project::Host::Track(from), crate::project::Host::Track(at))));
        let returns = (0..SEND_COUNT).map(crate::project::Host::Send).chain(std::iter::once(crate::project::Host::Master)).map(|host| (host, host));
        let pairs: Vec<(crate::project::Host, crate::project::Host)> = tracks.chain(returns).collect();
        for (host, lands_at) in pairs {
            let (old, new) = (self.project.rack(host), landed.rack(lands_at));
            if old == new {
                continue;
            }
            let landings = landings(old, new);
            let lands = |row: usize| landings.get(row).copied().flatten();
            let kept = |armed: &mut Option<automation::Armed>| {
                armed.take_if(|armed| !armed.aim.remap(&lands));
            };
            if host == shown {
                self.open = self.open.and_then(lands);
                kept(&mut self.armed);
                picked = picked.into_iter().filter_map(lands).collect();
                picked.sort_unstable();
                picked.dedup();
            }
            match host {
                crate::project::Host::Track(track) => {
                    if let Some(generator) = self.generators.get_mut(track) {
                        *generator = followed(std::mem::take(generator), &landings);
                    }
                }
                crate::project::Host::Send(send) => kept(&mut self.armed_returns[send]),
                crate::project::Host::Master => kept(&mut self.armed_returns[SEND_COUNT]),
            }
        }
        picked
    }

    fn transport_moved(&self, document: &Json) -> bool {
        let mut state = Fields::new();
        self.transport_fields(&mut state);
        let state = state.into_json();
        [("playheadBeat", 0.0), ("loopStartBeat", -1.0), ("loopEndBeat", -1.0)].iter().any(|(key, default)| number(Some(document), key, *default) != number(Some(&state), key, *default))
    }
}

pub(super) fn followed(mut generator: Generator, landings: &[Option<usize>]) -> Generator {
    let lands = |row: usize| landings.get(row).copied().flatten();
    generator.core = generator.core.and_then(|solo| lands(solo.row).map(|row| CoreSolo { row, ..solo }));
    generator.open = generator.open.and_then(lands);
    generator.armed.take_if(|armed| !armed.aim.remap(&lands));
    generator
}

pub(super) fn landings(old: &[Row], new: &[Row]) -> Vec<Option<usize>> {
    let same = |was: &Row, now: &Row| match (was, now) {
        (Row::Core(mine), Row::Core(theirs)) => mine.seed == theirs.seed,
        _ => was == now,
    };
    let better = |a: (usize, usize), b: (usize, usize)| if (a.0, Reverse(a.1)) >= (b.0, Reverse(b.1)) { a } else { b };
    let paired = |best: &[Vec<(usize, usize)>], was: usize, now: usize| (best[was][now].0 + 1, best[was][now].1 + was.abs_diff(now));
    let mut best = vec![vec![(0, 0); new.len() + 1]; old.len() + 1];
    for was in 0..old.len() {
        for now in 0..new.len() {
            let skipped = better(best[was][now + 1], best[was + 1][now]);
            best[was + 1][now + 1] = if same(&old[was], &new[now]) { better(paired(&best, was, now), skipped) } else { skipped };
        }
    }
    let mut lands = vec![None; old.len()];
    let (mut was, mut now) = (old.len(), new.len());
    while was > 0 && now > 0 {
        if same(&old[was - 1], &new[now - 1]) && best[was][now] == paired(&best, was - 1, now - 1) {
            lands[was - 1] = Some(now - 1);
            (was, now) = (was - 1, now - 1);
        } else if best[was][now] == best[was - 1][now] {
            was -= 1;
        } else {
            now -= 1;
        }
    }
    let mut free: Vec<bool> = (0..new.len()).map(|now| !lands.contains(&Some(now))).collect();
    let passes: [&dyn Fn(usize, usize) -> bool; 3] = [&|was: usize, now: usize| same(&old[was], &new[now]), &|was: usize, now: usize| old[was].same_kind(&new[now]), &|was: usize, now: usize| was == now];
    for alike in passes {
        for (was, land) in lands.iter_mut().enumerate() {
            if land.is_none()
                && let Some(now) = (0..new.len()).find(|&now| free[now] && alike(was, now))
            {
                *land = Some(now);
                free[now] = false;
            }
        }
    }
    let twin = |was: usize| (0..old.len()).filter(|&twin| lands[twin].is_some() && old[twin] == old[was]).min_by_key(|&twin| twin.abs_diff(was)).and_then(|twin| lands[twin]);
    (0..old.len()).map(|was| lands[was].or_else(|| twin(was))).collect()
}

fn landed_at(shifts: &[mcp::Shift], track: usize) -> Option<usize> {
    shifts.iter().try_fold(track, |at, shift| match *shift {
        mcp::Shift::Insert(new) => Some(at + usize::from(at >= new)),
        mcp::Shift::Remove(gone) => (at != gone).then(|| at - usize::from(at > gone)),
        mcp::Shift::Move(from, to) => Some(moved_index(at, from, to)),
    })
}

fn named(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => String::from(*one),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn folder_name(path: &str) -> String {
    std::path::Path::new(path).file_name().and_then(|name| name.to_str()).map_or_else(|| String::from(path), String::from)
}

fn soloed(mut project: Project, track: Option<usize>) -> Result<Project, usize> {
    if let Some(track) = track {
        if track >= project.tracks.len() {
            return Err(project.tracks.len());
        }
        for (index, each) in project.tracks.iter_mut().enumerate() {
            each.solo = index == track;
        }
    }
    Ok(project)
}

fn dynamics_of(project: &Project) -> (Vec<crate::project::Host>, Vec<Dynamic>) {
    let tracks = (0..project.tracks.len()).map(|track| (crate::project::Host::Track(track), format!("tracks/{track}")));
    let sends = (0..SEND_COUNT).map(|send| (crate::project::Host::Send(send), format!("returns/{send}")));
    let (mut hosts, mut dynamics) = (Vec::new(), Vec::new());
    for (host, at) in tracks.chain(sends).chain(std::iter::once((crate::project::Host::Master, String::from("master")))) {
        let found: Vec<Dynamic> = project
            .rack(host)
            .iter()
            .enumerate()
            .filter_map(|(row, each)| match each {
                Row::Effect(slot) if slot.kind().meters_reduction() || matches!(&slot.params, Params::Utility(utility) if utility.limit) => Some(Dynamic { host: hosts.len(), row, path: format!("{at}/rack/{row}"), kind: slot.kind().name() }),
                _ => None,
            })
            .collect();
        if !found.is_empty() {
            hosts.push(host);
            dynamics.extend(found);
        }
    }
    (hosts, dynamics)
}

fn report(remembered: &Remembered, spans: &[(f64, f64)], details: bool, wav: Option<(&str, bool)>) -> String {
    let Remembered { asked, listened } = remembered;
    let heard = &listened.heard;
    let tenths = |value: f64| Json::Number((value * 10.0).round() / 10.0);
    let seconds = |beat: f64| (beat - asked.span.0) * 60.0 / asked.bpm;
    let beat = |seconds: f64| Json::Number(((asked.span.0 + seconds * asked.bpm / 60.0) * 1000.0).round() / 1000.0);
    let transients = |from: f64, until: f64| {
        let mut out = Vec::new();
        if let Some(at) = heard.peak_at(from, until) {
            out.push(("peakBeat", beat(at)));
        }
        let hits = heard.hits(from, until);
        if !hits.is_empty() {
            out.push(("hits", Json::Array(hits.iter().take(MOST_HITS).map(|hit| Json::Array(vec![beat(hit.seconds), tenths(amplitude_db(hit.peak)), Json::Number(hit.hz.round())])).collect())));
        }
        if hits.len() > MOST_HITS {
            out.push(("moreHits", Json::Number((hits.len() - MOST_HITS) as f64)));
        }
        out
    };
    let bands = |heard: &Heard| heard.bands_db.map_or(Json::Null, |bands| mcp::object(BANDS.iter().zip(bands).map(|((name, _, _), db)| (*name, tenths(db))).collect()));
    let mut fields = vec![("revision", Json::Number(asked.revision as f64)), ("fromBeat", Json::Number(asked.span.0)), ("untilBeat", Json::Number(asked.span.1)), ("seconds", tenths(heard.seconds))];
    if let Some(track) = asked.track {
        fields.push(("track", Json::Number(track as f64)));
    }
    if let Some(core) = asked.core {
        fields.push(("row", Json::Number(core.row as f64)));
        fields.push(("effects", Json::Bool(core.effects)));
    }
    if let Some(send) = asked.alone {
        fields.push(("return", Json::Number(send as f64)));
    }
    if let Some((wav, false)) = wav {
        fields.push(("wav", mcp::text(wav)));
    }
    if let (Some(against), Some(null)) = (asked.against, &listened.null) {
        let mut entry = vec![("against", Json::Number(against as f64)), ("peakDb", tenths(null.peak_db)), ("lufs", tenths(null.lufs.unwrap_or(FLOOR_DB)))];
        if let Some((wav, true)) = wav {
            entry.insert(1, ("wav", mcp::text(wav)));
        }
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for (bar, _) in null.bars.iter().enumerate().filter(|(_, (peak, _))| *peak > FLOOR_DB) {
            match ranges.last_mut() {
                Some(last) if last.1 == bar => last.1 = bar + 1,
                _ => ranges.push((bar, bar + 1)),
            }
        }
        if let Some(at) = null.peak_at(0.0, null.seconds).filter(|_| null.peak_db > FLOOR_DB) {
            entry.push(("peakBeat", beat(at)));
        }
        if !ranges.is_empty() {
            let edge = |bar: usize| Json::Number((asked.span.0 + bar as f64 * f64::from(BEATS_PER_BAR)).min(asked.span.1));
            entry.push(("differs", Json::Array(ranges.iter().take(MOST_RANGES).map(|&(from, until)| Json::Array(vec![edge(from), edge(until)])).collect())));
        }
        if ranges.len() > MOST_RANGES {
            entry.push(("moreDiffers", Json::Number((ranges.len() - MOST_RANGES) as f64)));
        }
        if !spans.is_empty() {
            entry.push(("spans", Json::Array(spans.iter().map(|&(from, until)| tenths(null.between(seconds(from), seconds(until)).1)).collect())));
        }
        fields.push(("null", mcp::object(entry)));
    }
    let over = amplitude_db(listened.over).max(0.0);
    fields.extend([
        ("lufs", tenths(heard.lufs.unwrap_or(FLOOR_DB))),
        ("peakDb", tenths(heard.peak_db)),
        ("truePeakDb", tenths(heard.true_peak_db)),
        ("overDb", tenths(over)),
        ("rmsDb", tenths(heard.rms_db)),
        ("crestDb", tenths(heard.peak_db - heard.rms_db)),
        ("correlation", heard.correlation.map_or(Json::Null, |correlation| Json::Number((correlation * 100.0).round() / 100.0))),
        ("sideDb", tenths(heard.side_db)),
        ("bandsDb", bands(heard)),
    ]);
    fields.extend(transients(0.0, heard.seconds).into_iter().filter(|(key, _)| spans.is_empty() || *key == "peakBeat"));
    if details || spans.is_empty() {
        let rows = heard.bars.iter().map(|&(peak, lufs)| Json::Array(vec![tenths(peak), tenths(lufs)])).collect();
        fields.push(("bars", mcp::object(vec![("beatsEach", Json::Number(f64::from(BEATS_PER_BAR))), ("columns", Json::Array(vec![mcp::text("peakDb"), mcp::text("lufs")])), ("rows", Json::Array(rows))])));
    }
    let sections: Vec<(f64, f64)> = spans.iter().map(|&(from, until)| heard.between(seconds(from), seconds(until))).collect();
    if !spans.is_empty() {
        let measured = spans.iter().zip(&sections).map(|(&(from, until), &(lufs, peak))| {
            let mut entry = vec![("fromBeat", Json::Number(from)), ("untilBeat", Json::Number(until)), ("lufs", tenths(lufs)), ("peakDb", tenths(peak))];
            entry.extend(transients(seconds(from), seconds(until)));
            mcp::object(entry)
        });
        fields.push(("spans", Json::Array(measured.collect())));
    }
    let track_peak = |index: usize| listened.tracks.get(index).and_then(Option::as_ref).map_or(FLOOR_DB, |heard| heard.peak_db);
    let mix = asked.track.is_none() && asked.alone.is_none();
    if mix {
        if details {
            fields.push(("measured", mcp::text("Mix: post-master. Tracks: after their rack and gain, before returns and master. Returns: after their rack and gain, before master.")));
        }
        let sounding = asked.names.iter().enumerate().filter_map(|(index, name)| {
            let heard = listened.tracks.get(index)?.as_ref().filter(|heard| heard.peak_db > FLOOR_DB)?;
            let mut fields = vec![("path", Json::String(format!("tracks/{index}"))), ("name", Json::String(name.clone())), ("lufs", tenths(heard.lufs.unwrap_or(FLOOR_DB))), ("peakDb", tenths(heard.peak_db)), ("bandsDb", bands(heard))];
            if !spans.is_empty() {
                fields.push(("spans", Json::Array(spans.iter().map(|&(from, until)| tenths(heard.between(seconds(from), seconds(until)).0)).collect())));
            }
            Some(mcp::object(fields))
        });
        fields.push(("tracks", Json::Array(sounding.collect())));
        let returning = listened.returns.iter().enumerate().filter_map(|(index, heard)| {
            let heard = heard.as_ref().filter(|heard| heard.peak_db > FLOOR_DB)?;
            let mut fields = vec![("path", Json::String(format!("returns/{index}"))), ("lufs", tenths(heard.lufs.unwrap_or(FLOOR_DB))), ("peakDb", tenths(heard.peak_db)), ("bandsDb", bands(heard))];
            if !spans.is_empty() {
                fields.push(("spans", Json::Array(spans.iter().map(|&(from, until)| tenths(heard.between(seconds(from), seconds(until)).0)).collect())));
            }
            Some(mcp::object(fields))
        });
        fields.push(("returns", Json::Array(returning.collect())));
    }
    let hops = listened.reductions.len();
    let reduction = |dynamic: &Dynamic, from: usize, until: usize| f64::from(listened.reductions[from.min(hops)..until.min(hops)].iter().map(|hop| hop.get(dynamic.host).and_then(|rows| rows.get(dynamic.row)).copied().unwrap_or(0.0)).fold(0.0, f32::max));
    let reduced: Vec<(&Dynamic, f64)> = asked.dynamics.iter().map(|dynamic| (dynamic, reduction(dynamic, 0, hops))).collect();
    let dynamics = reduced.iter().filter(|(_, db)| details || *db > 0.0).map(|(dynamic, db)| {
        let mut fields = vec![("path", Json::String(dynamic.path.clone())), ("kind", mcp::text(dynamic.kind)), ("reductionDb", tenths(*db))];
        if !spans.is_empty() {
            fields.push(("spans", Json::Array(spans.iter().map(|&(from, until)| tenths(reduction(dynamic, hop_of(seconds(from)), hop_of(seconds(until)).max(hop_of(seconds(from)) + 1)))).collect())));
        }
        mcp::object(fields)
    });
    fields.push(("dynamics", Json::Array(dynamics.collect())));
    let mut flags = Vec::new();
    if over > CLIPPED_DB {
        flags.push(format!("Master clipping: {over:.1} dB above full scale before clamping."));
    }
    if heard.true_peak_db > CLIPPED_DB {
        flags.push(format!("True peak: {:.1} dB above full scale.", heard.true_peak_db));
    }
    for (dynamic, db) in reduced.iter().filter(|(dynamic, db)| dynamic.path.starts_with("master") && *db > PINNED_DB) {
        flags.push(format!("{} {}: up to {db:.1} dB gain reduction.", dynamic.path, dynamic.kind));
    }
    for (index, name) in asked.names.iter().enumerate().filter(|(index, _)| mix && asked.scheduled.get(*index).copied().unwrap_or(false) && track_peak(*index) < SILENT_DB) {
        flags.push(format!("tracks/{index} {name}: clips present but nearly silent, peak {:.0} dB.", track_peak(index)));
    }
    let sounding = |track: usize, (from, until): (f64, f64)| listened.tracks.get(track).and_then(Option::as_ref).is_some_and(|heard| heard.between(seconds(from), seconds(until)).0 > HEARD_LUFS);
    let crossing = |from: (f64, f64), to: (f64, f64)| asked.names.iter().enumerate().filter(|(track, _)| !sounding(*track, from) && sounding(*track, to)).map(|(_, name)| name.as_str()).collect::<Vec<&str>>();
    for (index, pair) in sections.windows(2).enumerate() {
        let (before, after) = (pair[0].0, pair[1].0);
        if !(before > HEARD_LUFS && after > HEARD_LUFS && (after - before).abs() < FLAT_LU) {
            continue;
        }
        let (first, second) = (spans[index], spans[index + 1]);
        let (entering, leaving) = (crossing(first, second), crossing(second, first));
        let change = match (entering.is_empty(), leaving.is_empty()) {
            (true, true) => continue,
            (false, true) => format!("{} {}", named(&entering), if entering.len() == 1 { "enters" } else { "enter" }),
            (true, false) => format!("{} {}", named(&leaving), if leaving.len() == 1 { "leaves" } else { "leave" }),
            (false, false) => format!("{} {} and {} {}", named(&entering), if entering.len() == 1 { "enters" } else { "enter" }, named(&leaving), if leaving.len() == 1 { "leaves" } else { "leave" }),
        };
        let word = if after < before { "quieter" } else { "louder" };
        flags.push(format!("spans {index} and {}: {change} at one loudness, {:.1} LU {word}", index + 1, (after - before).abs()));
    }
    if heard.correlation.is_some_and(|correlation| correlation < 0.0) {
        flags.push(String::from("Negative stereo correlation: check mono compatibility."));
    }
    if !flags.is_empty() {
        fields.push(("flags", Json::Array(flags.into_iter().map(Json::String).collect())));
    }
    mcp::object(fields).write()
}

impl Host for App {
    fn document(&self) -> Json {
        let mut session = Fields::new();
        session.list("folders", self.browser.folders().into_iter().map(Json::String).collect());
        self.session_fields(&mut session);
        let mut document = files::before_tracks(self.with_state(self.project.to_json()), session);
        let mut names = Vec::new();
        wave_names(&document, &mut names);
        let pool = self.pool.waves_summary(&names);
        if !pool.is_empty()
            && let Json::Object(fields) = &mut document
        {
            fields.push((String::from("waves"), Json::Array(pool)));
        }
        document
    }

    fn manual(&self) -> Json {
        files::manual()
    }

    fn follow(&mut self, shifts: &[mcp::Shift]) {
        self.agent.shifts = shifts.to_vec();
    }

    fn adopt(&mut self, document: &Json) -> Result<(), String> {
        let shifts = std::mem::take(&mut self.agent.shifts);
        let project = Project::from_json(document).ok_or_else(|| String::from("The document would not read as a project"))?;
        self.follow_tracks(&shifts, &project);
        let picked = self.follow_rows(&project, Some);
        self.settle_project(project, Some);
        self.pick_rows(picked);
        if self.transport_moved(document) {
            self.transport_from_json(document);
        }
        Ok(())
    }

    fn connected(&mut self, client: String) {
        self.agent.status.connected(client);
    }

    fn import(&mut self, path: &str) -> Result<Json, String> {
        self.pool.import_wave(path)
    }

    fn play_from(&mut self, beat: Option<f64>, until: Option<f64>) -> Result<String, String> {
        let tick = |beat: f64| ticks(beat.max(0.0)).min(LONGEST_TICKS);
        let start = beat.map_or(self.play_from, tick);
        let end = until.map(tick);
        if let Some(end) = end.filter(|end| *end <= start) {
            return Err(format!("untilBeat {} is not after beat {}, where play starts", beats(end), beats(start)));
        }
        self.mark_start(start);
        if let Some(end) = end {
            self.ruler.remember(Some(Loop { start, end }));
        }
        self.play();
        Ok(match self.loop_beats() {
            Some((from, to)) => format!("Playing from beat {}, looping beats {from} to {to}", beats(start)),
            None => format!("Playing from beat {}", beats(start)),
        })
    }

    fn stop_playing(&mut self) -> String {
        self.stop();
        String::from("Stopped")
    }

    fn step_back(&mut self) -> Option<String> {
        let step = self.history.undo()?;
        self.restore(step);
        Some(String::from("Undone"))
    }

    fn step_forward(&mut self) -> Option<String> {
        let step = self.history.redo()?;
        self.restore(step);
        Some(String::from("Redone"))
    }

    fn shelves(&self) -> Vec<Shelf> {
        let factory = self.factory_folders().into_iter().map(|path| (path, true));
        let folders = self.browser.folders().into_iter().map(|path| (path, false));
        factory.chain(folders).map(|(path, fixed)| Shelf { name: folder_name(&path), path, fixed }).collect()
    }

    fn relist(&mut self, folder: String) {
        self.requests.push(Request::List { path: folder });
    }

    fn trash(&mut self, path: String) {
        self.requests.push(Request::Delete { path });
    }

    fn hearts(&self) -> Vec<String> {
        self.browser.hearts().to_vec()
    }

    fn heart(&mut self, path: String, on: bool) -> bool {
        let changed = self.browser.set_heart(&path, on);
        self.keep_hearts();
        changed
    }

    fn revisions(&mut self) -> &mut mcp::Revisions {
        &mut self.agent.revisions
    }

    fn project_path(&self) -> Option<String> {
        self.disk.path.clone()
    }

    fn save(&mut self, path: String) -> String {
        let own = self.disk.path.as_deref() == Some(path.as_str());
        let (json, save) = self.file_json(!own);
        self.assured(save);
        self.requests.push(Request::Write { path: path.clone(), json, save });
        if own { format!("Saving to {path}, the project's own file") } else { format!("Saving to {path}, which becomes the project's file") }
    }

    fn save_copy(&mut self, path: String) -> String {
        let json = self.copy_json();
        self.requests.push(Request::Exported { path: path.clone(), bytes: json.into_bytes() });
        format!("Writing a copy to {path}; the project keeps its own file")
    }

    fn render(&mut self, path: String, span: Option<(f64, f64)>) -> Result<String, String> {
        if self.disk.exporting.is_some() {
            return Err(String::from("An export is rendering already; wait until it lands"));
        }
        let (start, end) = match span {
            Some((from, until)) => (ticks(from.max(0.0)), ticks(until).min(LONGEST_TICKS)),
            None => self.export_span().ok_or("The song is empty with no loop; pass fromBeat and untilBeat")?,
        };
        let tail = if span.is_some_and(|(_, until)| ticks(until) < self.project.end()) || (span.is_none() && self.ruler.range().is_some()) { audio::Tail::Cut } else { audio::Tail::Rings };
        self.export_between(path.clone(), (start, end), tail);
        if self.disk.exporting.is_none() {
            return Err(String::from("The render would not start"));
        }
        if let Err(text) = mcp::housed(self, &path) {
            self.cancel_export();
            return Err(text);
        }
        Ok(format!("Rendered beats {} to {} into {path}", beats(start), beats(end)))
    }

    fn export_midi(&mut self, parts: &[(String, Vec<Json>)], bpm: Option<f64>, span: Option<(f64, f64)>, path: String) -> Result<String, String> {
        let bpm = match bpm {
            Some(bpm) if (LEAST_BPM..=MOST_BPM).contains(&bpm) => bpm,
            Some(_) => return Err(format!("bpm is {LEAST_BPM} to {MOST_BPM}")),
            None => self.project.bpm,
        };
        let (start, end) = span.map_or((0, LONGEST_TICKS), |(from, until)| (ticks(from.max(0.0)), ticks(until).min(LONGEST_TICKS)));
        let mut tracks: Vec<(String, Vec<crate::project::Clip>)> = Vec::new();
        for (name, clips) in parts {
            let mut kept = Vec::new();
            for clip in clips {
                let clip = crate::project::codec::clip_from_json(clip).filter(|clip| clip.file.is_none()).ok_or_else(|| format!("{name} holds a clip that is not MIDI: startBeat, lengthBeats and notes, without a wave"))?;
                if !clip.notes.is_empty() && clip.start < end && clip.end() > start {
                    kept.push(clip);
                }
            }
            if !kept.is_empty() {
                tracks.push((name.clone(), kept));
            }
        }
        if tracks.is_empty() {
            return Err(String::from(if span.is_some() { "No notes between fromBeat and untilBeat" } else { "No notes to write" }));
        }
        let from = if span.is_some() { start } else { tracks.iter().flat_map(|(_, clips)| clips).map(|clip| clip.start).min().unwrap_or(0) };
        for clip in tracks.iter_mut().flat_map(|(_, clips)| clips) {
            *clip = clip.between(from, end, bpm);
        }
        let notes: usize = tracks.iter().flat_map(|(_, clips)| clips).map(|clip| clip.notes.len()).sum();
        let bytes = midi::write(&tracks, bpm);
        mcp::housed(self, &path)?;
        self.requests.push(Request::Exported { path: path.clone(), bytes });
        let spread = if tracks.len() == 1 { String::new() } else { format!(" on {} tracks", tracks.len()) };
        Ok(format!("Wrote {}{spread} from beat {} at {bpm} bpm into {path}", mcp::counted(notes, "note"), beats(from)))
    }

    fn ask_folder(&mut self, at: Option<String>) {
        self.requests.push(Request::Folder { at });
    }

    fn sampled(&self, steps: &[String], beats: &[f64]) -> Result<Vec<f64>, String> {
        mcp::sampled(&self.project, &audio::sent(&self.project), steps, beats)
    }

    fn sent(&self, project: &Project) -> flatten::Sent {
        audio::sent(project)
    }

    fn keep(&mut self, path: String, text: String) {
        self.requests.push(Request::Exported { path, bytes: text.into_bytes() });
    }

    fn alike(&self, clips: &[crate::project::Clip], like: usize) -> Vec<usize> {
        self.alike_among(clips, like)
    }

    fn heard_until(&self, clip: &crate::project::Clip) -> Option<f64> {
        App::heard_until(self, clip)
    }

    fn export_clip(&mut self, track: usize, index: usize, span: Option<(f64, f64)>, path: String) -> Result<String, String> {
        let at = format!("tracks/{track}/clips/{index}");
        let clip = self.project.tracks.get(track).and_then(|owner| owner.clips.get(index)).cloned().ok_or_else(|| format!("No clip at {at}"))?;
        if clip.file.is_none() {
            return Err(format!("{at} holds notes; a .wav takes an audio clip, and a .mid a MIDI one"));
        }
        let (start, end) = span.map_or((clip.start, clip.end()), |(from, until)| (ticks(from).max(clip.start), ticks(until).min(clip.end())));
        if end <= start {
            return Err(format!("fromBeat and untilBeat lie outside {at}, which plays beats {} to {}", beats(clip.start), beats(clip.end())));
        }
        let (wave, lowered) = self.bounced(std::slice::from_ref(&clip), start, end, str::to_owned).map_err(|reason| if reason == TOO_LONG { format!("{at} is too long to write as one wave") } else { format!("{at}: its wave is missing or empty; set wave.path or import it") })?;
        mcp::housed(self, &path)?;
        let seconds = wave.seconds();
        let fitted = if lowered > 0.0 { format!(", lowered {lowered:.1} dB to stay under full scale") } else { String::new() };
        self.requests.push(Request::Exported { path: path.clone(), bytes: wave.wav() });
        Ok(format!("Wrote {at} as heard, beats {} to {}, {seconds:.3} s{fitted}, into {path}", beats(start), beats(end)))
    }
}

type Listen = audio::Job<(Listened, Option<Vec<u8>>)>;

pub(super) struct Listened {
    heard: Heard,
    null: Option<Heard>,
    reductions: Vec<Vec<[f32; RACK_ROWS]>>,
    over: f32,
    tracks: Vec<Option<Heard>>,
    returns: Vec<Option<Heard>>,
}

struct Nulled {
    ear: Ear,
    left: Vec<f32>,
    right: Vec<f32>,
    at: usize,
}

impl Nulled {
    fn hear(&mut self, left: &[f32], right: &[f32]) -> (&[f32], &[f32]) {
        let end = self.at + left.len();
        if self.left.len() < end {
            self.left.resize(end, 0.0);
            self.right.resize(end, 0.0);
        }
        let heard = std::mem::replace(&mut self.at, end)..end;
        for (held, now) in self.left[heard.clone()].iter_mut().zip(left).chain(self.right[heard.clone()].iter_mut().zip(right)) {
            *held = now - *held;
        }
        self.ear.hear(&self.left[heard.clone()], &self.right[heard.clone()]);
        (&self.left[heard.clone()], &self.right[heard])
    }

    fn finish(mut self) -> Heard {
        self.ear.hear(&self.left[self.at..], &self.right[self.at..]);
        self.ear.finish()
    }
}

fn start_listen(project: &Project, against: Option<Project>, waves: &[Arc<Wave>], span: (i32, i32), hosts: Vec<crate::project::Host>, (alone, solos): (Option<usize>, Vec<Option<CoreSolo>>), keep: Option<bool>) -> Option<Listen> {
    let expected = audio::Reach::of(project.bpm, span, Tail::Cut)?.expected();
    let bar = (frames_per_beat(EXPORT_SAMPLE_RATE, project.bpm) * f64::from(BEATS_PER_BAR)).round() as usize;
    let hop = ((EXPORT_SAMPLE_RATE * HOP_SECONDS).round() as usize).max(1);
    let (project, waves) = (project.clone(), waves.to_vec());
    audio::Job::spawn("bp-listen", expected, move |counted, stopped| {
        let mut nulled = match &against {
            Some(reference) => {
                let (mut left, mut right) = (Vec::new(), Vec::new());
                let held = |l: &[f32], r: &[f32]| {
                    left.extend_from_slice(l);
                    right.extend_from_slice(r);
                };
                render_span(reference, &waves, span, Tail::Cut, alone, &solos, &AtomicUsize::new(0), stopped, None, held, |_, _| {}, |_| {})?;
                Some(Nulled { ear: Ear::sparse(EXPORT_SAMPLE_RATE, bar), left, right, at: 0 })
            }
            None => None,
        };
        let mut ear = Ear::new(EXPORT_SAMPLE_RATE, bar);
        let mut file = keep.map(|_| Wav::new(expected));
        let (mut reductions, mut current, mut over, mut tracks, mut returns) = (Vec::new(), vec![[0.0; RACK_ROWS]; hosts.len()], 0.0, Vec::new(), Vec::new());
        let hear = |left: &[f32], right: &[f32]| {
            ear.hear(left, right);
            let changed = nulled.as_mut().map(|nulled| nulled.hear(left, right)).filter(|_| keep == Some(true));
            if let Some(file) = &mut file {
                let (left, right) = changed.unwrap_or((left, right));
                file.append(left, right);
            }
        };
        let gauged = |audio: &Audio, done: usize| {
            audio.deepen(&hosts, &mut current);
            if reductions.len() < done / hop {
                reductions.resize(done / hop, current.clone());
                current.iter_mut().for_each(|rows| *rows = [0.0; RACK_ROWS]);
            }
        };
        render_span(&project, &waves, span, Tail::Cut, alone, &solos, counted, stopped, Some(bar), hear, gauged, |audio| {
            over = audio.master_peak();
            tracks = audio.heard_tracks();
            returns = audio.heard_returns();
        })?;
        reductions.push(current);
        Some((Listened { heard: ear.finish(), null: nulled.map(Nulled::finish), reductions, over, tracks, returns }, file.map(Wav::finish)))
    })
}
