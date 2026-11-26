use super::*;

const METER_FALL_DB_PER_SECOND: f32 = 24.0;

const METER_FLOOR: f32 = 1e-4;

const CLIP_HOLD_SECONDS: f32 = 1.5;

const LOAD_FALL_PER_SECOND: f32 = 0.5;

fn settled(level: (f32, f32), peak: (f32, f32), fall: f32) -> (f32, f32) {
    let settle = |level: f32, peak: f32| {
        let next = peak.max(level * fall);
        if next < METER_FLOOR { 0.0 } else { next }
    };
    (settle(level.0, peak.0), settle(level.1, peak.1))
}

fn held(hold: (f32, f32), peak: (f32, f32), seconds: f32) -> (f32, f32) {
    let next = |hold: f32, peak: f32| if peak >= 1.0 { CLIP_HOLD_SECONDS } else { (hold - seconds).max(0.0) };
    (next(hold.0, peak.0), next(hold.1, peak.1))
}

fn lit(hold: (f32, f32)) -> (bool, bool) {
    (hold.0 > 0.0, hold.1 > 0.0)
}

fn eased(load: f32, peak: f32, seconds: f32) -> f32 {
    peak.max(load - seconds * LOAD_FALL_PER_SECOND).max(0.0)
}

fn percent(load: f32) -> u32 {
    (load * 100.0).round() as u32
}

fn watched(meters: &Meters, host: Host, rack: &[Row], gauges: &mut [f32; RACK_ROWS], signals: &mut [f32; RACK_ROWS], strips: &mut [Strips; RACK_ROWS]) -> bool {
    let (read, signalled) = split_gauges(meters.read(host, rack));
    let banded = meters.read_strips(host);
    let changed = read != *gauges || signalled != *signals || banded != *strips;
    (*gauges, *signals, *strips) = (read, signalled, banded);
    changed
}

struct Watched<'a> {
    rack: &'a [Row],
    places: &'a [usize],
    gauges: &'a [f32; RACK_ROWS],
    signals: &'a [f32; RACK_ROWS],
    strips: &'a [Strips; RACK_ROWS],
}

impl Watched<'_> {
    fn gauge(&self, row: usize) -> f32 {
        let slots = copies(self.places, row);
        let read = |slot: usize| self.gauges.get(slot).copied().unwrap_or(0.0);
        let adds = matches!(self.rack.get(slots.start), Some(Row::Effect(slot)) if slot.kind().meters_reduction());
        if adds { slots.map(read).sum() } else { read(slots.start) }
    }

    fn signal(&self, row: usize) -> f32 {
        self.signals.get(copies(self.places, row).start).copied().unwrap_or(f32::NEG_INFINITY)
    }

    fn strips(&self, row: usize) -> Strips {
        let slots = copies(self.places, row);
        let read = |slot: usize| self.strips.get(slot).copied().unwrap_or([[0.0; 2]; FREAK_BANDS]);
        let last = read(slots.end.saturating_sub(1));
        std::array::from_fn(|band| [slots.clone().map(|slot| read(slot)[band][0]).sum(), last[band][1]])
    }
}

fn finish(ears: &[Mutex<Option<Ear>>]) -> Vec<Option<Heard>> {
    ears.iter().map(|ear| ear.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take().map(Ear::finish)).collect()
}

impl Audio {
    pub fn tick(&mut self, seconds: f32) -> bool {
        self.link.commit();
        self.link.flush();
        self.reclaim();
        let rendered = self.seer.poll();
        self.deliver(rendered);
        let twins = self.twin_seer.poll();
        self.deliver_twins(twins);
        let progress = self.seer.progress().or(self.twin_seer.progress());
        let fall = (-seconds * METER_FALL_DB_PER_SECOND * 10f32.ln() / 20.0).exp();
        let mut changed = progress != self.progress;
        self.progress = progress;
        for (index, (mirror, meter)) in self.tracks.iter_mut().zip(self.meters.tracks.iter()).enumerate() {
            let level = settled(mirror.level, meter.take(), fall);
            changed |= level != mirror.level;
            mirror.level = level;
            changed |= watched(&self.meters, Host::Track(index), &mirror.rack, &mut mirror.gauges, &mut mirror.signals, &mut mirror.strips);
        }
        for (index, (mirror, meter)) in self.sends.iter_mut().zip(self.meters.sends.iter()).enumerate() {
            let level = settled(mirror.level, meter.take(), fall);
            changed |= level != mirror.level;
            mirror.level = level;
            changed |= watched(&self.meters, Host::Send(index), &mirror.rack, &mut mirror.gauges, &mut mirror.signals, &mut mirror.strips);
        }
        changed |= watched(&self.meters, Host::Master, &self.master.rack, &mut self.master.gauges, &mut self.master.signals, &mut self.master.strips);
        let peak = self.meters.master.take();
        let clip_hold = held(self.clip_hold, peak, seconds);
        let level = settled(self.master.level, peak, fall);
        changed |= lit(clip_hold) != lit(self.clip_hold) || level != self.master.level;
        self.clip_hold = clip_hold;
        self.master.level = level;
        let load = eased(self.load, f32::from_bits(self.meters.load.swap(0, Ordering::Relaxed)), seconds);
        changed |= percent(load) != percent(self.load);
        self.load = load;
        for (tap, ((spectrum, shown), aimed)) in self.meters.taps.iter().zip(self.spectra.iter_mut().zip(self.cuts.iter_mut()).zip(&self.aimed)) {
            if *aimed == Target::NONE {
                continue;
            }
            let fresh = self.analyzer.analyze(tap).then(|| self.analyzer.cells());
            changed |= spectrum.step(fresh, seconds);
            let cuts = tap.read_curve();
            changed |= cuts != *shown;
            *shown = cuts;
        }
        changed
    }

    pub fn aim(&mut self, scope: Scope, target: Target) {
        let scope = scope as usize;
        if self.aimed[scope] == target {
            return;
        }
        self.aimed[scope] = target;
        self.meters.taps[scope].aim(target);
        self.spectra[scope].rest();
        self.cuts[scope] = [0.0; sdk::spectrum::CURVE];
    }

    pub fn spectrum(&self, scope: Scope) -> &Spectrum {
        &self.spectra[scope as usize]
    }

    pub fn cuts(&self, scope: Scope) -> &[f32; sdk::spectrum::CURVE] {
        &self.cuts[scope as usize]
    }

    pub fn level(&self, track: usize) -> (f32, f32) {
        self.tracks.get(track).map_or((0.0, 0.0), |mirror| mirror.level)
    }

    fn watched(&self, host: Host) -> Option<Watched<'_>> {
        match host {
            Host::Track(track) => self.tracks.get(track).map(|mirror| Watched { rack: &mirror.rack, places: &mirror.places, gauges: &mirror.gauges, signals: &mirror.signals, strips: &mirror.strips }),
            Host::Send(send) => self.sends.get(send).map(|mirror| Watched { rack: &mirror.rack, places: &mirror.places, gauges: &mirror.gauges, signals: &mirror.signals, strips: &mirror.strips }),
            Host::Master => Some(Watched { rack: &self.master.rack, places: &self.master.places, gauges: &self.master.gauges, signals: &self.master.signals, strips: &self.master.strips }),
        }
    }

    pub fn tapped(&self, host: Host, row: usize) -> Target {
        let places = self.watched(host).map_or(&[][..], |watched| watched.places);
        host.insert(copies(places, row).end.saturating_sub(1))
    }

    pub fn settled(&mut self, playing: bool) -> bool {
        if self.link.sending() {
            return false;
        }
        let batches = self.link.batches();
        let mut settled = false;
        for (track, mirror) in self.tracks.iter_mut().enumerate().filter(|(_, mirror)| mirror.lingering.iter().any(|held| *held > 0)) {
            let spliced = playing && self.timing.tracks.get(track).is_none_or(TrackTiming::staggered);
            let Some(sounding) = self.meters.gauges(Host::Track(track)).lingering(batches).filter(|_| !spliced) else {
                continue;
            };
            for (row, held) in mirror.lingering.iter_mut().enumerate().filter(|(_, held)| **held > 0) {
                let end = copies(&mirror.places, row).end;
                if (end.saturating_sub(*held)..end).all(|slot| sounding >> slot & 1 == 0) {
                    *held = 0;
                    settled = true;
                }
            }
        }
        settled
    }

    pub(crate) fn deepen(&self, hosts: &[Host], deepest: &mut [[f32; RACK_ROWS]]) {
        for (host, rows) in hosts.iter().zip(deepest) {
            let gauges = self.meters.gauges(*host);
            let places = self.watched(*host).map_or(&[][..], |watched| watched.places);
            for (row, held) in rows.iter_mut().enumerate() {
                let taken: f32 = copies(places, row).filter(|slot| *slot < RACK_ROWS).map(|slot| gauges.take(slot)).sum();
                *held = held.max(taken);
            }
        }
    }

    pub(crate) fn hear(&self, rate: f64, bar: usize, tracks: usize) {
        for ear in self.meters.track_ears.iter().take(tracks).chain(&self.meters.send_ears) {
            *ear.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Ear::sparse(rate, bar));
        }
    }

    pub(crate) fn heard_tracks(&self) -> Vec<Option<Heard>> {
        finish(&self.meters.track_ears)
    }

    pub(crate) fn heard_returns(&self) -> Vec<Option<Heard>> {
        finish(&self.meters.send_ears)
    }

    pub(crate) fn master_peak(&self) -> f32 {
        let (left, right) = self.meters.master.take();
        left.max(right)
    }

    pub fn gauge(&self, host: Host, index: usize) -> f32 {
        self.watched(host).map_or(0.0, |watched| watched.gauge(index))
    }

    pub fn signal(&self, host: Host, index: usize) -> f32 {
        self.watched(host).map_or(f32::NEG_INFINITY, |watched| watched.signal(index))
    }

    pub fn strips(&self, host: Host, index: usize) -> [[f32; 2]; FREAK_BANDS] {
        self.watched(host).map_or([[0.0; 2]; FREAK_BANDS], |watched| watched.strips(index))
    }

    pub fn send_level(&self, send: usize) -> (f32, f32) {
        self.sends.get(send).map_or((0.0, 0.0), |mirror| mirror.level)
    }

    pub fn master_level(&self) -> (f32, f32) {
        self.master.level
    }

    pub fn clip(&self) -> (bool, bool) {
        lit(self.clip_hold)
    }

    pub fn clear_clip(&mut self) {
        self.clip_hold = (0.0, 0.0);
    }

    pub fn dropouts(&self) -> u32 {
        self.meters.dropouts.load(Ordering::Relaxed)
    }

    pub fn load(&self) -> u32 {
        percent(self.load)
    }

    pub fn rendering(&self) -> Option<f32> {
        self.progress
    }
}
