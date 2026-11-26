use sdk::dsp::filter::{Coeffs, Path, State, Tuned};
use sdk::dsp::svf::Svf;
use crate::lfo_shape::{Join, begun, seconds, turns};
use crate::{Clock, Unit};
use sdk::dsp::{Mix, Ring, WIDTH_UNITY_EPS, db_to_gain, finite, lp_coeff, one_pole_lp, sanitize, sine, smoothing, triangle, widen};
use crate::params::{self, ChorusMode, Filter, Slope};
use std::convert::identity;
use std::f64::consts::{PI, SQRT_2};

const RING_SECONDS: f64 = 0.075;
const RING_PAD: usize = 8;
const RING_MIN: usize = 256;
const MIN_READ_DELAY_SAMPLES: f64 = 1.5;
const DC_TICKLE: f32 = 1e-15;
const SMOOTH_SECONDS: f64 = 0.030;
const MODE_GAIN_SECONDS: f64 = 0.005;
const NYQUIST_GUARD: f64 = 0.45;
const RECON_HZ: f64 = 6600.0;
const FILTERS: usize = params::Chorus::FILTERS;
const WET_CUT: usize = 0;
const LINE_CUT: usize = 1;
const LOOP_CEILING: f32 = 0.5;
const BBD_COMPRESS_EPS: f32 = 1e-20;
const ENSEMBLE_TAP_OFFSET_MS: f64 = 3.0;
const ENSEMBLE_TRI_WEIGHT: f32 = 0.7;
const ENSEMBLE_SINE_WEIGHT: f32 = 0.3;
const SPATIAL_CENTER_MS: f64 = 10.0;
const SPATIAL_MIN_READ_MS: f64 = 5.0;
const SPATIAL_HP_HZ: f64 = 80.0;
const TAPS: usize = 3;
const DUAL_FAR: f64 = 1.5;
const DUAL_LAG: f64 = 0.13;
const DUAL_GAIN: f32 = 0.74;
const CLASSIC_LOG_SWING: f64 = 0.7;
const FAR_FLOOR: f64 = 0.5 * f64::EPSILON;

impl ChorusMode {
    fn feedback_max(self) -> f32 {
        match self {
            ChorusMode::Standard | ChorusMode::Vintage | ChorusMode::Classic => 0.30,
            ChorusMode::Ensemble => 0.15,
            ChorusMode::Spatial => 0.0,
        }
    }

    fn wet_scale(self) -> f32 {
        match self {
            ChorusMode::Ensemble => 1.2,
            ChorusMode::Spatial => 1.1,
            ChorusMode::Standard | ChorusMode::Vintage | ChorusMode::Classic => 0.85,
        }
    }

    fn excursion_ms(self) -> f64 {
        match self {
            ChorusMode::Ensemble => 6.0,
            ChorusMode::Spatial => 2.5,
            ChorusMode::Standard | ChorusMode::Vintage | ChorusMode::Classic => 5.0,
        }
    }

    fn fast_rate(self, rate: f64) -> f64 {
        match self {
            ChorusMode::Ensemble => (rate * 10.0).max(6.0),
            _ => rate * 1.37,
        }
    }
}

fn samples(ms: f64, sample_rate: f64) -> f64 {
    ms * 0.001 * sample_rate
}

fn frac(x: f64) -> f64 {
    x - x.floor()
}

fn tri(phase: f64) -> f32 {
    triangle(phase) as f32
}

fn ensemble(slow: f64, fast: f64) -> f32 {
    ENSEMBLE_TRI_WEIGHT * tri(slow) + ENSEMBLE_SINE_WEIGHT * sine(fast) as f32
}

fn log_swing(depth: f32, phase: f64) -> f64 {
    CLASSIC_LOG_SWING * f64::from(depth) * sine(phase)
}

fn dual_pair(cl: &mut Channel, cr: &mut Channel, tap: usize, center: f64, excursion: f64, slow: (f64, f64), longest: f64, shape: fn(f32) -> f32) -> (f32, f32) {
    let (slow_l, slow_r) = slow;
    (cl.read_dual(tap, center, excursion, slow_l, slow_r + DUAL_LAG, longest, shape), cr.read_dual(tap, center, excursion, slow_r, slow_l + DUAL_LAG, longest, shape))
}

fn peak_gain(filter: &Filter) -> f32 {
    if !filter.rings() || filter.q <= Filter::FLAT_Q {
        return 1.0;
    }
    let peak = 2.0 * filter.q * filter.q / (4.0 * filter.q * filter.q - 1.0).sqrt();
    (if filter.slope == Slope::TwentyFour { peak * peak } else { peak }) as f32
}

fn bbd_compress(x: f32) -> f32 {
    (x.abs() + BBD_COMPRESS_EPS).sqrt().copysign(x)
}

fn bbd_expand(y: f32) -> f32 {
    y * y.abs()
}

#[derive(Clone, Copy)]
struct Knobs {
    rate: f64,
    depth: f32,
    center: f64,
    far: f64,
    feedback: f32,
    filters: [Coeffs; FILTERS],
    width: f32,
    trim: f32,
}

impl Knobs {
    fn target(params: &params::Chorus, rate: f64, filters: [Coeffs; FILTERS], sample_rate: f64) -> Knobs {
        let feedback = params.mode.feedback_max() * (params.feedback as f32).tanh();
        Knobs {
            rate,
            depth: params.depth as f32,
            center: samples(params.center_ms, sample_rate),
            far: samples(params.far_center_ms, sample_rate),
            feedback: feedback.min(LOOP_CEILING / peak_gain(&params.filter2)),
            filters,
            width: params.width as f32,
            trim: db_to_gain(params.trim_db) as f32,
        }
    }

    fn glide(&mut self, target: &Knobs, coeff: f64) {
        let c = coeff as f32;
        self.rate += coeff * (target.rate - self.rate);
        self.depth += c * (target.depth - self.depth);
        self.center += coeff * (target.center - self.center);
        self.far += coeff * (target.far - self.far);
        if target.far == 0.0 && self.far < FAR_FLOOR {
            self.far = 0.0;
        }
        self.feedback += c * (target.feedback - self.feedback);
        for (mine, theirs) in self.filters.iter_mut().zip(&target.filters) {
            mine.approach(theirs, c);
        }
        self.width += c * (target.width - self.width);
        self.trim += c * (target.trim - self.trim);
    }
}

struct Channel {
    ring: Ring,
    ap: [f32; 2 * TAPS],
    lows: f32,
    recon: Svf,
    cuts: [State; FILTERS],
}

impl Channel {
    fn new(samples: usize) -> Channel {
        Channel { ring: Ring::new(samples), ap: [0.0; 2 * TAPS], lows: 0.0, recon: Svf::default(), cuts: [State::default(); FILTERS] }
    }

    fn clear(&mut self) {
        self.ring.clear();
        self.ap = [0.0; 2 * TAPS];
        self.lows = 0.0;
        self.recon.reset();
        self.cuts = [State::default(); FILTERS];
    }

    fn read(&mut self, tap: usize, delay: f64) -> f32 {
        let d = delay.max(MIN_READ_DELAY_SAMPLES);
        let m = (d - 0.5) as usize;
        let delta = (d - m as f64) as f32;
        let eta = (1.0 - delta) / (1.0 + delta);
        let y = eta * (self.ring.tap(m) - self.ap[tap]) + self.ring.tap(m + 1);
        self.ap[tap] = y;
        y
    }

    fn read_swept(&mut self, tap: usize, center: f64, excursion: f64, lfo: f32, shape: fn(f32) -> f32) -> f32 {
        shape(self.read(tap, center + excursion * f64::from(lfo)))
    }

    fn read_two(&mut self, tap: usize, center: f64, far: f64, excursion: f64, near_lfo: f32, far_lfo: f32, longest: f64, shape: fn(f32) -> f32) -> f32 {
        let near = shape(self.read(tap, center + excursion * f64::from(near_lfo)));
        let far = shape(self.read(tap + TAPS, (far + excursion * f64::from(far_lfo)).min(longest)));
        DUAL_GAIN * (near + far)
    }

    fn read_dual(&mut self, tap: usize, center: f64, excursion: f64, lower: f64, upper: f64, longest: f64, shape: fn(f32) -> f32) -> f32 {
        let near = shape(self.read(tap, center + excursion * sine(lower)));
        let far = shape(self.read(tap + TAPS, (DUAL_FAR * center + excursion * sine(upper)).min(longest)));
        DUAL_GAIN * (near + far)
    }

    fn read_log(&mut self, tap: usize, center: f64, swing: f64, longest: f64) -> f32 {
        self.read(tap, (center * swing.exp()).min(longest))
    }

    fn write(&mut self, x: f32, path: Path, coeffs: &Coeffs, bbd: bool, tickle: f32) {
        let toned = finite(self.cuts[LINE_CUT].tick(finite(x), path, coeffs));
        let stored = if bbd { bbd_compress(toned) } else { toned };
        self.ring.push(stored + tickle);
    }

    fn reconstruct(&mut self, x: f32, g: f64) -> f32 {
        self.recon.tick(f64::from(x), g, SQRT_2).lp as f32
    }

    fn high_passed(&mut self, x: f32, coeff: f32) -> f32 {
        self.lows = one_pole_lp(x, coeff, self.lows);
        x - self.lows
    }

    fn cut_wet(&mut self, x: f32, path: Path, coeffs: &Coeffs) -> f32 {
        self.cuts[WET_CUT].tick(x, path, coeffs)
    }
}

pub struct Chorus {
    sample_rate: f64,
    recon_g: f64,
    l: Channel,
    r: Channel,
    mode: Option<ChorusMode>,
    knobs: Option<Knobs>,
    filters: [Tuned; FILTERS],
    phases: Option<(f64, f64)>,
    joins: [Join; 2],
    joining: bool,
    mode_gain: f32,
    mix: Mix,
    tickle: f32,
}

impl Chorus {
    pub(super) fn clear(&mut self) {
        self.l.clear();
        self.r.clear();
        self.mode = None;
        self.knobs = None;
        self.phases = None;
        self.joins = [Join::default(); 2];
        self.mode_gain = 1.0;
        self.mix = Mix::new(self.sample_rate);
        self.tickle = DC_TICKLE;
    }
}

impl Unit for Chorus {
    type Params = params::Chorus;

    fn new(sample_rate: f64, _: usize) -> Chorus {
        let ring = ((RING_SECONDS * sample_rate).ceil() as usize + RING_PAD).max(RING_MIN);
        Chorus {
            sample_rate,
            recon_g: (PI * RECON_HZ.min(sample_rate * NYQUIST_GUARD) / sample_rate).tan(),
            l: Channel::new(ring),
            r: Channel::new(ring),
            mode: None,
            knobs: None,
            filters: params::Chorus::DEFAULT.filters().map(|filter| Tuned::of(&filter, sample_rate)),
            phases: None,
            joins: [Join::default(); 2],
            joining: false,
            mode_gain: 1.0,
            mix: Mix::new(sample_rate),
            tickle: DC_TICKLE,
        }
    }

    fn restart(&mut self) {
        self.joining = true;
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], params: &params::Chorus, wet: f32, clock: &Clock) {
        let n = l.len().min(r.len());
        sanitize(l, r);
        let sr = self.sample_rate;
        let mode = params.mode;
        if matches!(self.mode, Some(was) if was != mode) {
            self.mode_gain = 0.0;
            self.l.clear();
            self.r.clear();
        }
        self.mode = Some(mode);

        for (tuned, filter) in self.filters.iter_mut().zip(params.filters()) {
            tuned.tune(&filter, sr);
        }
        let paths = self.filters.map(|tuned| tuned.path);
        let target = Knobs::target(params, clock.rate(params.sync, params.division, params.rate_hz), self.filters.map(|tuned| tuned.coeffs), sr);
        let mut knobs = self.knobs.unwrap_or(target);
        let (mut phase_slow, mut phase_fast) = match self.phases {
            Some(phases) if !self.joining => phases,
            was => {
                let now = (begun(params.start_phase, turns(clock, params.sync, params.division, params.rate_hz)).1, begun(params.start_phase, seconds(clock) * mode.fast_rate(target.rate)).1);
                if let Some(was) = was {
                    self.joins[0].begin(was.0, now.0, sr);
                    self.joins[1].begin(was.1, now.1, sr);
                }
                now
            }
        };
        self.joining = false;
        let mut mode_gain = self.mode_gain;
        let mut tickle = self.tickle;
        let recon_g = self.recon_g;
        let (cl, cr) = (&mut self.l, &mut self.r);

        let spread = params.spread;
        let dual = params.dual;
        let far_phase = params.far_phase;
        let bbd = mode == ChorusMode::Vintage;
        let wet_scale = if params.crossfade { 1.0 } else { mode.wet_scale() };
        let glide = smoothing(SMOOTH_SECONDS, sr);
        let mode_gain_coeff = smoothing(MODE_GAIN_SECONDS, sr) as f32;
        let max_excursion = samples(mode.excursion_ms(), sr);
        let tap_offset = samples(ENSEMBLE_TAP_OFFSET_MS, sr);
        let spatial_center = samples(SPATIAL_CENTER_MS, sr);
        let spatial_coeff = lp_coeff(SPATIAL_HP_HZ, sr);
        let longest = RING_SECONDS * sr;
        let (tap_below, min_read) = match mode {
            ChorusMode::Ensemble => (tap_offset, MIN_READ_DELAY_SAMPLES),
            ChorusMode::Spatial => (0.0, samples(SPATIAL_MIN_READ_MS, sr)),
            ChorusMode::Standard | ChorusMode::Vintage | ChorusMode::Classic => (0.0, MIN_READ_DELAY_SAMPLES),
        };

        for i in 0..n {
            tickle = -tickle;
            knobs.glide(&target, glide);
            mode_gain += mode_gain_coeff * (1.0 - mode_gain);

            let base_center = if mode == ChorusMode::Spatial { spatial_center } else { knobs.center };
            let nearest = base_center - tap_below;
            let available = (nearest - min_read).max(0.0);
            let excursion = (f64::from(knobs.depth) * max_excursion).min(available);

            let (slow_l, fast_l) = (self.joins[0].swept(phase_slow), self.joins[1].swept(phase_fast));
            let slow_r = frac(slow_l + 0.5 * spread);
            let fast_r = frac(fast_l + 0.5 * spread);
            phase_slow = frac(phase_slow + knobs.rate / sr);
            phase_fast = frac(phase_fast + mode.fast_rate(knobs.rate) / sr);

            let (mut wet_l, mut wet_r, fb_l, fb_r) = match mode {
                ChorusMode::Standard => {
                    let (t_l, t_r) = if knobs.far > 0.0 {
                        (cl.read_two(0, knobs.center, knobs.far, excursion, tri(slow_l), tri(slow_l + far_phase), longest, identity), cr.read_two(0, knobs.center, knobs.far, excursion, tri(slow_r), tri(slow_r + far_phase), longest, identity))
                    } else if dual {
                        dual_pair(cl, cr, 0, knobs.center, excursion, (slow_l, slow_r), longest, identity)
                    } else {
                        (cl.read_swept(0, knobs.center, excursion, tri(slow_l), identity), cr.read_swept(0, knobs.center, excursion, tri(slow_r), identity))
                    };
                    (t_l, t_r, t_l, t_r)
                }
                ChorusMode::Vintage => {
                    let (t_l, t_r) = if knobs.far > 0.0 {
                        (cl.read_two(0, knobs.center, knobs.far, excursion, tri(slow_l), tri(slow_l + far_phase), longest, bbd_expand), cr.read_two(0, knobs.center, knobs.far, excursion, tri(slow_r), tri(slow_r + far_phase), longest, bbd_expand))
                    } else if dual {
                        dual_pair(cl, cr, 0, knobs.center, excursion, (slow_l, slow_r), longest, bbd_expand)
                    } else {
                        (cl.read_swept(0, knobs.center, excursion, tri(slow_l), bbd_expand), cr.read_swept(0, knobs.center, excursion, tri(slow_r), bbd_expand))
                    };
                    (cl.reconstruct(t_l, recon_g), cr.reconstruct(t_r, recon_g), t_l, t_r)
                }
                ChorusMode::Ensemble => {
                    let (mut sum_l, mut sum_r) = (0.0, 0.0);
                    let (mut first_l, mut first_r) = (0.0, 0.0);
                    for tap in 0..TAPS {
                        let turn = tap as f64 / 3.0;
                        let center = knobs.center + (tap as f64 - 1.0) * tap_offset;
                        let (t_l, t_r) = if dual { dual_pair(cl, cr, tap, center, excursion, (slow_l + turn, slow_r + turn), longest, identity) } else { (cl.read_swept(tap, center, excursion, ensemble(slow_l + turn, fast_l + turn), identity), cr.read_swept(tap, center, excursion, ensemble(slow_r + turn, fast_r + turn), identity)) };
                        if tap == 0 {
                            (first_l, first_r) = (t_l, t_r);
                        }
                        sum_l += t_l;
                        sum_r += t_r;
                    }
                    (sum_l * (1.0 / 3.0), sum_r * (1.0 / 3.0), first_l, first_r)
                }
                ChorusMode::Spatial => {
                    let lfo = tri(slow_l);
                    let (t_l, t_r) = if dual { dual_pair(cl, cr, 0, spatial_center, excursion, (slow_l + 0.5, slow_l), longest, identity) } else { (cl.read_swept(0, spatial_center, excursion, -lfo, identity), cr.read_swept(0, spatial_center, excursion, lfo, identity)) };
                    let h_l = cl.high_passed(t_l, spatial_coeff);
                    let h_r = cr.high_passed(t_r, spatial_coeff);
                    (0.5 * (h_l - h_r), 0.5 * (h_r - h_l), 0.0, 0.0)
                }
                ChorusMode::Classic => {
                    let (t_l, t_r) = if knobs.far > 0.0 {
                        (cl.read_two(0, knobs.center, knobs.far, excursion, sine(slow_l) as f32, sine(slow_l + far_phase) as f32, longest, identity), cr.read_two(0, knobs.center, knobs.far, excursion, sine(slow_r) as f32, sine(slow_r + far_phase) as f32, longest, identity))
                    } else if dual {
                        dual_pair(cl, cr, 0, knobs.center, excursion, (slow_l, slow_r), longest, identity)
                    } else {
                        (cl.read_log(0, knobs.center, log_swing(knobs.depth, slow_l), longest), cr.read_log(0, knobs.center, log_swing(knobs.depth, slow_r), longest))
                    };
                    (t_l, t_r, t_l, t_r)
                }
            };

            let (x_l, x_r) = (l[i], r[i]);
            let fb_gain = knobs.feedback * mode_gain;
            cl.write(x_l + fb_gain * fb_l, paths[LINE_CUT], &knobs.filters[LINE_CUT], bbd, tickle);
            cr.write(x_r + fb_gain * fb_r, paths[LINE_CUT], &knobs.filters[LINE_CUT], bbd, -tickle);

            let wet_gain = mode_gain * wet_scale;
            wet_l = cl.cut_wet(wet_l * wet_gain, paths[WET_CUT], &knobs.filters[WET_CUT]);
            wet_r = cr.cut_wet(wet_r * wet_gain, paths[WET_CUT], &knobs.filters[WET_CUT]);
            if (knobs.width - 1.0).abs() > WIDTH_UNITY_EPS {
                (wet_l, wet_r) = widen(wet_l, wet_r, knobs.width);
            }

            let mix = self.mix.tick(wet);
            let (out_l, out_r) = if params.crossfade {
                let (sin, cos) = (mix * std::f32::consts::FRAC_PI_2).sin_cos();
                (cos.max(0.0) * x_l + sin * wet_l, cos.max(0.0) * x_r + sin * wet_r)
            } else {
                (x_l + mix * wet_l, x_r + mix * wet_r)
            };
            l[i] = finite(knobs.trim * out_l);
            r[i] = finite(knobs.trim * out_r);
        }

        self.knobs = Some(knobs);
        self.phases = Some((phase_slow, phase_fast));
        self.mode_gain = mode_gain;
        self.tickle = tickle;
    }
}
