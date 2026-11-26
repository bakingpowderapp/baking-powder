use super::*;
use crate::project::RowTiming;

enum Reshape {
    Remove(usize),
    Insert(usize),
    Move(usize, usize),
}

fn lane_lives(rack: &[Row], lfo: &LaneLfo) -> bool {
    match &lfo.aim {
        Aim::Gain | Aim::Send(_) => true,
        Aim::Level(row) | Aim::Wet(row) => rack.get(*row).is_some_and(|entry| entry.mix_aim(*row).as_ref() == Some(&lfo.aim)),
        Aim::Knob { row, band, name } => rack.get(*row).is_some_and(|row| row.automatable(band, name)),
    }
}

pub(in crate::audio) fn beats_of(cache: &mut Vec<(Arc<Wave>, Arc<Beats>)>, wave: &Arc<Wave>) -> Arc<Beats> {
    if let Some((_, beats)) = cache.iter().find(|(known, _)| Arc::ptr_eq(known, wave)) {
        return Arc::clone(beats);
    }
    let beats = Arc::new(Beats::of(wave));
    cache.push((Arc::clone(wave), Arc::clone(&beats)));
    beats
}

pub(in crate::audio) fn plays(track: &Track, waves: &[Arc<Wave>], bpm: f64, cache: &mut Vec<(Arc<Wave>, Arc<Beats>)>) -> Vec<Play> {
    let mut plays: Vec<Play> = track
        .clips
        .iter()
        .filter_map(|clip| {
            let file = clip.file.as_ref()?;
            let wave = waves.iter().find(|wave| wave.name == file.name).filter(|wave| wave.frames() > 0)?;
            let regions = file.spans.iter().map(|span| Region { from: span.from * wave.rate, to: span.to * wave.rate, gain: span.gain() as f32 }).collect();
            let warp = file.warp.then(|| Warped { beats: beats_of(cache, wave), ratio: file.warp_ratio(bpm), mode: file.gap, tau: warp::decay_frames(file.envelope as f32, wave.rate) });
            Some(Play { start: beats(clip.start), end: beats(clip.end()), anchor: file.anchor(beats(clip.start), bpm), ratio: file.ratio(), stop: file.stop(wave.frames(), wave.rate), gain: file.gain() as f32, wave: Arc::clone(wave), regions, fades: file.fades, warp })
        })
        .collect();
    plays.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut joined: Vec<Play> = Vec::with_capacity(plays.len());
    for play in plays {
        match joined.last_mut() {
            Some(last) if last.end == play.start && last.ratio == play.ratio && last.stop == play.stop && last.gain == play.gain && (last.anchor - play.anchor).abs() < BEAT_EPSILON && Arc::ptr_eq(&last.wave, &play.wave) && last.regions == play.regions && last.fades == play.fades && last.warp == play.warp => last.end = play.end,
            _ => joined.push(play),
        }
    }
    joined
}

fn unlagged(lfos: &Lfos) -> Lagged {
    lfos.iter().map(|lfos| vec![None; lfos.len()]).collect()
}

fn song_lfos(automation: &Automation, rack: &[Row]) -> Vec<LaneLfo> {
    let mut lfos: Vec<LaneLfo> = automation.lfos.iter().filter(|lfo| lfo.amount > 0.0).cloned().collect();
    for curve in automation.curves.iter().filter(|curve| curve.lfo.is_some()) {
        if !lfos.iter().any(|lfo| lfo.aim == curve.aim) {
            let stored = automation.lfos.iter().find(|lfo| lfo.aim == curve.aim).cloned();
            lfos.push(stored.unwrap_or_else(|| LaneLfo::blank(curve.aim.clone())));
        }
    }
    lfos.retain(|lfo| lane_lives(rack, lfo));
    lfos
}

fn without(rows: &[Row], at: usize) -> impl Iterator<Item = &Row> {
    rows.iter().enumerate().filter(move |(index, _)| *index != at).map(|(_, row)| row)
}

fn alike(mine: &[Row], theirs: &[Row]) -> bool {
    mine.len() == theirs.len() && mine.iter().zip(theirs).all(|(a, b)| a.same_kind(b))
}

fn pairs(count: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..count).flat_map(move |from| (0..count).map(move |to| (from, to))).filter(|(from, to)| from != to)
}

fn moved(rows: &[Row], from: usize, to: usize) -> Vec<Row> {
    let mut rows = rows.to_vec();
    let row = rows.remove(from);
    rows.insert(to, row);
    rows
}

fn exact_move(old: &[Row], new: &[Row]) -> Option<(usize, usize)> {
    pairs(old.len()).find(|&(from, to)| moved(old, from, to) == new)
}

fn reordered(old: &[Row], new: &[Row]) -> Option<(usize, usize)> {
    let first = old.iter().zip(new).position(|(mine, theirs)| mine != theirs)?;
    let last = old.iter().zip(new).rposition(|(mine, theirs)| mine != theirs)?;
    if first == last {
        None
    } else if old[first] == new[last] && old[first + 1..=last] == new[first..last] {
        Some((first, last))
    } else if old[last] == new[first] && old[first..last] == new[first + 1..=last] {
        Some((last, first))
    } else {
        None
    }
}

fn stays(mine: &Row, theirs: &Row) -> bool {
    let bare = |row: &Row| {
        let mut row = *row;
        row.set_times(1);
        if let Row::Bus(bus) = &mut row {
            bus.master = 0;
        }
        row
    };
    bare(mine) == bare(theirs)
}

fn reshape(old: &[Row], new: &[Row]) -> Reshape {
    let ahead = old.iter().zip(new).take_while(|(mine, theirs)| stays(mine, theirs)).count();
    let behind = old[ahead..].iter().rev().zip(new[ahead..].iter().rev()).take_while(|(mine, theirs)| stays(mine, theirs)).count();
    if old.len() > new.len() && ahead + behind == new.len() {
        return Reshape::Remove(ahead);
    }
    if new.len() > old.len() && ahead + behind == old.len() {
        return Reshape::Insert(ahead);
    }
    if old.len() == new.len() + 1 {
        let exact = (0..old.len()).find(|&at| without(old, at).eq(new.iter()));
        let by_kind = || (0..old.len()).find(|&at| without(old, at).zip(new).all(|(a, b)| a.same_kind(b)));
        if let Some(at) = exact.or_else(by_kind) {
            return Reshape::Remove(at);
        }
    }
    if new.len() == old.len() + 1 {
        let exact = (0..new.len()).find(|&at| without(new, at).eq(old.iter()));
        let by_kind = || (0..new.len()).find(|&at| without(new, at).zip(old).all(|(a, b)| a.same_kind(b)));
        if let Some(at) = exact.or_else(by_kind) {
            return Reshape::Insert(at);
        }
    }
    if old.len() == new.len() {
        let by_kind = || pairs(old.len()).find(|&(from, to)| alike(&moved(old, from, to), new));
        if let Some((from, to)) = exact_move(old, new).or_else(by_kind) {
            return Reshape::Move(from, to);
        }
    }
    let disagree = old.iter().zip(new).position(|(a, b)| !a.same_kind(b)).unwrap_or(old.len().min(new.len()));
    if old.len() >= new.len() { Reshape::Remove(disagree) } else { Reshape::Insert(disagree) }
}

fn carried(was: &[Row], held: &[usize], now: &[Row]) -> Vec<usize> {
    let (mut rows, mut held) = (was.to_vec(), held.to_vec());
    held.resize(rows.len(), 0);
    while !alike(&rows, now) {
        match reshape(&rows, now) {
            Reshape::Remove(at) => {
                rows.remove(at);
                held.remove(at);
            }
            Reshape::Insert(at) => {
                rows.insert(at, now[at]);
                held.insert(at, 0);
            }
            Reshape::Move(from, to) => {
                let (row, leaving) = (rows.remove(from), held.remove(from));
                rows.insert(to, row);
                held.insert(to, leaving);
            }
        }
    }
    let sounding = rows.iter().zip(&held).map(|(was, held)| was.times() + held);
    let wanted: Vec<usize> = sounding.zip(now).map(|(sounding, row)| if row.is_midi() || row.is_core() { sounding.saturating_sub(row.times()) } else { 0 }).collect();
    lingering(now, &wanted)
}

fn row_moves(was: &RowTiming, now: &RowTiming) -> bool {
    (was.before, was.signal, was.upstream, was.latency) != (now.before, now.signal, now.upstream, now.latency) || !was.folds.iter().map(|fold| (fold.lane, fold.sum)).eq(now.folds.iter().map(|fold| (fold.lane, fold.sum)))
}

fn holds_lines(row: &RowTiming) -> bool {
    row.before > 0 || row.folds.iter().any(|fold| fold.lane > 0 || fold.sum > 0)
}

fn rack_moves(was: &RackTiming, now: &RackTiming, kept: &[Option<usize>], rack: &[Row]) -> bool {
    let arrived = now.rows.iter().zip(kept).enumerate().any(|(at, (row, from))| match from.and_then(|from| was.rows.get(from)) {
        Some(before) => row_moves(before, row),
        None => !rack.get(at).is_some_and(Row::is_core) && holds_lines(row),
    });
    arrived || was.rows.iter().enumerate().any(|(at, row)| !kept.contains(&Some(at)) && holds_lines(row))
}

fn track_moves(was: &TrackTiming, now: &TrackTiming, kept: &[Option<usize>], rack: &[Row]) -> bool {
    (was.latency, was.pre, was.to_master, was.to_returns, was.to_group) != (now.latency, now.pre, now.to_master, now.to_returns, now.to_group) || rack_moves(&was.rack, &now.rack, kept, rack)
}

struct Kept {
    tracks: Vec<Option<usize>>,
    rows: Vec<Vec<Option<usize>>>,
    returns: [Vec<Option<usize>>; SEND_COUNT],
    master: Vec<Option<usize>>,
}

fn moves(was: &Timing, now: &Timing, kept: &Kept, project: &Project) -> bool {
    let track = |index: usize| kept.tracks.get(index).copied().flatten().and_then(|from| was.tracks.get(from)).is_some_and(|before| track_moves(before, &now.tracks[index], &kept.rows[index], &project.tracks[index].rack));
    let ret = |send: usize| was.returns[send].latency != now.returns[send].latency || was.return_to_master[send] != now.return_to_master[send] || rack_moves(&was.returns[send], &now.returns[send], &kept.returns[send], &project.sends[send].rack);
    was.output != now.output || rack_moves(&was.master, &now.master, &kept.master, &project.master.rack) || (0..SEND_COUNT).any(ret) || (0..now.tracks.len().min(kept.rows.len())).any(track)
}

impl Link {
    pub(super) fn sync_rack(&self, host: Host, mirror: &mut Vec<Row>, rack: &[Row], twinned: bool) -> (bool, Vec<Option<usize>>) {
        let mut kept: Vec<Option<usize>> = (0..mirror.len()).map(Some).collect();
        let mut reshaped = !alike(mirror, rack);
        while !alike(mirror, rack) {
            match reshape(mirror, rack) {
                Reshape::Remove(at) => {
                    self.send(Message::RemoveRow { host, index: at });
                    mirror.remove(at);
                    kept.remove(at);
                }
                Reshape::Insert(at) => {
                    self.send(Message::InsertRow { host, index: at, insert: self.insert(rack[at], twinned), joins: at > 0 && rack[at].repeats(&rack[at - 1]) });
                    mirror.insert(at, rack[at]);
                    kept.insert(at, None);
                }
                Reshape::Move(from, to) => {
                    self.send(Message::MoveRow { host, from, to });
                    let row = mirror.remove(from);
                    mirror.insert(to, row);
                    let held = kept.remove(from);
                    kept.insert(to, held);
                }
            }
        }
        if let Some((from, to)) = reordered(mirror, rack) {
            self.send(Message::MoveRow { host, from, to });
            let row = mirror.remove(from);
            mirror.insert(to, row);
            let held = kept.remove(from);
            kept.insert(to, held);
            reshaped = true;
        }
        for (index, (mine, theirs)) in mirror.iter_mut().zip(rack).enumerate() {
            if mine != theirs {
                let woken = theirs.is_effect() && mine.bypassed() && !theirs.bypassed();
                *mine = *theirs;
                if woken {
                    self.renew(host, index, *theirs, twinned);
                } else {
                    self.send(Message::Row { host, index, row: *theirs });
                }
            }
        }
        (reshaped, kept)
    }

    fn renew(&self, host: Host, index: usize, row: Row, twinned: bool) {
        self.send(Message::RenewRow { host, index, insert: self.insert(row, twinned) });
    }
}

impl Audio {
    fn resend_row(&self, project: &Project, track: usize, row: usize) {
        if let Some(found) = project.tracks.get(track).and_then(|found| found.rack.get(row)) {
            self.link.send(Message::Row { host: Host::Track(track), index: row, row: *found });
        }
    }

    fn resend_strip(&self, index: usize) {
        if let Some(mirror) = self.tracks.get(index) {
            self.link.send(Message::Strip { track: index, gain: mirror.gain, heard: mirror.heard, pours: mirror.pours, unmuted: mirror.unmuted, group: mirror.group });
        }
    }

    fn resend_sends(&self, index: usize) {
        if let Some(mirror) = self.tracks.get(index) {
            self.link.send(Message::Sends { track: index, amounts: mirror.sends });
        }
    }

    fn resend_return_row(&self, project: &Project, index: usize, row: usize) {
        let host = return_host(index);
        if let Some(found) = project.rack(host).get(row) {
            self.link.send(Message::Row { host, index: row, row: *found });
        }
    }

    fn resend_return_gain(&self, index: usize) {
        match return_host(index) {
            Host::Send(send) => self.link.send(Message::Return { send, gain: self.sends[send].gain }),
            _ => self.link.send(Message::Master { gain: self.master.gain }),
        }
    }

    pub fn insert(&mut self, at: usize, track: &Track) {
        self.link.open();
        let played = played(&track.rack, at);
        let rack = self.link.rack(&played);
        self.link.send(Message::Insert { at, rack, strip: Strip::fresh(self.link.max_frames, cap(self.link.sample_rate)) });
        self.tracks.insert(at, Mirror { gain: 1.0, heard: true, pours: true, unmuted: true, twinned: false, group: None, sends: [0.0; SEND_COUNT], rack: played, places: places(&track.rack), written: track.rack.clone(), lingering: Vec::new(), solo: None, level: (0.0, 0.0), gauges: [0.0; RACK_ROWS], signals: [0.0; RACK_ROWS], strips: NO_STRIPS, plays: Vec::new(), was: None });
    }

    pub fn remove(&mut self, at: usize) {
        self.link.open();
        self.link.send(Message::Remove { at });
        self.tracks.remove(at);
    }

    pub fn move_track(&mut self, from: usize, to: usize) {
        self.link.open();
        self.link.send(Message::Move { from, to });
        let mirror = self.tracks.remove(from);
        self.tracks.insert(to, mirror);
    }

    pub fn clear(&mut self) {
        self.link.send(Message::Clear);
        self.tracks.clear();
        self.curves.clear();
        self.lfos.clear();
        self.return_curves.clear();
        self.return_lfos.clear();
        for mirror in &mut self.sends {
            mirror.rack.clear();
        }
        self.master.rack.clear();
        self.timing = Timing::default();
        self.cleared = true;
        if self.recipe.take().is_some() {
            self.seer.dispatch(None, self.link.sample_rate);
        }
        if self.twin_recipe.take().is_some() {
            self.twin_seer.dispatch(None, self.link.sample_rate);
        }
    }

    pub fn install(&mut self, project: &Project, solo: Option<usize>, solos: &[Option<CoreSolo>], waves: &[Arc<Wave>]) {
        self.link.open();
        self.clear();
        self.song.clear();
        self.sync(project, solo, solos, waves);
    }

    pub fn sync(&mut self, project: &Project, solo: Option<usize>, solos: &[Option<CoreSolo>], waves: &[Arc<Wave>]) {
        self.link.flush();
        self.link.open();
        let written = project;
        while self.tracks.len() > written.tracks.len() {
            self.remove(self.tracks.len() - 1);
        }
        for (at, track) in written.tracks.iter().enumerate().skip(self.tracks.len()) {
            self.insert(at, track);
        }
        for (mirror, track) in self.tracks.iter_mut().zip(&written.tracks).filter(|(mirror, track)| mirror.written != track.rack) {
            mirror.lingering = carried(&mirror.written, &mirror.lingering, &track.rack);
            mirror.written.clone_from(&track.rack);
        }
        let expanded = written.expanded(|track| self.tracks.get(track).map_or(&[], |mirror| &mirror.lingering));
        let project = &*expanded;
        let mut reshaped = false;
        let mut kept = Kept { tracks: self.tracks.iter().map(|mirror| mirror.was).collect(), rows: Vec::with_capacity(self.tracks.len()), returns: Default::default(), master: Vec::new() };
        if self.bpm != project.bpm {
            self.bpm = project.bpm;
            self.link.send(Message::Tempo { bpm: project.bpm });
        }
        let key = project.key_bits();
        if self.key != key {
            self.key = key;
            self.link.send(Message::Key { bits: key });
        }
        self.beats.retain(|(wave, _)| waves.iter().any(|kept| Arc::ptr_eq(kept, wave)));
        let tracks = &project.tracks;
        let solo_of = |index: usize| solos.get(index).copied().flatten();
        let audible = heard(project);
        let named = signalled(project);
        let wanted = |index: usize| !tracks[index].audio && (named & (1 << index) != 0 || project.group_of(index).is_some_and(|group| named & (1 << group) != 0));
        let mut twins_changed = false;
        for (index, (track, mirror)) in tracks.iter().zip(self.tracks.iter_mut()).enumerate() {
            mirror.places = places_with(&written.tracks[index].rack, &mirror.lingering);
            let solo = solo_of(index).map(|solo| Alone::of(solo, &mirror.places));
            let group = project.group_of(index);
            let heard = audible[index];
            let pours = pours(project, &audible, index);
            let unmuted = !track.mute;
            if mirror.gain != track.gain || mirror.heard != heard || mirror.pours != pours || mirror.unmuted != unmuted || mirror.group != group {
                mirror.gain = track.gain;
                mirror.heard = heard;
                mirror.pours = pours;
                mirror.unmuted = unmuted;
                mirror.group = group;
                self.link.send(Message::Strip { track: index, gain: track.gain, heard, pours, unmuted, group });
            }
            if mirror.sends != track.sends {
                mirror.sends = track.sends;
                self.link.send(Message::Sends { track: index, amounts: track.sends });
            }
            let (changed, rows) = self.link.sync_rack(Host::Track(index), &mut mirror.rack, &track.rack, mirror.twinned);
            reshaped |= changed;
            kept.rows.push(rows);
            if mirror.twinned != wanted(index) {
                twins_changed = true;
                mirror.twinned = wanted(index);
                let lines = mirror.twinned.then(|| Box::new(TwinLines::new(self.link.max_frames, cap(self.link.sample_rate))));
                self.link.send(Message::Twins { track: index, lines, units: self.link.twins(&mirror.rack, mirror.twinned) });
            }
            if mirror.solo != solo {
                let woken = mirror.solo.is_some_and(|was| !was.effects) && solo.is_none_or(|now| now.effects);
                mirror.solo = solo;
                self.link.send(Message::CoreSolo { track: index, solo });
                for (row, entry) in mirror.rack.iter().enumerate().filter(|(_, entry)| woken && entry.is_effect() && !entry.bypassed()) {
                    self.link.renew(Host::Track(index), row, *entry, false);
                }
            }
            let plays = plays(track, waves, project.bpm, &mut self.beats);
            if mirror.plays != plays {
                mirror.plays = plays.clone();
                self.link.send(Message::Plays { track: index, plays });
            }
        }
        if self.master.gain != project.master.gain {
            self.master.gain = project.master.gain;
            self.link.send(Message::Master { gain: project.master.gain });
        }
        self.master.places = places(&written.master.rack);
        kept.master = self.link.sync_rack(Host::Master, &mut self.master.rack, &project.master.rack, false).1;
        for (index, (send, mirror)) in project.sends.iter().zip(self.sends.iter_mut()).enumerate() {
            if mirror.gain != send.gain {
                mirror.gain = send.gain;
                self.link.send(Message::Return { send: index, gain: send.gain });
            }
            mirror.places = places(&written.sends[index].rack);
            kept.returns[index] = self.link.sync_rack(Host::Send(index), &mut mirror.rack, &send.rack, false).1;
        }
        if self.solo != solo {
            self.solo = solo;
            self.link.send(Message::SendSolo { send: solo });
        }
        if self.metronome != project.metronome {
            self.metronome = project.metronome;
            self.link.send(Message::Metronome { on: project.metronome });
        }
        let song: Song = tracks.iter().map(Track::song_notes).collect();
        if song != self.song {
            self.song = song.clone();
            self.link.send(Message::Song(Box::new(song)));
        }
        let leaders = led(project);
        if leaders != self.leaders {
            self.leaders = leaders;
            self.link.send(Message::Leaders { leaders: leaders.0, order: leaders.1 });
        }
        let curves: Curves = tracks.iter().map(|track| track.automation.curves.clone()).collect();
        if curves != self.curves {
            let restored: Vec<usize> = volume_gone(&curves, &self.curves, |curve| &curve.aim).collect();
            let resent = sends_gone(&curves, &self.curves, |curve| &curve.aim);
            let freed = lanes_gone(&curves, &self.curves, |curve| &curve.aim);
            self.curves = curves.clone();
            self.link.send(Message::Curves(Box::new(curves)));
            for index in restored {
                self.resend_strip(index);
            }
            for index in resent {
                self.resend_sends(index);
            }
            for (track, row) in freed {
                self.resend_row(project, track, row);
            }
        }
        let lfos: Lfos = tracks.iter().map(|track| song_lfos(&track.automation, &track.rack)).collect();
        if lfos != self.lfos {
            let restored: Vec<usize> = volume_gone(&lfos, &self.lfos, |lfo| &lfo.aim).collect();
            let resent = sends_gone(&lfos, &self.lfos, |lfo| &lfo.aim);
            let freed = lanes_gone(&lfos, &self.lfos, |lfo| &lfo.aim);
            self.lfos = lfos.clone();
            let lagged = unlagged(&lfos);
            self.link.send(Message::Lfos(Box::new(lfos), lagged));
            for index in restored {
                self.resend_strip(index);
            }
            for index in resent {
                self.resend_sends(index);
            }
            for (track, row) in freed {
                self.resend_row(project, track, row);
            }
        }
        let return_curves: Curves = project.returns().map(|ret| ret.automation.curves.clone()).collect();
        if return_curves != self.return_curves {
            let restored: Vec<usize> = volume_gone(&return_curves, &self.return_curves, |curve| &curve.aim).collect();
            let freed = lanes_gone(&return_curves, &self.return_curves, |curve| &curve.aim);
            self.return_curves = return_curves.clone();
            self.link.send(Message::ReturnCurves(Box::new(return_curves)));
            for index in restored {
                self.resend_return_gain(index);
            }
            for (index, row) in freed {
                self.resend_return_row(project, index, row);
            }
        }
        let return_lfos: Lfos = project.returns().map(|ret| song_lfos(&ret.automation, &ret.rack)).collect();
        if return_lfos != self.return_lfos {
            let restored: Vec<usize> = volume_gone(&return_lfos, &self.return_lfos, |lfo| &lfo.aim).collect();
            let freed = lanes_gone(&return_lfos, &self.return_lfos, |lfo| &lfo.aim);
            self.return_lfos = return_lfos.clone();
            let lagged = unlagged(&return_lfos);
            self.link.send(Message::ReturnLfos(Box::new(return_lfos), lagged));
            for index in restored {
                self.resend_return_gain(index);
            }
            for (index, row) in freed {
                self.resend_return_row(project, index, row);
            }
        }
        let timing = Timing::of(project, self.link.sample_rate);
        if reshaped || timing != self.timing {
            let moved = self.cleared || moves(&self.timing, &timing, &kept, project);
            self.link.send(Message::Timing(Box::new(Plan::of(&timing, project, self.link.max_frames, moved))));
            self.timing = timing;
        }
        self.cleared = false;
        for (index, mirror) in self.tracks.iter_mut().enumerate() {
            mirror.was = Some(index);
        }
        self.link.commit();
        if !self.seer.sees() {
            return;
        }
        let soloed = project.tracks.iter().any(|track| track.solo);
        let recipe = vision::recipe(project, solo, waves, |_| true);
        if recipe != self.recipe || (twins_changed && !soloed) {
            for (host, index) in vision::unforeseen(project, recipe.as_ref()) {
                self.link.send(Message::Forget { host, index });
            }
            self.recipe = recipe;
            self.shared = !soloed;
            let rendered = self.seer.dispatch(self.recipe.clone(), self.link.sample_rate);
            self.deliver(rendered);
        }
        let twinned = |host: Host| host.track().and_then(|track| self.tracks.get(track)).is_some_and(|mirror| mirror.twinned);
        let twin_recipe = (soloed && self.tracks.iter().any(|mirror| mirror.twinned)).then(|| vision::recipe(&unsoloed(project), solo, waves, twinned)).flatten();
        if twin_recipe != self.twin_recipe || (twins_changed && soloed) {
            self.twin_recipe = twin_recipe;
            let rendered = self.twin_seer.dispatch(self.twin_recipe.clone(), self.link.sample_rate);
            self.deliver_twins(rendered);
        }
    }
}

fn unsoloed(project: &Project) -> Project {
    let mut calm = project.clone();
    for track in &mut calm.tracks {
        track.solo = false;
    }
    calm
}

pub(in crate::audio) fn unled() -> ([u8; TRACKS], [u8; TRACKS]) {
    ([u8::MAX; TRACKS], std::array::from_fn(|track| track as u8))
}

fn led(project: &Project) -> ([u8; TRACKS], [u8; TRACKS]) {
    let count = project.tracks.len().min(TRACKS);
    let mut leaders: Vec<Option<usize>> = project.tracks.iter().take(count).enumerate().map(|(track, owner)| owner.follows.filter(|&leader| leader < count && leader != track)).collect();
    while let Some(cycle) = (0..count).find_map(|track| looped(&leaders, track)) {
        leaders[cycle] = None;
    }
    let depth = |track: usize| std::iter::successors(Some(track), |&at| leaders[at]).count();
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by_key(|&track| depth(track));
    let (mut led, mut ordered) = unled();
    for (track, leader) in leaders.iter().enumerate() {
        led[track] = leader.map_or(u8::MAX, |leader| leader as u8);
    }
    for (slot, track) in order.into_iter().enumerate() {
        ordered[slot] = track as u8;
    }
    (led, ordered)
}

fn looped(leaders: &[Option<usize>], track: usize) -> Option<usize> {
    let mut path = vec![track];
    let mut at = track;
    while let Some(next) = leaders[at] {
        if let Some(from) = path.iter().position(|&seen| seen == next) {
            return path[from..].iter().copied().min();
        }
        path.push(next);
        at = next;
    }
    None
}

fn lanes_gone<T>(now: &[Vec<T>], before: &[Vec<T>], key: impl Fn(&T) -> &Aim + Copy) -> Vec<(usize, usize)> {
    let mut freed: Vec<(usize, usize)> = now
        .iter()
        .zip(before)
        .enumerate()
        .flat_map(|(track, (now, before))| {
            before.iter().filter(move |lane| !now.iter().any(|kept| key(kept) == key(lane))).map(move |lane| (track, key(lane).row_or_top()))
        })
        .collect();
    freed.sort_unstable();
    freed.dedup();
    freed
}

fn sends_gone<T>(now: &[Vec<T>], before: &[Vec<T>], key: impl Fn(&T) -> &Aim + Copy) -> Vec<usize> {
    let gone = |now: &[T], lane: &T| matches!(key(lane), Aim::Send(_)) && !now.iter().any(|kept| key(kept) == key(lane));
    now.iter().zip(before).enumerate().filter(|(_, (now, before))| before.iter().any(|lane| gone(now, lane))).map(|(index, _)| index).collect()
}

fn volume_gone<'a, T>(now: &'a [Vec<T>], before: &'a [Vec<T>], aim: impl Fn(&T) -> &Aim + 'a) -> impl Iterator<Item = usize> + 'a {
    let has_volume = move |lanes: &[T]| lanes.iter().any(|lane| *aim(lane) == Aim::Gain);
    now.iter().zip(before).enumerate().filter(move |(_, (now, before))| has_volume(before) && !has_volume(now)).map(|(index, _)| index)
}
