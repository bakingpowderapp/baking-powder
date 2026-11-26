use super::*;
use crate::params::{Osc, Quantize};
use sdk::notes::{Articulation, Join, Sink};
use crate::voice::SubOsc;
use sdk::dsp::unison::MAX_UNISON;
use std::cmp::Reverse;

impl Core {
    pub(super) fn frames_to_wander(&self) -> usize {
        DriftPlan::of(&self.params.analog).frames_left(&self.tempo, self.sample_clock)
    }

    pub fn note_on(&mut self, midi: u8, velocity: u8) {
        self.press(midi, velocity, true);
    }

    fn press(&mut self, midi: u8, velocity: u8, slide: bool) {
        self.settle_mono();
        let struck = self.struck.take();
        let from = self.glide_from(midi).filter(|_| slide).map(|from| (from, self.last_transpose));
        self.held.retain(|key| *key != midi);
        self.held.push(midi);
        self.last_midi = Some(midi);
        self.last_transpose = struck.map_or(0.0, |articulation| f64::from(articulation.shift));
        if self.params.voices == 1 {
            self.mono_note_on(midi, velocity, struck);
        } else {
            self.start_voice(midi, velocity, struck, from);
        }
    }

    fn glide_from(&self, midi: u8) -> Option<u8> {
        let mono = self.params.mono;
        self.last_midi.filter(|from| *from != midi && mono.glide_ms > 0.0 && (self.held.contains(from) || !mono.legato))
    }

    fn latched(&self, struck: Option<Articulation>) -> (f64, f64, f64) {
        let (cents, lane, transpose) = match struck {
            Some(articulation) => (f64::from(articulation.cents), articulation.modulation.map(f64::from), f64::from(articulation.shift)),
            None => (0.0, None, 0.0),
        };
        let modulation = if self.params.modulation.sampled { lane.unwrap_or(1.0) * self.params.modulation.sample } else { lane.unwrap_or(0.0) };
        (cents, modulation, transpose)
    }

    fn take_modulation(&mut self, modulation: f64) -> (f64, bool) {
        let from = self.last_modulation;
        self.last_modulation = modulation;
        (from, self.params.modulation.lag_ms > 0.0)
    }

    pub fn note_off(&mut self, midi: u8) {
        self.held.retain(|key| *key != midi);
        if self.params.voices == 1 {
            self.mono_note_off(midi);
        } else {
            self.end_voices(midi);
        }
    }

    pub fn release_all(&mut self) {
        for voice in &mut self.voices {
            if voice.sounding() {
                let_go(voice);
            }
        }
        self.mono.clear();
        self.held.clear();
        self.last_midi = None;
    }

    pub fn active(&self) -> bool {
        self.voices.iter().any(|voice| voice.active)
    }

    fn end_voices(&mut self, midi: u8) {
        let fresh = self.params.fresh;
        for voice in &mut self.voices {
            if voice.sounding() && voice.midi == midi && !(fresh && voice.env.phase == Phase::Release) {
                let_go(voice);
            }
        }
    }

    fn voice_index_for(&mut self, midi: u8, taken: &[usize]) -> usize {
        let cap = self.params.voices.clamp(1, VOICES);
        let open = self.voices[..cap].iter().enumerate().filter(|(index, _)| !taken.contains(index));
        let same = if self.params.fresh { None } else { open.clone().find(|(_, voice)| voice.sounding() && voice.midi == midi) };
        let free = open.clone().find(|(_, voice)| !voice.active);
        if let Some((found, _)) = same.or(free) {
            return found;
        }
        let faintest = self.faintest(cap, taken);
        self.steal(faintest, cap)
    }

    fn stack_size(&self) -> usize {
        self.params.stack.clamp(1, Params::MAX_STACK).min(self.params.voices.clamp(1, VOICES))
    }

    fn faintest(&self, cap: usize, taken: &[usize]) -> usize {
        let motion = self.motion_now();
        let rank = |voice: &Voice| (standing(voice), self.loudness(voice, &motion), Reverse(voice.age));
        let faintest = self.voices[..cap].iter().enumerate().filter(|(index, _)| !taken.contains(index)).min_by(|(_, a), (_, b)| {
            let (a, b) = (rank(a), rank(b));
            a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2))
        });
        faintest.map_or(0, |(index, _)| index)
    }

    fn steal(&mut self, stolen: usize, cap: usize) -> usize {
        let spare = self.spare(cap);
        self.voices.swap(stolen, spare);
        self.voices[spare].force_fast = true;
        self.set_envelope_times(spare);
        let_go(&mut self.voices[spare]);
        stolen
    }

    fn spare(&mut self, cap: usize) -> usize {
        let spares = &self.voices[cap..];
        if let Some(free) = spares.iter().position(|voice| !voice.active) {
            return cap + free;
        }
        let motion = self.motion_now();
        let quietest = spares.iter().enumerate().min_by(|(_, a), (_, b)| self.loudness(a, &motion).total_cmp(&self.loudness(b, &motion))).map_or(cap, |(index, _)| cap + index);
        let voice = &mut self.voices[quietest];
        voice.active = false;
        voice.idle_at = Some(self.sample_clock);
        voice.env.rest();
        quietest
    }

    fn motion_now(&self) -> MotionBlock {
        MotionBlock::of(&self.params.motion, self.sample_clock, self.tempo, self.sample_rate)
    }

    fn loudness(&self, voice: &Voice, motion: &MotionBlock) -> f64 {
        let p = &self.params;
        let m = motion.peek(voice);
        let mut source = if p.noise.on { (p.noise.level + m.noise).max(0.0) } else { 0.0 };
        let mod_level = mod_levels(&p.modulation);
        for (k, osc) in p.oscs().iter().enumerate() {
            source = source.max(source_level(osc, mod_level[k], voice.modulation, m.level[k]));
        }
        let velocity = 1.0 + f64::from(self.vel_amp.z) * (voice.vel - 1.0);
        voice.env.value() * velocity * (1.0 + m.amp).max(0.0) * source
    }

    pub(super) fn set_envelope_times(&mut self, vi: usize) {
        let sr = self.sample_rate as f32;
        let amp = self.params.amp;
        let filter = self.params.filter.env;
        let scatter = ScatterPlan::of(&self.params.analog);
        let voice = &mut self.voices[vi];
        let midi = voice.midi;
        let mut amp_times = voice.scatter.amp_times(&scatter, amp.seconds(midi));
        if voice.force_fast {
            amp_times[RELEASE] = FAST_CUT_RELEASE;
        }
        voice.env.set_times(amp_times, amp.bends(), amp.sustain, sr);
        voice.filter_env.set_times(voice.scatter.filter_times(&scatter, filter.seconds(midi)), filter.bends(), filter.sustain, sr);
        for (env, mover) in voice.motion.envs.iter_mut().zip(self.params.motion.envs()) {
            env.set_times(mover.env.seconds(midi), mover.env.bends(), mover.env.sustain, sr);
        }
    }

    fn place_oscillators(&mut self, vi: usize, midi: f64, seed: (u8, i64), fresh: bool, all_copies: bool) {
        let oscs = self.params.oscs();
        let scatter = self.params.phase_random;
        let shifts = voice_shifts(&self.voices[vi], &self.params.modulation.tunes(), self.params.modulation.quantize != Quantize::Off);
        let transpose = self.voices[vi].transpose_now();
        let idle = self.voices[vi].idle_at.map(|idle| (self.sample_clock - idle).max(0));
        let song = self.tempo.song(self.sample_clock);
        for (k, osc) in oscs.iter().enumerate() {
            let hz = midi_to_hz(stepped(midi, osc.step_share, transpose)) / self.sample_rate;
            let ratio = tune_ratio(osc.tune, osc.fine);
            let base = hz * ratio;
            let voice = &mut self.voices[vi];
            if fresh {
                let carried = idle.filter(|_| osc.carry);
                let running = osc.free.then_some(hz * ratio * song as f64);
                place_copies(&mut voice.subs[k], &self.spread[k].detune, osc, scatter, carried, running, seed, k as u64 + 1, stack_turn(seed.0, seed.1, voice.stack_at, k as u64));
                voice.pulse_width = None;
            }
            let copies = if all_copies { MAX_UNISON } else { osc.unison.clamp(1, MAX_UNISON) };
            for u in 0..copies {
                voice.subs[k][u].inc = base * voice.drift.ratio[k][u] * shifts[k] * self.spread[k].detune[u];
            }
        }
    }

    fn start_voice(&mut self, midi: u8, velocity: u8, struck: Option<Articulation>, glide: Option<(u8, f64)>) {
        let frame = self.tempo.draw(self.sample_clock);
        if self.params.fresh {
            self.end_voices(midi);
        }
        let vel_norm = f64::from(velocity.max(1)) / 127.0;
        let (cents, modulation, transpose) = self.latched(struck);
        let (from, lagged) = self.take_modulation(modulation);
        let onset = Onset { midi, vel_norm, cents, modulation, transpose, from, lagged, glide, frame };
        let copies = self.stack_size();
        let (stack_cents_of, stack_spread_of) = (self.params.stack_cents, self.params.stack_spread);
        let mut taken = [usize::MAX; Params::MAX_STACK];
        for at in 0..copies {
            let vi = self.voice_index_for(midi, &taken[..at]);
            taken[at] = vi;
            let voice = &mut self.voices[vi];
            voice.stack_at = at;
            voice.stack_of = copies;
            voice.stack_cents = stack_cents(stack_cents_of, at, copies);
            voice.stack_pan = stack_pan(stack_spread_of, at, copies);
            self.start_one(vi, onset);
        }
    }

    fn start_one(&mut self, vi: usize, onset: Onset) {
        let Onset { midi, vel_norm, cents, modulation, transpose, from, lagged, glide, frame: seed_frame } = onset;
        let was_active = self.voices[vi].active;
        let continuing_same_pitch = was_active && self.voices[vi].env.phase != Phase::Idle && self.voices[vi].midi == midi;
        let (tunes, quantize) = (self.params.modulation.tunes(), self.params.modulation.quantize);
        let motion = self.params.motion;
        let sensed = f64::from(self.vel_filter.z);
        {
            let voice = &mut self.voices[vi];
            if was_active && voice.vel_target != vel_norm {
                voice.filter_env.rescale(restruck(sensed, voice.vel_target, vel_norm));
            }
            voice.active = true;
            voice.midi = midi;
            voice.glide_from = glide.map_or(f64::from(midi), |(from, _)| f64::from(from));
            voice.glide_midi = voice.glide_from;
            voice.gliding = glide.is_some();
            if !was_active {
                voice.vel = vel_norm;
            }
            voice.vel_target = vel_norm;
            voice.cents = cents;
            voice.transpose = transpose;
            voice.transpose_from = glide.map_or(transpose, |(_, from)| from);
            voice.modulation_target = modulation;
            if !lagged {
                voice.modulation = modulation;
                voice.mod_semitones = tunes.map(|tune| quantize.snap(tune * modulation));
            } else if !was_active {
                voice.modulation = from;
                voice.mod_semitones = tunes.map(|tune| quantize.snap(tune * from));
            }
            if !was_active {
                voice.cutoff_slew = 0.0;
                voice.mod_eased = voice.modulation;
            }
            voice.age = 0;
            voice.motion.start(&motion, midi, seed_frame);
        }
        self.analog_draw(vi, midi, seed_frame);
        let start = self.voices[vi].glide_midi;
        if !continuing_same_pitch {
            self.place_oscillators(vi, start, (midi, seed_frame), true, true);
        } else if glide.is_some() {
            self.place_oscillators(vi, start, (midi, seed_frame), false, true);
        }
        self.voices[vi].force_fast = false;
        self.set_envelope_times(vi);
        let voice = &mut self.voices[vi];
        voice.env.trigger();
        if !was_active {
            voice.reset_filters();
            voice.filter_env.rest();
            voice.motion.rest();
        }
        voice.filter_env.trigger();
        voice.motion.trigger();
    }

    fn mono_winner(&self) -> Held {
        self.mono.stack.last().copied().unwrap_or(Held { midi: 0, velocity: 127, cents: 0.0, modulation: 0.0, transpose: 0.0 })
    }

    fn mono_note_on(&mut self, midi: u8, velocity: u8, struck: Option<Articulation>) {
        let was_gated = self.mono.gated;
        let (cents, modulation, transpose) = self.latched(struck);
        self.mono.stack.retain(|held| held.midi != midi);
        self.mono.stack.push(Held { midi, velocity, cents, modulation, transpose });
        self.mono.gated = true;
        let winner = self.mono_winner();
        if was_gated && winner.midi == self.mono.target_midi && self.voices[0].sounding() {
            return;
        }
        let glide_on = self.params.mono.glide_ms > 0.0 && self.mono.has_reference;
        let do_glide = glide_on && (was_gated || !self.params.mono.legato);
        let do_retrigger = !was_gated || self.params.mono.retrigger;
        self.mono.target_midi = winner.midi;
        self.aim_mono(winner.midi, do_glide);
        self.fire_mono_voice(winner, do_retrigger);
        self.mono.has_reference = true;
    }

    fn aim_mono(&mut self, midi: u8, glide: bool) {
        let voice = &mut self.voices[0];
        if glide {
            voice.transpose_from = voice.transpose_now();
            voice.glide_from = voice.glide_midi;
        } else {
            voice.glide_midi = f64::from(midi);
        }
        voice.gliding = glide;
    }

    pub fn legato(&mut self, from: u8, to: u8, velocity: u8, glide: bool) {
        self.settle_mono();
        if self.params.voices != 1 || !self.mono.gated || !self.voices[0].sounding() {
            if from == to {
                self.rearticulate(to);
            } else if !self.retune(from, to, velocity, glide) {
                self.press(to, velocity, glide);
                self.note_off(from);
            }
            return;
        }
        let struck = self.struck.take();
        self.held.retain(|key| *key != from && *key != to);
        self.held.push(to);
        self.last_midi = Some(to);
        let (cents, modulation, transpose) = self.latched(struck);
        self.last_transpose = transpose;
        let held = Held { midi: to, velocity, cents, modulation, transpose };
        self.mono.stack.retain(|held| held.midi != from && held.midi != to);
        self.mono.stack.push(held);
        let do_glide = glide && self.params.mono.glide_ms > 0.0 && self.mono.has_reference;
        self.mono.target_midi = to;
        self.aim_mono(to, do_glide);
        self.fire_mono_voice(held, false);
    }

    fn retune(&mut self, from: u8, to: u8, velocity: u8, glide: bool) -> bool {
        if !self.voices.iter().any(|voice| voice.sounding() && voice.midi == from) {
            return false;
        }
        let struck = self.struck.take();
        let (cents, modulation, transpose) = self.latched(struck);
        self.held.retain(|key| *key != from && *key != to);
        self.held.push(to);
        self.last_midi = Some(to);
        self.last_transpose = transpose;
        let (_, lagged) = self.take_modulation(modulation);
        let (tunes, quantize) = (self.params.modulation.tunes(), self.params.modulation.quantize);
        let gliding = glide && self.params.mono.glide_ms > 0.0;
        let vel_norm = f64::from(velocity.max(1)) / 127.0;
        let sensed = f64::from(self.vel_filter.z);
        let seed_frame = self.tempo.draw(self.sample_clock);
        for vi in 0..self.voices.len() {
            let voice = &mut self.voices[vi];
            if !voice.sounding() || voice.midi != from {
                continue;
            }
            if voice.vel_target != vel_norm {
                voice.filter_env.rescale(restruck(sensed, voice.vel_target, vel_norm));
                voice.vel_target = vel_norm;
            }
            if gliding {
                voice.transpose_from = voice.transpose_now();
                voice.glide_from = voice.glide_midi;
            } else {
                voice.glide_midi = f64::from(to);
            }
            voice.gliding = gliding;
            voice.midi = to;
            voice.cents = cents;
            voice.transpose = transpose;
            voice.modulation_target = modulation;
            if !lagged {
                voice.modulation = modulation;
                voice.mod_semitones = tunes.map(|tune| quantize.snap(tune * modulation));
            }
            let glide_midi = voice.glide_midi;
            self.place_oscillators(vi, glide_midi, (to, seed_frame), false, true);
        }
        true
    }

    fn rearticulate(&mut self, midi: u8) {
        let struck = self.struck.take();
        let (cents, modulation, transpose) = self.latched(struck);
        self.last_transpose = transpose;
        let (_, lagged) = self.take_modulation(modulation);
        let (tunes, quantize) = (self.params.modulation.tunes(), self.params.modulation.quantize);
        let seed_frame = self.tempo.draw(self.sample_clock);
        for vi in 0..self.voices.len() {
            let voice = &mut self.voices[vi];
            if !voice.sounding() || voice.midi != midi {
                continue;
            }
            voice.cents = cents;
            voice.transpose = transpose;
            voice.modulation_target = modulation;
            if !lagged {
                voice.modulation = modulation;
                voice.mod_semitones = tunes.map(|tune| quantize.snap(tune * modulation));
            }
            let glide_midi = voice.glide_midi;
            self.place_oscillators(vi, glide_midi, (midi, seed_frame), false, true);
        }
    }

    fn mono_note_off(&mut self, midi: u8) {
        self.mono.stack.retain(|held| held.midi != midi);
        self.mono.stale = true;
    }

    pub(super) fn settle_mono(&mut self) {
        if !self.mono.stale {
            return;
        }
        self.mono.stale = false;
        if self.mono.stack.is_empty() {
            self.mono.gated = false;
            for voice in &mut self.voices {
                if voice.sounding() {
                    let_go(voice);
                }
            }
            return;
        }
        let winner = self.mono_winner();
        if winner.midi == self.mono.target_midi {
            return;
        }
        self.mono.target_midi = winner.midi;
        let do_glide = self.params.mono.glide_ms > 0.0 && self.mono.has_reference;
        self.aim_mono(winner.midi, do_glide);
        self.fire_mono_voice(winner, false);
    }

    fn fade_mono_voice(&mut self) {
        let voice = &self.voices[0];
        let (glide_from, glide_midi, gliding, transpose_from) = (voice.glide_from, voice.glide_midi, voice.gliding, voice.transpose_from);
        self.steal(0, 1);
        let voice = &mut self.voices[0];
        voice.glide_from = glide_from;
        voice.glide_midi = glide_midi;
        voice.gliding = gliding;
        voice.transpose_from = transpose_from;
    }

    fn fire_mono_voice(&mut self, held: Held, retrigger: bool) {
        let Held { midi, velocity, cents, modulation, transpose } = held;
        let seed_frame = self.tempo.draw(self.sample_clock);
        if self.voices[0].active && (retrigger && self.params.fresh || !self.voices[0].sounding()) {
            self.fade_mono_voice();
        }
        let fresh = !self.voices[0].active;
        let vel_norm = f64::from(velocity.max(1)) / 127.0;
        let (from, lagged) = self.take_modulation(modulation);
        let (tunes, quantize) = (self.params.modulation.tunes(), self.params.modulation.quantize);
        let motion = self.params.motion;
        let sensed = f64::from(self.vel_filter.z);
        {
            let voice = &mut self.voices[0];
            if !fresh && voice.vel_target != vel_norm {
                voice.filter_env.rescale(restruck(sensed, voice.vel_target, vel_norm));
            }
            voice.active = true;
            voice.midi = midi;
            voice.stack_at = 0;
            voice.stack_of = 1;
            if fresh {
                voice.vel = vel_norm;
                voice.cutoff_slew = 0.0;
            }
            voice.vel_target = vel_norm;
            voice.cents = cents;
            voice.transpose = transpose;
            voice.modulation_target = modulation;
            if !lagged {
                voice.modulation = modulation;
                voice.mod_semitones = tunes.map(|tune| quantize.snap(tune * modulation));
            } else if fresh {
                voice.modulation = from;
                voice.mod_semitones = tunes.map(|tune| quantize.snap(tune * from));
            }
            if fresh {
                voice.mod_eased = voice.modulation;
            }
        }
        if fresh || retrigger {
            self.analog_draw(0, midi, seed_frame);
        } else {
            self.refresh_drift(0);
        }
        let glide_midi = self.voices[0].glide_midi;
        self.place_oscillators(0, glide_midi, (midi, seed_frame), fresh, false);
        if fresh {
            let voice = &mut self.voices[0];
            voice.reset_filters();
            voice.filter_env.rest();
            voice.motion.rest();
        }
        self.voices[0].force_fast = false;
        self.set_envelope_times(0);
        if fresh || retrigger {
            let voice = &mut self.voices[0];
            voice.env.trigger();
            voice.filter_env.trigger();
            voice.age = 0;
            voice.motion.start(&motion, midi, seed_frame);
            voice.motion.trigger();
        }
    }

    fn analog_draw(&mut self, vi: usize, midi: u8, seed_frame: i64) {
        let shift = drift::lane_shift(self.lane);
        let seed_frame = seed_frame.wrapping_add(shift);
        let voice = &mut self.voices[vi];
        let at = voice.stack_at;
        voice.drift.draw((vi as i64).wrapping_add(shift), midi, seed_frame, at, &self.tempo, self.sample_clock);
        voice.scatter.draw(midi, seed_frame, at);
        self.refresh_drift(vi);
    }

    pub(super) fn refresh_drift(&mut self, vi: usize) {
        let plan = DriftPlan::of(&self.params.analog);
        let unison = self.params.oscs().map(|osc| osc.unison.clamp(1, MAX_UNISON));
        self.voices[vi].drift.refresh(&plan, unison, &self.tempo, self.sample_clock, self.sample_rate);
    }
}

fn standing(voice: &Voice) -> bool {
    !matches!(voice.env.phase, Phase::Idle | Phase::Release)
}

fn place_copies(subs: &mut [SubOsc; MAX_UNISON], detune: &[f64; MAX_UNISON], osc: &Osc, scatter: f64, carried: Option<i64>, running: Option<f64>, note: (u8, i64), index: u64, turn: f64) {
    let (midi, frame) = note;
    for (copy, (sub, d)) in subs.iter_mut().zip(detune).enumerate() {
        sub.phase = match (carried, running) {
            (Some(frames), _) => (sub.phase + sub.inc * frames as f64).fract(),
            (None, Some(turns)) => (free_copy_offset(index, copy) + turn + turns * d) % 1.0,
            (None, None) if copy > 0 => (osc.phase + turn + scatter * copy_phase(midi, frame, index, copy)) % 1.0,
            (None, None) if turn > 0.0 => (osc.phase + turn) % 1.0,
            (None, None) => osc.phase,
        };
        sub.steps.clear();
    }
}

fn restruck(sensed: f64, before: f64, after: f64) -> f64 {
    (1.0 + sensed * (before - 1.0)) / (1.0 + sensed * (after - 1.0))
}

fn copy_phase(midi: u8, frame: i64, osc: u64, copy: usize) -> f64 {
    note_seed01(midi, frame, COPY_SALT + osc * MAX_UNISON as u64 + copy as u64)
}

fn free_copy_offset(osc: u64, copy: usize) -> f64 {
    if copy == 0 { 0.0 } else { note_seed01(0, 0, FREE_COPY_SALT + osc * MAX_UNISON as u64 + copy as u64) }
}

fn source_level(osc: &Osc, mod_level: f64, modulation: f64, swing: f64) -> f64 {
    if !osc.sounding() {
        return 0.0;
    }
    (osc.level + mod_level * modulation + swing).clamp(0.0, 1.0)
}

fn let_go(voice: &mut Voice) {
    voice.env.release();
    voice.filter_env.release();
    voice.motion.release();
}

impl Sink for Core {
    fn note_on(&mut self, midi: u8, velocity: u8) {
        Core::note_on(self, midi, velocity);
    }

    fn note_off(&mut self, midi: u8) {
        Core::note_off(self, midi);
    }

    fn strike(&mut self, midi: u8, velocity: u8, articulation: Articulation) {
        self.struck = Some(articulation);
        Core::note_on(self, midi, velocity);
    }

    fn legato(&mut self, from: u8, to: u8, velocity: u8, articulation: Articulation) {
        self.struck = Some(articulation);
        Core::legato(self, from, to, velocity, articulation.join == Join::Slide);
    }
}
