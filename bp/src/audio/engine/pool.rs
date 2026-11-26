use super::*;

const CLOCK_SPINS: u32 = 256;

const PHASE_SHIFT: u32 = 24;

const PHASE_MASK: u64 = (1 << PHASE_SHIFT) - 1;

pub(in crate::audio) struct Phase {
    cursor: AtomicU64,
    done: AtomicU64,
}

impl Phase {
    pub(super) fn new() -> Phase {
        Phase { cursor: AtomicU64::new(0), done: AtomicU64::new(0) }
    }

    pub(in crate::audio) fn reset(&self, generation: u64) {
        self.cursor.store(generation << PHASE_SHIFT, Ordering::Relaxed);
        self.done.store(generation << PHASE_SHIFT, Ordering::Relaxed);
    }

    pub(super) fn claim(&self, generation: u64, count: usize) -> Option<usize> {
        let mut seen = self.cursor.load(Ordering::Relaxed);
        loop {
            let id = (seen & PHASE_MASK) as usize;
            if seen >> PHASE_SHIFT != generation || id >= count {
                return None;
            }
            match self.cursor.compare_exchange_weak(seen, seen + 1, Ordering::SeqCst, Ordering::Relaxed) {
                Ok(_) => return Some(id),
                Err(current) => seen = current,
            }
        }
    }

    pub(super) fn close(&self, generation: u64) {
        self.cursor.fetch_max((generation << PHASE_SHIFT) | PHASE_MASK, Ordering::SeqCst);
    }

    pub(super) fn finish(&self, generation: u64) {
        let mut seen = self.done.load(Ordering::Relaxed);
        loop {
            if seen >> PHASE_SHIFT != generation {
                return;
            }
            match self.done.compare_exchange_weak(seen, seen + 1, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return,
                Err(current) => seen = current,
            }
        }
    }

    pub(super) fn finished(&self, generation: u64, count: usize) -> bool {
        let seen = self.done.load(Ordering::Acquire);
        seen >> PHASE_SHIFT == generation && (seen & PHASE_MASK) as usize >= count
    }
}

pub(in crate::audio) struct Countdown(AtomicU64);

impl Countdown {
    pub(super) fn new() -> Countdown {
        Countdown(AtomicU64::new(0))
    }

    fn set(&self, generation: u64, count: usize) {
        self.0.store((generation << PHASE_SHIFT) | count as u64, Ordering::Relaxed);
    }

    fn tick(&self, generation: u64) {
        let mut seen = self.0.load(Ordering::Relaxed);
        while seen >> PHASE_SHIFT == generation && seen & PHASE_MASK > 0 {
            match self.0.compare_exchange_weak(seen, seen - 1, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return,
                Err(current) => seen = current,
            }
        }
    }

    fn reached(&self, generation: u64) -> bool {
        self.0.load(Ordering::Acquire) == generation << PHASE_SHIFT
    }
}

struct Turn<'a> {
    busy: &'a AtomicUsize,
    phase: &'a Phase,
    generation: u64,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.busy.fetch_sub(1, Ordering::SeqCst);
        self.phase.finish(self.generation);
    }
}

pub type Demote = Option<Box<dyn FnOnce() + Send>>;

#[derive(Clone, Copy)]
pub(in crate::audio) struct Lead {
    pub(in crate::audio) memory: f64,
    pub(in crate::audio) step: f64,
    pub(in crate::audio) recalls: bool,
    pub(in crate::audio) each_note: bool,
}

impl Lead {
    fn of(row: &MidiRow, bpm: f64) -> Lead {
        let arp = row.heard();
        Lead { memory: arp.memory(bpm), step: arp.step_beats(bpm), recalls: arp.recalls(), each_note: arp.restarts_each_note() }
    }

    fn and(self, row: &MidiRow, bpm: f64) -> Lead {
        let below = Lead::of(row, bpm);
        Lead { memory: self.memory + below.memory, step: self.step.min(below.step), recalls: self.recalls || below.recalls, each_note: self.each_note }
    }
}

pub struct Workers {
    pub(in crate::audio) table: Arc<Table>,
}

impl Workers {
    pub fn start<F>(&self, count: usize, promote: F)
    where
        F: Fn() -> Demote + Send + Sync + 'static,
    {
        start_pool(&self.table, count, promote);
    }
}

fn work(table: &Table) {
    let mut seen = 0u64;
    while !table.stop.load(Ordering::Relaxed) {
        let generation = table.generation.load(Ordering::Acquire);
        if generation == seen {
            if table.resting.load(Ordering::Acquire) {
                std::thread::park();
            } else {
                std::hint::spin_loop();
            }
            continue;
        }
        seen = generation;
        let d = loop {
            if let Some(block) = grab(&table.block) {
                break *block;
            }
            std::hint::spin_loop();
        };
        if d.generation != generation {
            continue;
        }
        table.work_cores(&d);
        table.work_strips(&d);
        if !table.until(&d, || table.mixed.load(Ordering::Acquire) >= generation) {
            continue;
        }
        table.work_returns(&d);
    }
}

pub(in crate::audio) fn start_pool<F>(table: &Arc<Table>, count: usize, promote: F)
where
    F: Fn() -> Demote + Send + Sync + 'static,
{
    if table.workers.lock().is_ok_and(|workers| !workers.is_empty()) {
        return;
    }
    let promote = Arc::new(promote);
    for _ in 0..count {
        let shared = Arc::clone(table);
        let promote = Arc::clone(&promote);
        let Ok(handle) = std::thread::Builder::new().name(String::from("bp-render")).spawn(move || {
            let demote = promote();
            work(&shared);
            if let Some(demote) = demote {
                demote();
            }
        }) else {
            return;
        };
        if let Ok(mut workers) = table.workers.lock() {
            workers.push(handle.thread().clone());
        }
    }
}

impl Table {
    pub(in crate::audio) fn wake(&self) -> usize {
        self.resting.store(false, Ordering::Release);
        grab(&self.workers).map_or(0, |workers| {
            for worker in workers.iter() {
                worker.unpark();
            }
            workers.len()
        })
    }

    pub(in crate::audio) fn rest(&self) {
        self.resting.store(true, Ordering::Release);
    }

    fn until(&self, d: &Dispatch, mut ready: impl FnMut() -> bool) -> bool {
        let mut spins = 0u32;
        while !ready() {
            spins = spins.wrapping_add(1);
            if spins.is_multiple_of(CLOCK_SPINS) && self.abandoned(d) {
                return false;
            }
            std::hint::spin_loop();
        }
        true
    }

    fn abandoned(&self, d: &Dispatch) -> bool {
        self.stop.load(Ordering::Relaxed) || self.generation.load(Ordering::Acquire) != d.generation || Instant::now() >= d.deadline
    }

    pub(in crate::audio) fn join(&self, phase: &Phase, d: &Dispatch, count: usize) {
        self.until(d, || phase.finished(d.generation, count));
    }

    pub(super) fn source_ready(&self, source: &TrackCell, d: &Dispatch) -> bool {
        self.until(d, || peek(&source.strip).is_some_and(|strip| strip.pass == d.generation))
    }

    pub(in crate::audio) fn idle(&self) -> bool {
        let generation = self.generation.load(Ordering::Acquire);
        for phase in [&self.cores, &self.strips, &self.returns] {
            phase.close(generation);
        }
        self.busy.load(Ordering::SeqCst) == 0
    }

    fn fed_by<'a>(above: Option<&'a MutexGuard<'_, Option<Insert>>>) -> Option<&'a Stream> {
        match above.map(|above| above.as_ref()) {
            Some(Some(Insert::Midi { stream, .. })) => Some(&**stream),
            _ => None,
        }
    }

    pub(in crate::audio) fn is_midi(slot: &Mutex<Option<Insert>>) -> bool {
        grab(slot).is_some_and(|row| matches!(row.as_ref(), Some(Insert::Midi { .. })))
    }

    pub(in crate::audio) fn prime(&self, midi: &[usize], tracks: &[usize], leaders: &[Option<usize>; TRACKS], d: &Dispatch, hook: &mut dyn FnMut(usize, usize, Option<&Stream>)) -> usize {
        let key = self.key.load(Ordering::Relaxed);
        let (mut rendered, mut last) = ([0u64; TRACKS], [None; TRACKS]);
        for &track in midi {
            let Some(cell) = self.tracks.get(track) else {
                continue;
            };
            let leader = leaders.get(track).copied().flatten().and_then(|leader| Some((self.tracks.get(leader)?, last.get(leader).copied().flatten().filter(|&at| rendered[leader] & (1u64 << at) != 0)?)));
            let mut chain = None;
            for (position, slot) in cell.rows.iter().enumerate() {
                if Table::is_midi(slot) {
                    hook(track, position, None);
                }
                let Some(mut row) = grab(slot) else {
                    continue;
                };
                match row.as_mut() {
                    None => break,
                    Some(Insert::Effect { .. } | Insert::Core { .. }) => {}
                    Some(Insert::Bus { master, .. }) => chain = chain.and(Some(*master)),
                    Some(Insert::Midi { row, runner, stream }) => {
                        runner.snap_to(row.snap(key));
                        runner.transpose_by(row.shift());
                        runner.retire(row.leaving());
                        let above = match chain {
                            Some(above) => grab(&cell.rows[above]),
                            None => leader.and_then(|(cell, at): (&TrackCell, usize)| grab(&cell.rows[at])),
                        };
                        let struck = (chain.is_none() && leader.is_some()).then_some(&*row);
                        if d.on_track(|| cell.shift(), |d| stream.render(runner, Table::fed_by(above.as_ref()), struck, d)).is_some() {
                            rendered[track] |= 1 << position;
                            hook(track, position, Some(stream));
                        }
                        chain = Some(position);
                        last[track] = Some(position);
                    }
                }
            }
        }
        let mut listed = 0;
        for &track in tracks {
            let Some(cell) = self.tracks.get(track) else {
                continue;
            };
            let first = listed;
            let list = |position: usize, listed: &mut usize| {
                if let Some(slot) = self.core_rows.get(*listed) {
                    slot.store(track * RACK_ROWS + position, Ordering::Relaxed);
                    *listed += 1;
                }
            };
            let mut chain = None;
            for (position, slot) in cell.rows.iter().enumerate() {
                let Some(mut row) = grab(slot) else {
                    list(position, &mut listed);
                    continue;
                };
                match row.as_mut() {
                    None => break,
                    Some(Insert::Effect { .. }) => {}
                    Some(Insert::Bus { master, .. }) => chain = chain.and(Some(*master)),
                    Some(Insert::Midi { .. }) => chain = Some(position),
                    Some(Insert::Core { stream, .. }) => {
                        let above = chain.and_then(|above: usize| grab(&cell.rows[above]));
                        match Table::fed_by(above.as_ref()) {
                            Some(fed) => stream.copy_from(fed),
                            None => stream.clear(),
                        }
                        list(position, &mut listed);
                    }
                }
            }
            cell.unrendered.set(d.generation, listed - first);
        }
        for &track in midi {
            let Some(cell) = self.tracks.get(track) else {
                continue;
            };
            for position in bits(rendered[track]) {
                if let Some(mut row) = grab(&cell.rows[position])
                    && let Some(Insert::Midi { stream, .. }) = row.as_mut()
                {
                    stream.clear();
                }
            }
        }
        listed
    }

    pub(in crate::audio) fn rewind(&self, track: usize, zero: f64, bpm: f64) -> Option<Lead> {
        let cell = self.tracks.get(track)?;
        let mut lead: Option<Lead> = None;
        for (position, slot) in cell.rows.iter().enumerate() {
            let row = grab(slot)?;
            match row.as_ref() {
                None => break,
                Some(Insert::Midi { runner, .. }) if position == 0 && runner.holding() => return None,
                Some(Insert::Midi { row, .. }) if row.heard().on => lead = Some(lead.map_or_else(|| Lead::of(row, bpm), |lead| lead.and(row, bpm))),
                Some(_) => {}
            }
        }
        let lead = lead?;
        for slot in &cell.rows {
            let Some(mut row) = grab(slot) else {
                continue;
            };
            match row.as_mut() {
                None => break,
                Some(Insert::Midi { runner, stream, .. }) => {
                    stream.lapse();
                    runner.rewind(zero);
                }
                Some(_) => {}
            }
        }
        Some(lead)
    }

    pub(in crate::audio) fn roll(&self, track: usize, d: &Dispatch) {
        let Some(cell) = self.tracks.get(track) else {
            return;
        };
        let key = self.key.load(Ordering::Relaxed);
        let mut chain = None;
        for (position, slot) in cell.rows.iter().enumerate() {
            let Some(mut row) = grab(slot) else {
                continue;
            };
            match row.as_mut() {
                None => break,
                Some(Insert::Bus { master, .. }) => chain = chain.and(Some(*master)),
                Some(Insert::Midi { row, runner, stream }) => {
                    runner.snap_to(row.snap(key));
                    runner.transpose_by(row.shift());
                    runner.retire(row.leaving());
                    let above = chain.and_then(|above: usize| grab(&cell.rows[above]));
                    stream.render(runner, Table::fed_by(above.as_ref()), None, d);
                    chain = Some(position);
                }
                Some(Insert::Core { .. } | Insert::Effect { .. }) => {}
            }
        }
        for slot in &cell.rows {
            let Some(mut row) = grab(slot) else {
                continue;
            };
            match row.as_mut() {
                None => break,
                Some(Insert::Midi { stream, .. }) => stream.fold(),
                Some(_) => {}
            }
        }
    }

    fn turn<'a>(&'a self, phase: &'a Phase, generation: u64, count: usize) -> Option<(usize, Turn<'a>)> {
        self.busy.fetch_add(1, Ordering::SeqCst);
        let claimed = phase.claim(generation, count);
        if claimed.is_none() {
            self.busy.fetch_sub(1, Ordering::SeqCst);
        }
        claimed.map(|id| (id, Turn { busy: &self.busy, phase, generation }))
    }

    pub(in crate::audio) fn work_cores(&self, d: &Dispatch) {
        while let Some((at, _turn)) = self.turn(&self.cores, d.generation, d.cores) {
            let id = self.core_rows.get(at).map_or(usize::MAX, |slot| slot.load(Ordering::Relaxed));
            if let Some(cell) = self.tracks.get(id / RACK_ROWS) {
                let started = d.timed.then(Instant::now);
                if let Some(mut row) = grab(&cell.rows[id % RACK_ROWS])
                    && let Some(Insert::Core { layer, core, stream, l, r, pass }) = row.as_mut()
                {
                    d.on_track(|| cell.shift(), |d| {
                        stream.play(core.as_mut(), lane_of(layer, id), layer.leaving(), &mut l[..d.frames], &mut r[..d.frames], d);
                        *pass = d.generation;
                    });
                }
                if let Some(started) = started {
                    cell.core_nanos.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
                }
                cell.unrendered.tick(d.generation);
            }
        }
    }

    pub(in crate::audio) fn work_strips(&self, d: &Dispatch) {
        while let Some((id, _turn)) = self.turn(&self.strips, d.generation, d.tracks) {
            let track = self.order.get(id).map_or(id, |slot| slot.load(Ordering::Relaxed));
            if let Some(cell) = self.tracks.get(track) {
                self.until(d, || cell.unrendered.reached(d.generation));
                if d.timed {
                    self.await_inputs(cell, track, d);
                }
                if let Some(mut strip) = self.hold_strip(cell, d) {
                    let started = d.timed.then(Instant::now);
                    self.strip_job(cell, &mut strip, track, d);
                    if let Some(started) = started {
                        strip.charge(started.elapsed(), cell.core_nanos.swap(0, Ordering::Relaxed), d.frames);
                    }
                }
            }
        }
    }

    fn await_inputs(&self, cell: &TrackCell, track: usize, d: &Dispatch) {
        let inputs = peek(&cell.strip).map_or(0, |strip| strip.inputs);
        for source in bits(inputs).filter(|&source| self.before(source, track, d)) {
            if let Some(source) = self.tracks.get(source) {
                self.source_ready(source, d);
            }
        }
    }

    fn hold_strip<'a>(&self, cell: &'a TrackCell, d: &Dispatch) -> Option<RwLockWriteGuard<'a, Strip>> {
        let mut held = None;
        self.until(d, || {
            held = hold(&cell.strip);
            held.is_some()
        });
        held
    }

    pub(in crate::audio) fn work_returns(&self, d: &Dispatch) {
        while let Some((id, _turn)) = self.turn(&self.returns, d.generation, SEND_COUNT) {
            if let Some(cell) = self.sends.get(id)
                && let Some(mut ret) = grab(&cell.strip)
            {
                self.return_job(cell, &mut ret, id, d);
            }
        }
    }
}
