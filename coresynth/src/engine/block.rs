use super::*;
use sdk::dsp::unison::MAX_UNISON;
use std::f64::consts::PI;

fn slew_alpha(ms: f64, sample_rate: f64) -> Option<f64> {
    (ms > 0.0).then(|| 1.0 - (-1.0 / (ms / 1000.0 * sample_rate)).exp())
}

impl Core {
    pub(super) fn prepare(&mut self, n: usize) -> Block {
        let p = self.params;
        let oscs = p.oscs();
        let shapes: [[Wave; WAVES]; OSCS] = std::array::from_fn(|k| oscs[k].shapes);
        let levels: [[f64; WAVES]; OSCS] = std::array::from_fn(|k| oscs[k].levels);
        let comp = levels.map(mix_compensation);
        let modulation = p.modulation;
        let mod_level = mod_levels(&modulation);
        let mod_tune = modulation.tunes();
        let pure: [bool; OSCS] = std::array::from_fn(|k| oscs[k].pure());
        let fm_on: [bool; OSCS] = std::array::from_fn(|k| oscs[k].fm > 0.0 || p.motion.moves(Target::fm_of(k)));
        let noise = p.noise;
        if noise.on && !self.last_noise_on {
            self.noise_smooth = NoiseSmooth { level: noise.level, tone: noise.tone, width: noise.width, high_pass_hz: noise.high_pass_hz, low_pass_hz: noise.low_pass_hz };
        }
        self.last_noise_on = noise.on;
        if noise.on {
            let a = 1.0 - (-(n as f64) / (NOISE_SMOOTH_SECONDS * self.sample_rate)).exp();
            let s = &mut self.noise_smooth;
            s.level += a * (noise.level - s.level);
            s.tone += a * (noise.tone - s.tone);
            s.width += a * (noise.width - s.width);
            s.high_pass_hz += a * (noise.high_pass_hz - s.high_pass_hz);
            s.low_pass_hz += a * (noise.low_pass_hz - s.low_pass_hz);
        }
        let noise_audible = noise.on && (noise.level > 0.0 || self.noise_smooth.level > 1e-4 || p.motion.moves(Target::Noise));
        let drift = DriftPlan::of(&p.analog);
        self.respread();
        self.retrace();
        let unison: [usize; OSCS] = std::array::from_fn(|k| oscs[k].unison.clamp(1, MAX_UNISON));
        self.settle_mono();
        if p.voices != 1 && (self.mono.gated || self.mono.has_reference || !self.mono.stack.is_empty()) {
            self.mono.clear();
        }
        let glide = Glide::of(&p.mono, self.sample_rate);
        let filter = p.filter;
        if filter.on && (!self.last_filter_on || filter.mode != self.last_filter_mode) {
            for voice in self.voices.iter_mut().filter(|voice| voice.active) {
                voice.reset_filters();
            }
        }
        self.last_filter_on = filter.on;
        self.last_filter_mode = filter.mode;
        let audible: [bool; OSCS] = std::array::from_fn(|k| comp[k] > 0.0 && (oscs[k].level > 0.0 || mod_level[k] != 0.0 || p.motion.moves(level_target(k))));
        let source_audible = audible.contains(&true) || noise_audible;
        let filter_on = filter.on && source_audible;
        let filter_fm_on = filter_on && filter.fm != 0.0;
        let mo = if p.modulator == Modulator::Osc3 { 2 } else { 1 };
        let demand: [bool; OSCS] = std::array::from_fn(|k| k != mo && (fm_on[k] || oscs[k].sync));
        let modulator = (demand.contains(&true) || filter_fm_on).then_some(mo);
        let carrier: [bool; OSCS] = std::array::from_fn(|k| modulator.is_some() && demand[k]);
        let sync: [bool; OSCS] = std::array::from_fn(|k| carrier[k] && oscs[k].sync);
        let edged: [bool; OSCS] = std::array::from_fn(|k| !(carrier[k] && fm_on[k] && !oscs[k].linear_fm));
        let edge: [Edge; OSCS] = std::array::from_fn(|k| Edge::of(oscs[k].bright));
        let rings: [bool; OSCS] = std::array::from_fn(|k| edged[k] || sync[k]);
        let nyquist_guard = self.sample_rate * 0.45;
        let filter_over = if matches!(filter.mode, FilterMode::Ladder | FilterMode::Svf) { filter.oversample } else { 1 };
        let tune_ratio: [f64; OSCS] = std::array::from_fn(|k| tune_ratio(oscs[k].tune, oscs[k].fine));
        let noise_smooth = self.noise_smooth;
        let sample_rate = self.sample_rate;
        let quantized = modulation.quantize != Quantize::Off;
        for vi in 0..self.voices.len() {
            if !self.voices[vi].active {
                continue;
            }
            let midi = self.voices[vi].midi;
            if drift.on {
                self.refresh_drift(vi);
            }
            let voice = &mut self.voices[vi];
            if quantized && voice.modulation == voice.modulation_target {
                voice.mod_semitones = mod_tune.map(|tune| modulation.quantize.snap(tune * voice.modulation));
            }
            voice.stack_cents = stack_cents(p.stack_cents, voice.stack_at, voice.stack_of);
            voice.stack_pan = stack_pan(p.stack_spread, voice.stack_at, voice.stack_of);
            let shifts = voice_shifts(voice, &mod_tune, quantized);
            for k in 0..OSCS {
                let hz = midi_to_hz(stepped(f64::from(midi), oscs[k].step_share, voice.transpose)) / sample_rate;
                let base = hz * tune_ratio[k];
                for u in 0..unison[k] {
                    let base_inc = if drift.on { base * voice.drift.ratio[k][u] } else { base } * shifts[k];
                    voice.subs[k][u].inc = base_inc * self.spread[k].detune[u];
                }
            }
            self.set_envelope_times(vi);
            if noise.on {
                let nv = &mut self.voices[vi].noise.noise;
                nv.set_sample_rate(sample_rate);
                nv.set_color(noise.color);
                nv.set_tone_norm(noise_smooth.tone);
                nv.set_width(noise_smooth.width);
                nv.flush_denormals();
            }
        }
        let noise = NoiseBlock {
            on: noise.on,
            level: if noise.on { noise_smooth.level } else { 0.0 },
            high_g: (noise.on && noise.high_pass_hz > Filter::MIN_HZ).then(|| corner(noise_smooth.high_pass_hz, nyquist_guard, sample_rate)),
            low_g: (noise.on && noise.low_pass_hz < Filter::MAX_HZ).then(|| corner(noise_smooth.low_pass_hz, nyquist_guard, sample_rate)),
        };
        let mut fed_back = Memo::default();
        for i in 0..n {
            self.curves.vel_amp[i] = f64::from(self.vel_amp.tick(p.amp.velocity as f32));
            self.curves.vel_filter[i] = f64::from(self.vel_filter.tick(filter.env.velocity as f32));
            if filter_on {
                self.curves.cutoff[i] = f64::from(self.cutoff.tick(filter.cutoff_hz as f32));
                let resonance = f64::from(self.resonance.tick(filter.resonance as f32));
                self.curves.resonance[i] = resonance;
                self.curves.k[i] = fed_back.of(resonance, |resonance| filters::feedback(filter.mode, resonance));
                self.curves.drive[i] = f64::from(self.drive.tick(filter.drive as f32));
                self.curves.back[i] = f64::from(self.back.tick(filter.feedback as f32));
            }
        }
        let clock = self.sample_clock;
        let mut bent = [false; OSCS];
        for (osc, slot) in self.bends.iter_mut().enumerate() {
            if let Some(bend) = slot {
                let mut lifted = Memo::default();
                for (i, ratio) in self.curves.bends[osc][..n].iter_mut().enumerate() {
                    *ratio = lifted.of(bend.at(clock + i as i64), |semitones| (semitones / 12.0).exp2());
                }
                bent[osc] = true;
                if bend.spent(clock + n as i64) {
                    *slot = None;
                }
            }
        }
        let motion = MotionBlock::of(&p.motion, self.sample_clock, self.tempo, sample_rate);
        if self.voices.iter().any(|voice| voice.active) {
            motion.trace(&mut self.curves.runs, n);
        }
        let lifts: [f64; OSCS] = std::array::from_fn(|k| motion.lift(level_target(k)) + mod_level[k].max(0.0));
        let sources = heat::Sources { oscs: &oscs, spread: &self.spread, cycles: &self.cycles, noise: &p.noise, lifts, noise_lift: motion.lift(Target::Noise) };
        Block {
            glide,
            tune_ratio,
            step_share: oscs.map(|osc| osc.step_share),
            unison,
            noise,
            filter_on,
            filter_mode: filter.mode,
            filter_slope: filter.slope,
            filter_shape: filter.shape,
            filter_comb_read: filter.comb_read,
            filter_env_octaves: filter.env_amount * 5.0,
            filter_key_track: filter.key_track,
            resonance_velocity: filter.resonance_velocity,
            filter_fm_octaves: if filter_fm_on { filter.fm * MOD_CUTOFF_OCTAVES } else { 0.0 },
            cutoff_slew: slew_alpha(filter.mod_slew_ms, sample_rate),
            drift,
            scatter: ScatterPlan::of(&p.analog),
            heat: self.curves.heat.plan(&p.analog, &sources, n),
            fm_turns: oscs.map(|osc| osc.fm * FM_MAX_RADIANS / (2.0 * PI)),
            fm_linear: oscs.map(|osc| osc.linear_fm),
            sync,
            sync_phase: oscs.map(|osc| osc.sync_phase),
            sync_any: sync.contains(&true),
            sync_pole: (-2.0 * PI * SYNC_DC_HZ / self.sample_rate).exp(),
            modulator,
            carrier,
            levels,
            shapes,
            stepped: std::array::from_fn(|k| Stepped::of(&shapes[k], &levels[k])),
            edge,
            edged,
            rings,
            bent,
            comp,
            level: oscs.map(|osc| osc.level),
            pure,
            sign: oscs.map(|osc| if osc.invert { -1.0 } else { 1.0 }),
            pulse_width: std::array::from_fn(|k| if pure[k] { oscs[k].pulse_width.clamp(0.05, 0.95) } else { 0.5 }),
            audible,
            source_audible,
            filter_over,
            filter_guard: nyquist_guard * filter_over as f64,
            mod_level,
            mod_tune,
            mod_tuned: mod_tune.iter().any(|tune| *tune != 0.0),
            mod_quantize: (modulation.quantize != Quantize::Off && mod_tune.iter().any(|tune| *tune != 0.0)).then_some(modulation.quantize),
            mod_lag_alpha: (modulation.lag_ms > 0.0).then(|| 1.0 - (-GLIDE_TIME_CONSTANTS / (modulation.lag_ms / 1000.0 * sample_rate)).exp()),
            mod_reach: Reach::of(&modulation),
            filter_env_holds: filter.env_holds && filter_on,
            motion,
        }
    }
}
