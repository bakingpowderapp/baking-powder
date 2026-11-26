use super::{Project, RACK_ROWS, Row, SEND_COUNT, TRACKS, Track};

pub const CAP_SECONDS: f64 = 0.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fold {
    pub core: usize,
    pub lane: usize,
    pub sum: usize,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RowTiming {
    pub latency: usize,
    pub before: usize,
    pub signal: usize,
    pub upstream: usize,
    pub folds: Vec<Fold>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RackTiming {
    pub latency: usize,
    pub rows: Vec<RowTiming>,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TrackTiming {
    pub source: usize,
    pub pre: usize,
    pub rack: RackTiming,
    pub latency: usize,
    pub delay: isize,
    pub to_master: usize,
    pub to_returns: usize,
    pub to_group: usize,
    pub uncompensated: usize,
}

impl TrackTiming {
    pub fn staggered(&self) -> bool {
        self.pre > 0 || self.rack.rows.iter().any(|row| row.before > 0 || row.signal > 0 || row.upstream > 0 || row.folds.iter().any(|fold| fold.lane > 0 || fold.sum > 0))
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Timing {
    pub rate: f64,
    pub cap: usize,
    pub tracks: Vec<TrackTiming>,
    pub returns: [RackTiming; SEND_COUNT],
    pub returns_in: usize,
    pub return_to_master: [usize; SEND_COUNT],
    pub master_in: usize,
    pub master: RackTiming,
    pub output: usize,
    pub longest: Option<usize>,
}

struct Lines {
    cap: usize,
    excess: usize,
}

impl Lines {
    fn fit(&mut self, wanted: usize) -> usize {
        let applied = wanted.min(self.cap);
        self.excess += wanted - applied;
        applied
    }

    fn reach(&mut self, join: usize, late: isize) -> usize {
        self.fit((join as isize - late).max(0).unsigned_abs())
    }

    fn meet(&mut self, signal: isize, at: usize) -> (usize, usize) {
        let gap = signal - at as isize;
        (self.fit(gap.max(0).unsigned_abs()), self.fit(gap.min(0).unsigned_abs()))
    }
}

impl TrackTiming {
    fn late(&self, tap: usize) -> isize {
        tap as isize - self.delay
    }
}

fn latest(lates: impl Iterator<Item = isize>) -> usize {
    lates.fold(0, isize::max).unsigned_abs()
}

fn delay_of(track: &Track, rate: f64) -> isize {
    (f64::from(track.delay_ms) * rate / 1000.0).round() as isize
}

fn group_delay(project: &Project, track: usize, rate: f64) -> isize {
    project.group_of(track).map_or(0, |group| delay_of(&project.tracks[group], rate))
}

fn shares_group(project: &Project, source: usize, track: usize) -> bool {
    project.group_of(source).is_some_and(|group| group == track || project.group_of(track) == Some(group))
}

fn signal_of(row: &Row, track: usize, done: &[Option<TrackTiming>], project: &Project, rate: f64) -> Option<isize> {
    let Row::Effect(slot) = row else {
        return None;
    };
    if slot.bypass {
        return None;
    }
    let (source, pre) = slot.source()?;
    if source == track {
        return None;
    }
    let timing = done.get(source)?.as_ref()?;
    let late = timing.late(if pre { timing.pre } else { timing.latency });
    Some(if shares_group(project, source, track) { late } else { late - group_delay(project, source, rate) })
}

fn walk(rows: &[Row], source: usize, track: usize, done: &[Option<TrackTiming>], project: &Project, rate: f64, lines: &mut Lines) -> RackTiming {
    let rows = &rows[..rows.len().min(RACK_ROWS)];
    let mut sum = source;
    let mut split = None;
    let mut timings: Vec<RowTiming> = Vec::with_capacity(rows.len());
    for (position, row) in rows.iter().enumerate() {
        let latency = row.latency(rate);
        let (mut before, mut signal, mut upstream) = (0, 0, 0);
        let mut folds = Vec::new();
        match row {
            Row::Core(_) => {
                let met = sum.max(latency);
                folds.push(Fold { core: position, lane: lines.fit(met - latency), sum: lines.fit(met - sum) });
                sum = met;
            }
            Row::Bus(_) if split.is_none() => {
                split = Some((position, sum));
                sum = source;
            }
            Row::Midi(_) | Row::Effect(_) | Row::Bus(_) => {
                if let Some(late) = signal_of(row, track, done, project, rate) {
                    (before, signal) = lines.meet(late, sum);
                }
                upstream = sum + before;
                sum = upstream + latency;
            }
        }
        timings.push(RowTiming { latency, before, signal, upstream, folds });
    }
    if let Some((position, first)) = split {
        let met = first.max(sum);
        timings[position].folds.push(Fold { core: position, lane: lines.fit(met - first), sum: lines.fit(met - sum) });
        sum = met;
    }
    RackTiming { latency: sum, rows: timings }
}

pub fn source_of(slot: &fx::Slot, track: usize, count: usize) -> u64 {
    slot.source().filter(|&(source, _)| !slot.bypass && source != track && source < count).map_or(0, |(source, _)| 1 << source)
}

fn sources_of(track: &Track, index: usize, count: usize) -> u64 {
    track.rack.iter().fold(0, |sources, row| match row {
        Row::Effect(slot) => sources | source_of(slot, index, count),
        Row::Midi(_) | Row::Core(_) | Row::Bus(_) => sources,
    })
}

fn looped(sources: &[u64; TRACKS], members: &[u64; TRACKS], placed: u64, count: usize) -> Option<usize> {
    let mut reach = [0u64; TRACKS];
    for (track, fed) in sources.iter().enumerate().take(count) {
        reach[track] = fed & !placed;
    }
    loop {
        let mut grew = false;
        for track in 0..count {
            let seen = (0..count).filter(|&source| reach[track] & (1 << source) != 0).fold(reach[track], |seen, source| seen | reach[source]);
            grew |= seen != reach[track];
            reach[track] = seen;
        }
        if !grew {
            let circling = |track: &usize| reach[*track] & (1 << *track) != 0;
            return (0..count).filter(circling).find(|&track| members[track] & !placed == 0).or_else(|| (0..count).find(circling));
        }
    }
}

pub fn placing(sources: &[u64; TRACKS], members: &[u64; TRACKS], count: usize) -> [usize; TRACKS] {
    let mut order = [0; TRACKS];
    let mut placed = 0u64;
    for (position, slot) in order.iter_mut().enumerate().take(count) {
        let ready = (0..count).find(|&track| placed & (1 << track) == 0 && sources[track] & !placed == 0);
        let next = ready.or_else(|| looped(sources, members, placed, count)).unwrap_or(position);
        placed |= 1 << next;
        *slot = next;
    }
    order
}

pub fn order(project: &Project) -> Vec<usize> {
    let count = project.tracks.len().min(TRACKS);
    let (mut sources, mut members) = ([0u64; TRACKS], [0u64; TRACKS]);
    for (index, track) in project.tracks.iter().enumerate().take(count) {
        sources[index] = sources_of(track, index, count);
    }
    for index in 0..count {
        if let Some(group) = project.group_of(index).filter(|&group| group < count && group != index) {
            members[group] |= 1 << index;
            sources[group] |= 1 << index;
        }
    }
    placing(&sources, &members, count)[..count].to_vec()
}

pub fn cap(rate: f64) -> usize {
    (CAP_SECONDS * rate).round() as usize
}

impl Timing {
    pub fn of(project: &Project, rate: f64) -> Timing {
        let cap = cap(rate);
        let count = project.tracks.len().min(TRACKS);
        let mut done: Vec<Option<TrackTiming>> = vec![None; count];
        for index in order(project) {
            let track = &project.tracks[index];
            let mut lines = Lines { cap, excess: 0 };
            let members = project.members_of(index);
            let source = latest(members.iter().filter_map(|&member| done.get(member).and_then(|timing| timing.as_ref())).map(|timing| timing.late(timing.latency)));
            let cores = track.rack.iter().filter(|row| row.is_core()).map(|row| row.latency(rate)).max().unwrap_or(0);
            let rack = walk(&track.rack, source, index, &done, project, rate, &mut lines);
            let delay = delay_of(track, rate);
            done[index] = Some(TrackTiming { source, pre: source.max(cores), latency: rack.latency, rack, delay, to_master: 0, to_returns: 0, to_group: 0, uncompensated: lines.excess });
        }
        let late_of = |index: usize| done[index].as_ref().map_or(0, |timing| timing.late(timing.latency));
        let sent_of = |index: usize| late_of(index) - group_delay(project, index, rate);
        let returns_in = latest((0..count).map(sent_of));
        let longest = (0..count).filter(|&index| sent_of(index) == returns_in as isize && returns_in > 0).min_by_key(|&index| index);
        let ungrouped = latest((0..count).filter(|&index| project.group_of(index).is_none()).map(late_of));
        let mut lines = Lines { cap, excess: 0 };
        let returns: [RackTiming; SEND_COUNT] = std::array::from_fn(|send| walk(&project.sends[send].rack, returns_in, TRACKS, &done, project, rate, &mut lines));
        let master_in = returns.iter().map(|ret| ret.latency).fold(ungrouped, usize::max);
        let return_to_master = std::array::from_fn(|send| lines.fit(master_in - returns[send].latency));
        let master = walk(&project.master.rack, master_in, TRACKS, &done, project, rate, &mut lines);
        let mut tracks: Vec<TrackTiming> = done.into_iter().map(Option::unwrap_or_default).collect();
        for index in 0..count {
            let (mut heard, mut sent) = (Lines { cap, excess: 0 }, Lines { cap, excess: 0 });
            let late = tracks[index].late(tracks[index].latency);
            let group = project.group_of(index);
            tracks[index].to_master = if group.is_some() { 0 } else { heard.reach(master_in, late) };
            tracks[index].to_group = group.map_or(0, |group| heard.reach(tracks[group].source, late));
            heard.excess += group.map_or(0, |group| (late - tracks[group].source as isize).max(0).unsigned_abs());
            tracks[index].to_returns = sent.reach(returns_in, late - group_delay(project, index, rate));
            let sends = project.tracks[index].sends.iter().any(|&level| level > 0.0);
            tracks[index].uncompensated += heard.excess.max(if sends { sent.excess } else { 0 });
        }
        Timing { rate, cap, tracks, returns, returns_in, return_to_master, master_in, output: master.latency, master, longest }
    }

    pub fn ms(&self, samples: usize) -> f64 {
        if self.rate > 0.0 { samples as f64 * 1000.0 / self.rate } else { 0.0 }
    }
}
