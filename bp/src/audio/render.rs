use super::*;

mod commands;
mod lanes;
mod rehearsal;
mod transport;

const PITCHES: usize = 128;
const ACTIVE_NOTES: usize = TRACKS * PITCHES;
const LANE_GRID: usize = 64;
const BUSY_WAKE: f64 = 0.2;
const PATIENCE: Duration = Duration::from_millis(500);
const PATIENT: Duration = Duration::from_secs(600);
const TIMED_SPANS: u64 = 16;

#[derive(Clone, Copy)]
pub(super) struct Pour {
    pub(super) beat: f64,
    pub(super) origin: f64,
}

struct Preview {
    cue: Box<Cue>,
    at: f64,
    played: usize,
    wait: usize,
    cursor: usize,
}

impl Preview {
    fn new(cue: Box<Cue>, wait: usize) -> Preview {
        Preview { cue, at: 0.0, played: 0, wait, cursor: 0 }
    }

    fn pour(&mut self, l: &mut [f32], r: &mut [f32], sample_rate: f64, fading: bool, level: f32) {
        let n = l.len().min(r.len());
        let skip = self.wait.min(n);
        self.wait -= skip;
        let (l, r) = (&mut l[skip..n], &mut r[skip..n]);
        let step = self.cue.wave.rate / sample_rate;
        match &self.cue.warp {
            Some(warp) => Preview::pour_warped(warp, &self.cue.wave, &mut self.at, &mut self.cursor, l, r, step, fading, level),
            None => Preview::pour_raw(&self.cue.wave, self.cue.once, &mut self.at, &mut self.played, l, r, step, fading, level),
        }
    }

    fn retime(&mut self, ratio: f64, output: usize) {
        let Some(warp) = &mut self.cue.warp else {
            return;
        };
        warp.ratio *= ratio;
        self.at *= ratio;
        self.cursor = warp.cursor_at(self.at);
        let held = self.wait.saturating_sub(output);
        self.wait = self.wait - held + (held as f64 * ratio).round() as usize;
    }

    fn progress(&self) -> f64 {
        let frame = match &self.cue.warp {
            Some(warp) => warp.source_at(self.at, self.cursor),
            None => self.at,
        };
        (frame / self.cue.wave.frames().max(1) as f64).clamp(0.0, 1.0)
    }

    fn pour_raw(wave: &Wave, once: bool, at: &mut f64, played: &mut usize, l: &mut [f32], r: &mut [f32], step: f64, fading: bool, level: f32) {
        let n = l.len();
        let frames = wave.frames() as f64;
        for i in 0..n {
            if *at >= frames {
                if once {
                    return;
                }
                *at -= frames;
                *played = 0;
            }
            let (left, right) = wave.at(*at, step);
            let remaining = ((frames - *at) / step) as f32;
            let span = if fading { (n - i) as f32 / n as f32 } else { 1.0 };
            let gain = level * span.min((*played as f32 + 1.0) / FADE).min(remaining / FADE).clamp(0.0, 1.0);
            l[i] += left * gain;
            r[i] += right * gain;
            *at += step;
            *played += 1;
        }
    }

    fn pour_warped(warp: &Warped, wave: &Wave, at: &mut f64, cursor: &mut usize, l: &mut [f32], r: &mut [f32], step: f64, fading: bool, level: f32) {
        let n = l.len();
        for i in 0..n {
            if *at >= warp.end() {
                *at -= warp.end();
                *cursor = 0;
            }
            let (left, right) = warp.sample(wave, *at, step, 1.0, cursor);
            let gain = level * if fading { (n - i) as f32 / n as f32 } else { 1.0 };
            l[i] += left * gain;
            r[i] += right * gain;
            *at += step;
        }
    }
}

#[derive(Clone, Copy)]
struct Key {
    frame: i64,
    drift: f64,
}

#[derive(Clone, Copy)]
struct Active {
    track: usize,
    pitch: u8,
    end: f64,
}

#[derive(Clone, Copy)]
struct Outgoing {
    beat: f64,
    done: usize,
}

impl Outgoing {
    fn at(beat: f64) -> Outgoing {
        Outgoing { beat, done: 0 }
    }
}

pub struct Renderer {
    receiver: Receiver<Message>,
    reclaim: SyncSender<Displaced>,
    spilled: RefCell<Vec<Displaced>>,
    retired: Arc<AtomicUsize>,
    meters: Arc<Meters>,
    table: Arc<Table>,
    count: usize,
    master: Fader,
    solo: Option<usize>,
    click: bool,
    metronome: Metronome,
    sample_rate: f64,
    max_frames: usize,
    patience: Duration,
    deadline: Instant,
    alone: f64,
    woken: usize,
    bpm: f64,
    frames_per_beat: f64,
    song: Box<Song>,
    leaders: [Option<usize>; TRACKS],
    midi_order: [usize; TRACKS],
    curves: Box<Curves>,
    lfos: Box<Lfos>,
    lagged: Lagged,
    return_curves: Box<Curves>,
    return_lfos: Box<Lfos>,
    return_lagged: Lagged,
    cursors: Vec<usize>,
    keys: [Option<Key>; TRACKS],
    active: Vec<Active>,
    rehearsal: Option<Box<Rehearsal>>,
    rehearsal_cursor: usize,
    rehearsal_began: i64,
    rehearsed: Vec<Active>,
    playing: bool,
    position: f64,
    rested: f64,
    rested_seconds: f64,
    rest_beats: f64,
    rest_seconds: f64,
    anchor: f64,
    anchor_frame: i64,
    origin: f64,
    tail: Option<Outgoing>,
    lanes_due: bool,
    preview: Option<Preview>,
    fading: Option<Preview>,
    preview_gain: f32,
    range: Option<(f64, f64)>,
    playhead: Arc<AtomicU64>,
    wraps: u16,
    plays: u16,
    frame: i64,
    serving: [Target; SCOPES],
    replan: bool,
    after: [u64; TRACKS],
    core_order: [usize; TRACKS],
    threads: usize,
    reschedule: bool,
    pending: Vec<Message>,
    ready: usize,
    parted: bool,
    batches: u64,
    mark: Mark,
    reach: usize,
    splice: Splice,
    output: usize,
    master_rows: Lags,
    master_signal_l: Vec<f32>,
    master_signal_r: Vec<f32>,
    span: Option<(i64, i64)>,
    measuring: Option<usize>,
    measured: Option<(i64, i64)>,
    printing: Option<Vec<Vec<Printer>>>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Splice {
    Settled,
    Down(usize),
    Held,
    Up(usize),
}

impl Splice {
    fn gain(self, i: usize) -> f32 {
        match self {
            Splice::Settled => 1.0,
            Splice::Down(done) => edge(done + i),
            Splice::Held => 0.0,
            Splice::Up(done) => 1.0 - edge(done + i),
        }
    }

    fn advanced(self, n: usize) -> Splice {
        match self {
            Splice::Settled => Splice::Settled,
            Splice::Down(done) => Splice::Down((done + n).min(SPLICE)),
            Splice::Held => Splice::Held,
            Splice::Up(done) => {
                if done + n >= SPLICE {
                    Splice::Settled
                } else {
                    Splice::Up(done + n)
                }
            }
        }
    }

    pub(super) fn down(self) -> Splice {
        match self {
            Splice::Settled => Splice::Down(0),
            Splice::Down(done) => Splice::Down(done),
            Splice::Held => Splice::Held,
            Splice::Up(done) => Splice::Down(SPLICE - done),
        }
    }

    fn holds(self) -> usize {
        match self {
            Splice::Down(done) if done < SPLICE => SPLICE - done,
            _ => usize::MAX,
        }
    }

    fn reached(self) -> bool {
        matches!(self, Splice::Down(done) if done >= SPLICE)
    }
}

impl Renderer {
    pub(super) fn new(receiver: Receiver<Message>, reclaim: SyncSender<Displaced>, retired: Arc<AtomicUsize>, meters: Arc<Meters>, table: Arc<Table>, playhead: Arc<AtomicU64>, sample_rate: f64, max_frames: usize) -> Renderer {
        Renderer {
            receiver,
            reclaim,
            spilled: RefCell::new(Vec::with_capacity(RECLAIM)),
            retired,
            meters,
            table,
            count: 0,
            master: Fader::UNITY,
            solo: None,
            click: false,
            metronome: Metronome::new(sample_rate),
            sample_rate,
            max_frames,
            patience: PATIENCE,
            deadline: Instant::now(),
            alone: 0.0,
            woken: 0,
            bpm: DEFAULT_BPM,
            frames_per_beat: frames_per_beat(sample_rate, DEFAULT_BPM),
            song: Box::new(Vec::new()),
            leaders: [None; TRACKS],
            midi_order: std::array::from_fn(|track| track),
            curves: Box::new(Vec::new()),
            lfos: Box::new(Vec::new()),
            lagged: Vec::new(),
            return_curves: Box::new(Vec::new()),
            return_lfos: Box::new(Vec::new()),
            return_lagged: Vec::new(),
            cursors: Vec::with_capacity(TRACKS),
            keys: [None; TRACKS],
            active: Vec::with_capacity(ACTIVE_NOTES),
            rehearsal: None,
            rehearsal_cursor: 0,
            rehearsal_began: 0,
            rehearsed: Vec::with_capacity(ACTIVE_NOTES),
            playing: false,
            position: 0.0,
            rested: 0.0,
            rested_seconds: 0.0,
            rest_beats: 0.0,
            rest_seconds: 0.0,
            anchor: 0.0,
            anchor_frame: 0,
            origin: 0.0,
            tail: None,
            lanes_due: true,
            preview: None,
            fading: None,
            preview_gain: 1.0,
            range: None,
            playhead,
            wraps: 0,
            plays: 0,
            frame: 0,
            serving: [Target::NONE; SCOPES],
            replan: false,
            after: [0; TRACKS],
            core_order: std::array::from_fn(|track| track),
            threads: 0,
            reschedule: false,
            pending: Vec::with_capacity(PENDING),
            ready: 0,
            parted: false,
            batches: 0,
            mark: Mark::EMPTY,
            reach: 0,
            splice: Splice::Settled,
            output: 0,
            master_rows: [Default::default(); RACK_ROWS],
            master_signal_l: vec![0.0; max_frames],
            master_signal_r: vec![0.0; max_frames],
            span: None,
            measuring: None,
            measured: None,
            printing: None,
        }
    }
}

const _: () = assert!(TRACKS <= u64::BITS as usize);

fn soonest(free: &[f32]) -> usize {
    free.iter().enumerate().min_by(|a, b| a.1.total_cmp(b.1)).map_or(0, |(thread, _)| thread)
}

impl Renderer {
    pub fn workers(&self) -> Workers {
        Workers { table: Arc::clone(&self.table) }
    }

    pub fn start_workers<F>(&self, count: usize, promote: F)
    where
        F: Fn() -> Demote + Send + Sync + 'static,
    {
        start_pool(&self.table, count, promote);
    }

    pub fn patient(&mut self) {
        self.patience = PATIENT;
    }

    pub(super) fn print(&mut self) {
        self.printing = Some(Vec::new());
    }

    fn print_row(&mut self, track: usize, row: usize, stream: &Stream, line: Segment) {
        let Some(printing) = &mut self.printing else {
            return;
        };
        if printing.len() <= track {
            printing.resize_with(track + 1, Vec::new);
        }
        let rows = &mut printing[track];
        if rows.len() <= row {
            rows.resize_with(row + 1, Printer::new);
        }
        rows[row].hear(stream, line);
    }

    pub(super) fn printed(&mut self) -> Vec<Vec<Vec<SongNote>>> {
        let beat = self.line().beat_at(self.frame);
        self.printing.take().unwrap_or_default().into_iter().map(|rows| rows.into_iter().map(|row| row.printed(beat)).collect()).collect()
    }

    pub(super) fn resting(&self) -> bool {
        let sung = self.song.iter().zip(&self.cursors).all(|(notes, cursor)| *cursor >= notes.len());
        let idle = self.table.tracks.iter().take(self.count).all(|cell| {
            cell.rows.iter().all(|row| match grab(row).as_deref() {
                Some(Some(Insert::Midi { runner, stream, .. })) => runner.idle(stream.sounding()),
                _ => true,
            })
        });
        sung && self.active.is_empty() && idle
    }

    fn retire(&self, displaced: Displaced) {
        self.retired.fetch_add(1, Ordering::Relaxed);
        if let Err(TrySendError::Full(displaced)) = self.reclaim.try_send(displaced) {
            let mut spilled = self.spilled.borrow_mut();
            if spilled.len() < spilled.capacity() {
                spilled.push(displaced);
            }
        }
    }

    fn unspill(&self) {
        let mut spilled = self.spilled.borrow_mut();
        while let Some(displaced) = spilled.pop() {
            if let Err(TrySendError::Full(displaced)) = self.reclaim.try_send(displaced) {
                spilled.push(displaced);
                return;
            }
        }
    }

    fn plan(&mut self) {
        let count = self.count.min(self.table.tracks.len());
        let mut sources = [0u64; TRACKS];
        let mut members = [0u64; TRACKS];
        for (track, fed) in sources.iter_mut().enumerate().take(count) {
            *fed = self.sources_of(track, count);
        }
        let mixed = self.table.sends.iter().map(|cell| &cell.rows).chain(std::iter::once(&self.table.master));
        let listened = mixed.flat_map(|rows| rows.iter()).fold(sources.iter().fold(0, |heard, fed| heard | fed), |heard, row| match grab(row).as_deref() {
            Some(Some(Insert::Effect { slot, .. })) => heard | source_of(slot, TRACKS, count),
            _ => heard,
        });
        for track in 0..count {
            if let Some(group) = peek(&self.table.tracks[track].strip).and_then(|strip| strip.group).filter(|&group| group < count && group != track) {
                members[group] |= 1 << track;
                sources[group] |= 1 << track;
            }
        }
        let listened = bits(listened).fold(listened, |heard, source| heard | members[source]);
        let mut feeds = [0u64; TRACKS];
        for (track, fed) in sources.iter().enumerate().take(count) {
            for source in bits(*fed) {
                feeds[source] |= 1 << track;
            }
        }
        let mut inputs = [0u64; TRACKS];
        self.after = [0; TRACKS];
        let mut placed = 0u64;
        for (position, next) in placing(&sources, &members, count).into_iter().enumerate().take(count) {
            inputs[next] = sources[next] & placed;
            self.after[next] = (sources[next] | feeds[next]) & placed;
            placed |= 1 << next;
            self.table.order[position].store(next, Ordering::Relaxed);
        }
        for (track, ((cell, pours), reads)) in self.table.tracks.iter().zip(members).zip(inputs).enumerate().take(count) {
            if let Some(mut strip) = hold(&cell.strip) {
                strip.members = pours;
                strip.inputs = reads;
                strip.listened = listened & (1 << track) != 0;
            }
        }
        self.reschedule = true;
    }

    fn schedule(&mut self) {
        let count = self.count.min(self.table.tracks.len());
        self.threads = self.woken + 1;
        self.reschedule = false;
        let threads = self.threads.min(TRACKS);
        let (mut cost, mut cores, mut inputs) = ([0.0f32; TRACKS], [0.0f32; TRACKS], [0u64; TRACKS]);
        for (track, cell) in self.table.tracks.iter().enumerate().take(count) {
            if let Some(strip) = peek(&cell.strip) {
                (cost[track], cores[track], inputs[track]) = (strip.cost, strip.core_cost, strip.inputs);
            }
        }
        self.guess_unmeasured(&mut cost[..count]);
        let (mut level, mut below) = ([0.0f32; TRACKS], [0.0f32; TRACKS]);
        for position in (0..count).rev() {
            let track = self.table.order[position].load(Ordering::Relaxed);
            level[track] = cost[track] + below[track];
            for source in bits(inputs[track]) {
                below[source] = below[source].max(level[track]);
            }
        }
        self.core_order = std::array::from_fn(|track| track);
        self.core_order[..count].sort_unstable_by(|&a, &b| (cores[b] + level[b]).total_cmp(&(cores[a] + level[a])));
        let (mut free, mut rendered) = ([0.0f32; TRACKS], [0.0f32; TRACKS]);
        for &track in self.core_order[..count].iter().filter(|&&track| cores[track] > 0.0) {
            let thread = soonest(&free[..threads]);
            free[thread] += cores[track];
            rendered[track] = free[thread];
        }
        let mut finish = [0.0f32; TRACKS];
        let mut placed = 0u64;
        for position in 0..count {
            let thread = soonest(&free[..threads]);
            let now = free[thread];
            let mut best = None;
            for track in (0..count).filter(|&track| placed & (1 << track) == 0) {
                let at = bits(inputs[track]).map(|source| finish[source]).fold(rendered[track], f32::max);
                let urgency = if self.after[track] & !placed != 0 { f32::NEG_INFINITY } else if at <= now { level[track] } else { -at };
                if best.is_none_or(|(_, _, held)| urgency > held) {
                    best = Some((track, at, urgency));
                }
            }
            let Some((track, at, _)) = best else {
                break;
            };
            finish[track] = now.max(at) + cost[track];
            free[thread] = finish[track];
            placed |= 1 << track;
            self.table.order[position].store(track, Ordering::Relaxed);
        }
    }

    fn guess_unmeasured(&self, cost: &mut [f32]) {
        if cost.iter().all(|&cost| cost > 0.0) {
            return;
        }
        let mut rows = [0.0f32; TRACKS];
        for (track, cell) in self.table.tracks.iter().enumerate().take(cost.len()) {
            rows[track] = Table::len(&cell.rows) as f32 + 1.0;
        }
        let (spent, counted) = cost.iter().zip(rows).filter(|(cost, _)| **cost > 0.0).fold((0.0, 0.0), |(spent, counted), (cost, rows)| (spent + cost, counted + rows));
        let per_row = if counted > 0.0 { spent / counted } else { 1.0 };
        for (cost, rows) in cost.iter_mut().zip(rows) {
            if *cost <= 0.0 {
                *cost = rows * per_row;
            }
        }
    }

    fn sources_of(&self, track: usize, count: usize) -> u64 {
        let mut sources = 0;
        for row in &self.table.tracks[track].rows {
            if let Some(row) = grab(row)
                && let Some(Insert::Effect { slot, .. }) = row.as_ref()
            {
                sources |= source_of(slot, track, count);
            }
        }
        sources
    }

    pub(super) fn mix(&self, n: usize, generation: u64) {
        for index in 0..self.count {
            let Some(cell) = self.table.tracks.get(index) else {
                break;
            };
            let Some(strip) = peek(&cell.strip) else {
                continue;
            };
            if strip.pass != generation || !strip.heard || strip.l.len() < n {
                continue;
            }
            let offset = strip.plan.as_ref().map_or(0, |plan| plan.to_returns);
            for (send, cell) in self.table.sends.iter().enumerate() {
                let amount = strip.sends[send];
                if amount <= 0.0 {
                    continue;
                }
                let Some(mut ret) = grab(&cell.strip) else {
                    continue;
                };
                if ret.l.len() < n {
                    continue;
                }
                if ret.fed != generation {
                    ret.l[..n].fill(0.0);
                    ret.r[..n].fill(0.0);
                    ret.fed = generation;
                }
                let Return { l, r, .. } = &mut *ret;
                strip.out.add(offset, amount, &mut l[..n], &mut r[..n]);
            }
        }
    }

    pub(super) fn blend(&self, out_l: &mut [f32], out_r: &mut [f32], n: usize, d: &Dispatch) {
        if self.solo.is_none() {
            for cell in self.table.tracks.iter().take(self.count) {
                let Some(strip) = peek(&cell.strip) else {
                    continue;
                };
                if strip.pass != d.generation || !strip.heard || strip.group.is_some() || strip.l.len() < n {
                    continue;
                }
                strip.out.add(strip.plan.as_ref().map_or(0, |plan| plan.to_master), 1.0, &mut out_l[..n], &mut out_r[..n]);
            }
        }
        for (index, cell) in self.table.sends.iter().enumerate() {
            if !self.solo.is_none_or(|solo| solo == index) {
                continue;
            }
            let Some(ret) = grab(&cell.strip) else {
                continue;
            };
            if ret.pass != d.generation || ret.l.len() < n {
                continue;
            }
            ret.line.add(ret.plan.to_master, 1.0, &mut out_l[..n], &mut out_r[..n]);
        }
    }

    pub fn render(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let started = Instant::now();
        self.drain();
        let n = out_l.len().min(out_r.len()).min(self.max_frames);
        self.deadline = Instant::now() + self.patience;
        let mut done = 0;
        while done < n && self.splice != Splice::Held {
            let seek = self.frames_to_seek();
            let step = self.frames_to_lane_grid().min(self.frames_to_window_grid());
            let cut = seek.unwrap_or(usize::MAX).min(step).min(self.splice.holds()).min(self.frames_to_shift());
            let span = cut.min(n - done);
            if span > 0 {
                self.render_span(&mut out_l[done..done + span], &mut out_r[done..done + span], (step != usize::MAX).then_some(cut));
                self.lanes_due = span == cut;
                done += span;
            }
            if self.splice.reached() && self.table.idle() {
                self.install_pending();
                self.splice = if self.parted { Splice::Held } else { Splice::Up(0) };
            }
            if seek.is_some_and(|frames| frames == span) && let Some((start, _)) = self.range {
                self.wraps = self.wraps.wrapping_add(1);
                self.tail = Some(Outgoing::at(self.position));
                self.seek(start);
            }
        }
        out_l[done..n].fill(0.0);
        out_r[done..n].fill(0.0);
        self.table.rest();
        self.woken = 0;
        if n > 0 {
            let busy = started.elapsed().as_secs_f64() * self.sample_rate / n as f64;
            self.meters.load.fetch_max((busy as f32).to_bits(), Ordering::Relaxed);
            if busy > 1.0 {
                self.meters.dropouts.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub(super) fn render_span(&mut self, out_l: &mut [f32], out_r: &mut [f32], lane_step: Option<usize>) {
        let n = out_l.len().min(out_r.len()).min(self.max_frames);
        let started = Instant::now();
        if self.woken == 0 && self.alone > BUSY_WAKE {
            self.woken = self.table.wake();
        }
        let at = self.position;
        let lane = self.lane();
        let beat = self.playing.then_some(at);
        self.advance(n);
        self.rehearse(n);
        let (now, frames_per_beat) = (self.frame, self.frames_per_beat);
        let transport = beat.map(|beat| Transport { beat });
        let pour = beat.map(|beat| Pour { beat, origin: self.origin });
        let tail = self.tail.take();
        self.tail = tail.filter(|outgoing| outgoing.done + n < SPLICE).map(|outgoing| Outgoing { beat: outgoing.beat + n as f64 / frames_per_beat, done: outgoing.done + n });
        let tail = tail.map(|outgoing| (outgoing.beat, Ramp::Down { over: SPLICE, done: outgoing.done }));
        let laned = std::mem::take(&mut self.lanes_due) || lane_step.is_none();
        let reach = lane_step.unwrap_or(n);
        if laned {
            self.automate(at, lane, reach);
            self.automate_returns(at, lane, reach);
        }
        out_l[..n].fill(0.0);
        out_r[..n].fill(0.0);
        let targets: [Target; SCOPES] = std::array::from_fn(|scope| self.meters.taps[scope].aimed());
        for (scope, target) in targets.iter().enumerate() {
            if *target != self.serving[scope] {
                self.serving[scope] = *target;
                self.meters.taps[scope].serve(*target);
            }
        }
        let generation = self.table.generation.load(Ordering::Relaxed) + 1;
        let synced = beat.map(|_| Beat { anchor: self.anchor, since: now - self.anchor_frame, frames_per_beat });
        if self.reschedule || self.threads != self.woken + 1 {
            self.schedule();
        }
        let timed = generation.is_multiple_of(TIMED_SPANS);
        let mut d = Dispatch { frames: n, tracks: self.count, cores: 0, generation, now, frames_per_beat, transport, pour, tail, clock: Clock { sample_rate: self.sample_rate, bpm: self.bpm, beat: synced }, mark: self.mark, targets, deadline: self.deadline, line: self.line(), span: self.span, window: None, measured: self.measured, timed, batches: self.batches };
        let (table, order, leaders, count) = (Arc::clone(&self.table), self.core_order, self.leaders, self.count);
        let midi = self.midi_order;
        let listed = midi[..count].iter().fold(0u64, |seen, &track| seen | 1u64.checked_shl(track as u32).unwrap_or(0));
        let every = u64::MAX.checked_shr((TRACKS - count) as u32).unwrap_or(0);
        let midi = if listed == every { midi } else { std::array::from_fn(|track| track) };
        let (line, dispatched) = (d.line, d);
        d.cores = table.prime(&midi[..count], &order[..count], &leaders, &dispatched, &mut |track, row, stream| match stream {
            None if laned => self.wave_row(track, row, at, lane, reach),
            None => {}
            Some(stream) => self.print_row(track, row, stream, line),
        });
        if laned {
            self.wave(at, lane, reach);
        }
        if !self.playing {
            self.rest_beats += n as f64 / frames_per_beat;
            self.rest_seconds += n as f64 / self.sample_rate;
        }
        self.frame += n as i64;
        if self.printing.is_some() {
            return;
        }
        for phase in [&self.table.cores, &self.table.strips, &self.table.returns] {
            phase.reset(generation);
        }
        loop {
            if let Some(mut block) = grab(&self.table.block) {
                *block = d;
                break;
            }
            std::hint::spin_loop();
        }
        self.table.generation.store(generation, Ordering::Release);
        self.table.work_cores(&d);
        self.table.work_strips(&d);
        self.table.join(&self.table.cores, &d, d.cores);
        self.table.join(&self.table.strips, &d, d.tracks);
        self.reschedule |= timed;
        self.mix(n, generation);
        self.table.mixed.store(generation, Ordering::Release);
        self.table.work_returns(&d);
        self.table.join(&self.table.returns, &d, SEND_COUNT);
        self.blend(out_l, out_r, n, &d);
        self.table.feed_mixed(&self.table.master, &self.master_rows, &mut self.master_signal_l[..n], &mut self.master_signal_r[..n], &d);
        run_rows(&self.table.master, Host::Master, &mut out_l[..n], &mut out_r[..n], &self.meters, &d, &self.master_rows, &self.table.segments, d.measured);
        if let Some(mut fading) = self.fading.take() {
            fading.pour(&mut out_l[..n], &mut out_r[..n], self.sample_rate, true, self.preview_gain);
            self.retire(Displaced::Preview(fading.cue));
        }
        if let Some(preview) = &mut self.preview {
            preview.pour(&mut out_l[..n], &mut out_r[..n], self.sample_rate, false, self.preview_gain);
        }
        publish(&self.meters.preview_progress, self.preview.as_ref().map_or(f64::NAN, Preview::progress));
        publish(&self.meters.rehearsal_beat, self.rehearsal.as_ref().map_or(f64::NAN, |_| (now - self.rehearsal_began) as f64 / frames_per_beat));
        let heard = now - self.output as i64;
        let struck = self.table.segments.at(self.mark, heard);
        let (ticked, spacing) = if struck.frame == self.anchor_frame { (beat.map(|beat| beat - self.output as f64 / frames_per_beat), frames_per_beat) } else { (beat.map(|_| struck.beat_at(heard)), struck.frames_per_beat) };
        self.metronome.render(&mut out_l[..n], &mut out_r[..n], ticked.filter(|_| self.click), spacing);
        let splice = self.splice;
        let (mut peak_l, mut peak_r) = (0.0f32, 0.0f32);
        for i in 0..n {
            let gain = self.master.at(i) * splice.gain(i);
            let (a, b) = (finite(out_l[i] * gain), finite(out_r[i] * gain));
            peak_l = peak_l.max(a.abs());
            peak_r = peak_r.max(b.abs());
            out_l[i] = a.clamp(-1.0, 1.0);
            out_r[i] = b.clamp(-1.0, 1.0);
        }
        self.splice = splice.advanced(n);
        self.master.advance(n);
        self.meters.master.raise(peak_l, peak_r);
        if let Some(tap) = tap_for(&self.meters, &d.targets, Host::Master.output()) {
            tap.write(&out_l[..n], &out_r[..n]);
        }
        self.alone = started.elapsed().as_secs_f64() * self.sample_rate / n as f64 * (self.woken + 1) as f64;
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.table.stop.store(true, Ordering::Relaxed);
        if let Ok(workers) = self.table.workers.lock() {
            for worker in workers.iter() {
                worker.unpark();
            }
        }
    }
}
