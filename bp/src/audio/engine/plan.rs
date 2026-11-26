use super::*;

pub(crate) struct Line {
    l: Vec<f32>,
    r: Vec<f32>,
    at: usize,
    filled: usize,
}

fn add_into(out: &mut [f32], from: &[f32], gain: f32) {
    for (out, from) in out.iter_mut().zip(from) {
        *out += from * gain;
    }
}

impl Line {
    pub(crate) const fn empty() -> Line {
        Line { l: Vec::new(), r: Vec::new(), at: 0, filled: 0 }
    }

    pub(crate) fn new(len: usize) -> Line {
        Line { l: vec![0.0; len], r: vec![0.0; len], at: 0, filled: 0 }
    }

    pub(crate) fn len(&self) -> usize {
        self.l.len()
    }

    pub(crate) fn clear(&mut self) {
        self.hush();
        self.at = 0;
        self.filled = 0;
    }

    fn hush(&mut self) {
        self.l.fill(0.0);
        self.r.fill(0.0);
    }

    pub(crate) fn push(&mut self, l: &[f32], r: &[f32]) {
        let len = self.l.len();
        let n = l.len().min(r.len()).min(len);
        if n == 0 {
            return;
        }
        let at = self.at;
        let first = n.min(len - at);
        self.l[at..at + first].copy_from_slice(&l[..first]);
        self.r[at..at + first].copy_from_slice(&r[..first]);
        let rest = n - first;
        self.l[..rest].copy_from_slice(&l[first..n]);
        self.r[..rest].copy_from_slice(&r[first..n]);
        self.at = if at + n >= len { at + n - len } else { at + n };
        self.filled = (self.filled + n).min(len);
    }

    fn carry(&mut self, old: &Line) {
        let (len, from) = (self.l.len(), old.l.len());
        let kept = len.min(from);
        if kept == 0 {
            return;
        }
        let start = (old.at + from - kept) % from;
        let first = kept.min(from - start);
        let (head, tail) = (len - kept, len - kept + first);
        self.l[head..tail].copy_from_slice(&old.l[start..start + first]);
        self.r[head..tail].copy_from_slice(&old.r[start..start + first]);
        self.l[tail..].copy_from_slice(&old.l[..kept - first]);
        self.r[tail..].copy_from_slice(&old.r[..kept - first]);
        self.at = 0;
        self.filled = old.filled.min(kept);
    }

    fn start(&self, n: usize, offset: usize) -> Option<usize> {
        let back = n + offset;
        let len = self.l.len();
        if n == 0 || back > len {
            return None;
        }
        Some(if self.at >= back { self.at - back } else { self.at + len - back })
    }

    pub(crate) fn add(&self, offset: usize, gain: f32, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let Some(start) = self.start(n, offset) else {
            return;
        };
        let first = n.min(self.l.len() - start);
        let (head_l, tail_l) = l[..n].split_at_mut(first);
        let (head_r, tail_r) = r[..n].split_at_mut(first);
        add_into(head_l, &self.l[start..start + first], gain);
        add_into(head_r, &self.r[start..start + first], gain);
        add_into(tail_l, &self.l[..n - first], gain);
        add_into(tail_r, &self.r[..n - first], gain);
    }

    pub(crate) fn read(&self, offset: usize, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let Some(start) = self.start(n, offset) else {
            l.fill(0.0);
            r.fill(0.0);
            return;
        };
        let first = n.min(self.l.len() - start);
        l[..first].copy_from_slice(&self.l[start..start + first]);
        r[..first].copy_from_slice(&self.r[start..start + first]);
        l[first..n].copy_from_slice(&self.l[..n - first]);
        r[first..n].copy_from_slice(&self.r[..n - first]);
    }

    pub(crate) fn delay(&mut self, offset: usize, l: &mut [f32], r: &mut [f32]) {
        self.push(l, r);
        self.read(offset, l, r);
    }
}

pub(crate) struct Delay {
    pub(crate) offset: usize,
    line: Line,
    warm: Warm,
}

impl Delay {
    const NONE: Delay = Delay { offset: 0, line: Line::empty(), warm: Warm::SETTLED };

    fn of(offset: usize, max_frames: usize) -> Delay {
        if offset == 0 { Delay::NONE } else { Delay { offset, line: Line::new(offset + max_frames), warm: Warm::SETTLED } }
    }

    pub(crate) fn apply(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.offset > 0 {
            self.line.delay(self.offset, l, r);
            self.warm.apply(l, r);
        }
    }

    fn adopt(&mut self, old: &mut Delay, playing: bool) {
        if self.offset == 0 {
            return;
        }
        if self.line.len() == old.line.len() {
            std::mem::swap(&mut self.line, &mut old.line);
            self.warm = old.warm;
        } else {
            self.line.carry(&old.line);
            self.warm = if playing { Warm::of(self.offset.saturating_sub(self.line.filled)) } else { Warm::SETTLED };
        }
    }

    fn clear(&mut self) {
        self.line.clear();
        self.warm = Warm::SETTLED;
    }
}

pub(crate) struct FoldPlan {
    pub(crate) core: usize,
    pub(crate) lane: Delay,
    pub(crate) sum: Delay,
    pub(crate) twin_lane: Delay,
    pub(crate) twin_sum: Delay,
}

pub(crate) struct RowPlan {
    pub(crate) before: Delay,
    pub(crate) twin_before: Delay,
    pub(crate) signal: usize,
    pub(crate) upstream: usize,
    pub(crate) latency: usize,
    pub(crate) folds: Vec<FoldPlan>,
    pub(crate) pre: Delay,
    pub(crate) twin_pre: Delay,
}

const NEW_ROW: usize = usize::MAX;

pub(crate) struct TrackPlan {
    pub(crate) latency: usize,
    pub(crate) shift: (isize, isize),
    pub(crate) to_master: usize,
    pub(crate) to_returns: usize,
    pub(crate) to_group: usize,
    pub(crate) rows: Vec<RowPlan>,
    layout: [usize; RACK_ROWS],
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RowLag {
    pub(crate) signal: usize,
    pub(crate) upstream: usize,
    pub(crate) latency: usize,
}

impl RowLag {
    pub(crate) fn of(&self, depth: usize) -> usize {
        self.upstream + depth.min(self.latency)
    }
}

pub(crate) type Lags = [RowLag; RACK_ROWS];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReturnPlan {
    pub(crate) latency: usize,
    pub(crate) to_master: usize,
    pub(crate) rows: Lags,
    pub(crate) shift: isize,
}

impl Default for ReturnPlan {
    fn default() -> ReturnPlan {
        ReturnPlan { latency: 0, to_master: 0, rows: [RowLag::default(); RACK_ROWS], shift: 0 }
    }
}

pub(crate) struct Plan {
    pub(crate) tracks: Vec<Option<Box<TrackPlan>>>,
    pub(crate) returns: [ReturnPlan; SEND_COUNT],
    pub(crate) master: Lags,
    pub(crate) output: usize,
    pub(crate) moves: bool,
}

impl TrackPlan {
    pub(crate) fn lag(&self, aim: &Aim, depth: usize) -> usize {
        match aim {
            Aim::Gain => self.latency,
            Aim::Send(_) => self.latency + self.to_returns,
            Aim::Level(_) | Aim::Wet(_) | Aim::Knob { .. } => self.rows.get(aim.row_or_top()).map_or(0, |row| row.upstream + depth.min(row.latency)),
        }
    }

    fn lines(&mut self) -> impl Iterator<Item = &mut Delay> {
        self.rows.iter_mut().flat_map(|row| {
            let RowPlan { before, twin_before, pre, twin_pre, folds, .. } = row;
            [before, twin_before, pre, twin_pre].into_iter().chain(folds.iter_mut().flat_map(|fold| [&mut fold.lane, &mut fold.sum, &mut fold.twin_lane, &mut fold.twin_sum]))
        })
    }

    pub(crate) fn adopt(&mut self, old: Option<&mut TrackPlan>, playing: bool) {
        if playing {
            for line in self.lines() {
                line.warm = Warm::of(line.offset);
            }
        }
        let Some(TrackPlan { rows, layout, .. }) = old else {
            return;
        };
        for (row, &was) in self.rows.iter_mut().zip(layout.iter()) {
            let Some(before) = rows.get_mut(was) else {
                continue;
            };
            row.before.adopt(&mut before.before, playing);
            row.twin_before.adopt(&mut before.twin_before, playing);
            row.pre.adopt(&mut before.pre, playing);
            row.twin_pre.adopt(&mut before.twin_pre, playing);
            for fold in &mut row.folds {
                let core = layout.get(fold.core).copied();
                if let Some(was) = before.folds.iter_mut().find(|was| Some(was.core) == core) {
                    fold.lane.adopt(&mut was.lane, playing);
                    fold.sum.adopt(&mut was.sum, playing);
                    fold.twin_lane.adopt(&mut was.twin_lane, playing);
                    fold.twin_sum.adopt(&mut was.twin_sum, playing);
                }
            }
        }
    }

    pub(crate) fn inserted(&mut self, at: usize) {
        if at < RACK_ROWS {
            self.layout.copy_within(at..RACK_ROWS - 1, at + 1);
            self.layout[at] = NEW_ROW;
        }
    }

    pub(crate) fn removed(&mut self, at: usize) {
        if at < RACK_ROWS {
            self.layout.copy_within(at + 1.., at);
            self.layout[RACK_ROWS - 1] = NEW_ROW;
        }
    }

    pub(crate) fn moved(&mut self, from: usize, to: usize) {
        if from.max(to) < RACK_ROWS {
            if from < to {
                self.layout[from..=to].rotate_left(1);
            } else {
                self.layout[to..=from].rotate_right(1);
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        for line in self.lines() {
            line.clear();
        }
    }

    pub(crate) fn clear_twin(&mut self) {
        for row in &mut self.rows {
            row.twin_before.clear();
            row.twin_pre.clear();
            for fold in &mut row.folds {
                fold.twin_lane.clear();
                fold.twin_sum.clear();
            }
        }
    }
}

fn arrival(read: usize, latency: usize, join: usize) -> isize {
    (read + latency) as isize - join as isize
}

fn returned(timing: &Timing, send: usize) -> isize {
    timing.return_to_master.get(send).zip(timing.returns.get(send)).map_or(0, |(&read, ret)| arrival(read, ret.latency, timing.master_in))
}

pub(crate) fn shift(timing: &Timing, project: &Project, track: usize) -> (isize, isize) {
    let Some(own) = timing.tracks.get(track) else {
        return (0, 0);
    };
    let sent = |index: usize| {
        let sender = &timing.tracks[index];
        let levels = project.tracks.get(index).map_or(&[][..], |owner| &owner.sends[..]);
        levels.iter().enumerate().filter(|(_, level)| **level > 0.0).map(move |(send, _)| arrival(sender.to_returns, sender.latency, timing.returns_in) + returned(timing, send))
    };
    let heard = |index: usize| std::iter::once(arrival(timing.tracks[index].to_master, timing.tracks[index].latency, timing.master_in)).chain(sent(index));
    let routes: Vec<isize> = match project.group_of(track).filter(|&group| group < timing.tracks.len()) {
        Some(group) => {
            let into = arrival(own.to_group, own.latency, timing.tracks[group].source);
            heard(group).map(|route| into + route).chain(sent(track)).collect()
        }
        None => heard(track).collect(),
    };
    routes.iter().fold((isize::MAX, isize::MIN), |(soonest, latest), &route| (soonest.min(route), latest.max(route)))
}

fn lags(rack: &RackTiming) -> Lags {
    std::array::from_fn(|row| rack.rows.get(row).map_or(RowLag::default(), |row| RowLag { signal: row.signal, upstream: row.upstream, latency: row.latency }))
}

impl Plan {
    pub(crate) fn of(timing: &Timing, project: &Project, max_frames: usize, moves: bool) -> Plan {
        let tracks = timing
            .tracks
            .iter()
            .zip(&project.tracks)
            .enumerate()
            .map(|(index, (track, owner))| {
                let twinned = |offset: usize| Delay::of(if owner.audio { 0 } else { offset }, max_frames);
                let rows = track
                    .rack
                    .rows
                    .iter()
                    .zip(&owner.rack)
                    .map(|(row, kind)| {
                        let pre = if kind.is_core() { track.pre.saturating_sub(row.latency) } else { 0 };
                        let folds = row.folds.iter().map(|fold| {
                            let bus = owner.rack.get(fold.core).is_some_and(Row::is_bus);
                            FoldPlan { core: fold.core, lane: Delay::of(fold.lane, max_frames), sum: Delay::of(fold.sum, max_frames), twin_lane: twinned(if bus { fold.lane } else { 0 }), twin_sum: twinned(fold.sum) }
                        });
                        RowPlan { before: Delay::of(row.before, max_frames), twin_before: twinned(row.before), signal: row.signal, upstream: row.upstream, latency: row.latency, folds: folds.collect(), pre: Delay::of(pre, max_frames), twin_pre: twinned(pre) }
                    })
                    .collect();
                let layout = std::array::from_fn(|row| row);
                Some(Box::new(TrackPlan { latency: track.latency, shift: shift(timing, project, index), to_master: track.to_master, to_returns: track.to_returns, to_group: track.to_group, rows, layout }))
            })
            .collect();
        let returns = std::array::from_fn(|send| ReturnPlan { latency: timing.returns[send].latency, to_master: timing.return_to_master[send], rows: lags(&timing.returns[send]), shift: returned(timing, send) });
        Plan { tracks, returns, master: lags(&timing.master), output: timing.output, moves }
    }
}
