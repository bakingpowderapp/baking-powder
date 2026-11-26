use super::*;
use sdk::face::spanned;
use crate::params::freak::{CURVE_OR_NONE, FreakBand, Order, SubSlope};

pub(crate) const READOUT_ROWS: i32 = 8;

const BIAS_PRE_LABELS: [&str; 2] = ["Post", "Pre"];
const TIMES_LABELS: [&str; 4] = ["1x", "2x", "3x", "4x"];
const CEILING_ON_LABELS: [&str; 2] = ["WET", "MIX"];

fn band_curve(band: &FreakBand) -> f64 {
    band.curve.map_or(0.0, |curve| curve.index() as f64 + 1.0)
}

fn set_band_curve(band: &mut FreakBand, v: f64) {
    band.curve = if v < 0.5 { None } else { Some(choice(Curve::ALL, v - 1.0)) };
}

fn flip_of(band: &FreakBand) -> f64 {
    f64::from(u8::from(band.invert) | (u8::from(band.invert_copy) << 1))
}

fn set_flip(band: &mut FreakBand, v: f64) {
    let bits = v.round() as u8;
    band.invert = bits & 1 != 0;
    band.invert_copy = bits & 2 != 0;
}

fn biased(band: &FreakBand) -> bool {
    band.bias_pre || matches!(band.curve, Some(Curve::Tube | Curve::Diode | Curve::Fold | Curve::HardClip | Curve::HalfRect | Curve::FullRect | Curve::Phase))
}

fn beyond<const N: usize>(p: &Freak) -> bool {
    p.band_count <= N
}

fn unshaped<const N: usize>(p: &Freak) -> bool {
    beyond::<N>(p) || p.bands[N].curve.is_none()
}

fn unbiased<const N: usize>(p: &Freak) -> bool {
    unshaped::<N>(p) || !biased(&p.bands[N])
}

fn lacks<const N: usize>(p: &Freak, curve: Curve) -> bool {
    beyond::<N>(p) || p.bands[N].curve != Some(curve)
}

fn not_diode<const N: usize>(p: &Freak) -> bool {
    lacks::<N>(p, Curve::Diode)
}

fn not_decimate<const N: usize>(p: &Freak) -> bool {
    lacks::<N>(p, Curve::Decimate)
}

fn not_fold<const N: usize>(p: &Freak) -> bool {
    lacks::<N>(p, Curve::Fold)
}

fn unrippled<const N: usize>(p: &Freak) -> bool {
    not_fold::<N>(p) || p.bands[N].fold_ripples <= 0.0
}

fn not_phase<const N: usize>(p: &Freak) -> bool {
    lacks::<N>(p, Curve::Phase)
}

fn unsqueezed<const N: usize>(p: &Freak) -> bool {
    beyond::<N>(p) || !p.bands[N].squeezing()
}

fn unexpanded<const N: usize>(p: &Freak) -> bool {
    unsqueezed::<N>(p) || !p.bands[N].squeeze.expanding()
}

fn not_peak<const N: usize>(p: &Freak) -> bool {
    unsqueezed::<N>(p) || p.bands[N].squeeze.detect_mode != DetectMode::Peak
}

macro_rules! band_foot {
    ($n:literal, $band:literal, $shp:literal, $sqz:literal, $mst:literal) => {
        [
            desc(unless(page(6, 0, "Shape", $band, "VIEW SHP", $shp), beyond::<$n>), "Shaper view"),
            desc(unless(page(6, 1, "Squeeze", $band, "VIEW SQZ", $sqz), beyond::<$n>), "Squeeze view"),
            desc(unless(page(6, 2, "Master", $band, "VIEW MST", $mst), beyond::<$n>), "Master view"),
            desc(held(unless(switch(6, 3, &TIMES_LABELS, $band, "TIMES", field!(|p| p.bands[$n].times as f64 - 1.0, |p, v| p.bands[$n].times = v.round() as usize + 1)), beyond::<$n>)), "Runs the band's chain this many times over"),
        ]
    };
}

macro_rules! shape_view {
    ($n:literal, $band:literal, $params:expr) => {
        [
            desc(unless(value(Centre, 0, 0, "Input", $params.input, Decibels), beyond::<$n>), "Band input gain"),
            desc(unless(pick(0, 1, 1, $band, "CURVE", field!(|p| band_curve(&p.bands[$n]), |p, v| set_band_curve(&mut p.bands[$n], v)), &CURVE_OR_NONE), beyond::<$n>), "Curve; None leaves the band unshaped"),
            desc(inert(unless(pick(0, 2, 1, $band, "STEREO", choose!(BandStereo::ALL, bands[$n].stereo), BandStereo::LABELS), beyond::<$n>), unshaped::<$n>), "Stereo component the curve shapes"),
            desc(inert(unless(value(Rise, 0, 3, "Drive", $params.drive, Decibels), beyond::<$n>), unshaped::<$n>), "Band drive on top of the master Drive"),
            desc(inert(unless(value(Centre, 0, 4, "Bias", $params.bias, SignedPercent), beyond::<$n>), unbiased::<$n>), "Curve bias, +/-4 units; the bias LFO swings it"),
            desc(inert(unless(switch(0, 5, &BIAS_PRE_LABELS, $band, "BIAS PRE", flag!(bands[$n].bias_pre)), beyond::<$n>), unbiased::<$n>), "Pre applies Bias before drive, in full-scale input units"),
            desc(inert(unless(value(Rise, 0, 6, "Wet", $params.wet, Percent), beyond::<$n>), unshaped::<$n>), "Curve/input blend"),
            desc(unless(pick(0, 7, 1, $band, "FLIP", field!(|p| flip_of(&p.bands[$n]), |p, v| set_flip(&mut p.bands[$n], v)), &FLIP_LABELS), beyond::<$n>), "Polarity: Whole, dry Copy or Curve"),
            desc(inert(unless(value(Centre, 1, 0, "Dynamics", $params.dynamics, SignedPercent), beyond::<$n>), unshaped::<$n>), "Envelope-driven drive, up to +/-12 dB"),
            desc(inert(unless(value(Rise, 1, 1, "Thresh", $params.threshold, Decibels), beyond::<$n>), unshaped::<$n>), "Threshold for Lift and Sharpen"),
            desc(inert(unless(value(Rise, 1, 2, "Lift", $params.lift, TwoPlaces), beyond::<$n>), unshaped::<$n>), "Pre-drive lift per dB below Thresh, capped by Range"),
            desc(inert(unless(value(Rise, 1, 3, "Sharpen", $params.sharpen, TwoPlaces), beyond::<$n>), unshaped::<$n>), "Pre-drive expansion per dB from Thresh, capped by Range"),
            desc(inert(unless(value(Rise, 1, 4, "Range", $params.range, Decibels), beyond::<$n>), unshaped::<$n>), "Maximum Lift/Sharpen gain change"),
            desc(inert(unless(value(Rise, 1, 5, "Attack", $params.attack, MillisTenth), beyond::<$n>), unshaped::<$n>), "Curve envelope attack"),
            desc(inert(unless(value(Rise, 1, 6, "Release", $params.release, MillisWhole), beyond::<$n>), unshaped::<$n>), "Curve envelope release"),
            desc(inert(unless(toggle(1, 7, "Linked", $band, "LINKED", flag!(bands[$n].linked)), beyond::<$n>), unshaped::<$n>), "Link the curve envelope across channels"),
            desc(inert(unless(value(Centre, 2, 0, "Bass", $params.bass, Decibels), beyond::<$n>), unshaped::<$n>), "Low shelf at 119 Hz on the shaped output"),
            desc(inert(unless(value(Centre, 2, 1, "Mid", $params.mid, Decibels), beyond::<$n>), unshaped::<$n>), "Wide bell at 600 Hz on the shaped output"),
            desc(inert(unless(value(Centre, 2, 2, "Treble", $params.treble, Decibels), beyond::<$n>), unshaped::<$n>), "High shelf at 2.5 kHz on the shaped output"),
            desc(inert(unless(value(Centre, 2, 3, "Presence", $params.presence, Decibels), beyond::<$n>), unshaped::<$n>), "High shelf at 7 kHz on the shaped output"),
            desc(unless(value(Rise, 3, 0, "Knee", $params.knee, Percent), not_diode::<$n>), "Diode curve softness"),
            desc(unless(value(Rise, 3, 0, "Decimate", $params.decimate, Hertz), not_decimate::<$n>), "Decimate sample-and-hold rate"),
            desc(unless(value(Rise, 3, 0, "Ripples", $params.ripples, Percent), not_fold::<$n>), "Fold ripple depth"),
            desc(unless(value(Rise, 3, 0, "Tone", $params.phase_tone, Hertz), not_phase::<$n>), "Phase curve modulator low-pass"),
            desc(unless(value(Rise, 3, 1, "Count", $params.ripple_count, Integer), unrippled::<$n>), "Fold ripple frequency multiplier"),
            desc(unless(value(Rise, 3, 1, "Normal", $params.normalize, Percent), not_phase::<$n>), "Phase curve depth made level independent"),
            desc(unless(switch(3, 2, RippleShape::LABELS, $band, "RIPPLE SHAPE", choose!(RippleShape::ALL, bands[$n].fold_ripple_shape)), unrippled::<$n>), "Fold ripple shape"),
            desc(unless(value(Rise, 3, 2, "Spread", $params.phase_spread, Degrees), not_phase::<$n>), "Phase offset between the sides"),
        ]
    };
}

macro_rules! squeeze_view {
    ($n:literal, $band:literal, $params:expr) => {
        [
            desc(held(unless(switch(0, 0, Order::LABELS, $band, "ORDER", choose!(Order::ALL, bands[$n].order)), beyond::<$n>)), "Squeeze OFF, POST the curve or PRE it"),
            desc(inert(unless(switch(0, 1, DetectMode::LABELS, $band, "DETECTOR", choose!(DetectMode::ALL, bands[$n].squeeze.detect_mode)), beyond::<$n>), unsqueezed::<$n>), "RMS over Detect; PEAK holds for Hold, releases over Decay"),
            desc(inert(unless(value(Rise, 0, 2, "Detect", $params.detect, MillisTenth), beyond::<$n>), unsqueezed::<$n>), "RMS detector window"),
            desc(inert(unless(value(Rise, 0, 3, "Hold", $params.hold, MillisWhole), beyond::<$n>), not_peak::<$n>), "Peak hold window"),
            desc(inert(unless(value(Rise, 0, 4, "Decay", $params.detect_release, MillisWhole), beyond::<$n>), not_peak::<$n>), "Peak detector release"),
            desc(inert(unless(switch(0, 5, DetectStereo::LABELS, $band, "STEREO", choose!(DetectStereo::ALL, bands[$n].squeeze.detect_stereo)), beyond::<$n>), unsqueezed::<$n>), "MAX the louder channel, MEAN the average channel power"),
            desc(inert(unless(switch(0, 6, DetectSlope::LABELS, $band, "DETECT SLOPE", choose!(DetectSlope::ALL, bands[$n].squeeze.detect_slope)), beyond::<$n>), unsqueezed::<$n>), "Detector slope; 12 includes neighbours"),
            desc(inert(unless(value(Rise, 1, 0, "Thresh", $params.threshold, Decibels), beyond::<$n>), unsqueezed::<$n>), "Compression threshold"),
            desc(inert(unless(value(Rise, 1, 1, "Ratio", $params.ratio, Ratio), beyond::<$n>), unsqueezed::<$n>), "Compression ratio"),
            desc(inert(unless(value(Rise, 1, 2, "Attack", $params.attack, Millis), beyond::<$n>), unsqueezed::<$n>), "How fast the gain falls"),
            desc(inert(unless(value(Rise, 1, 3, "Release", $params.release, Millis), beyond::<$n>), unsqueezed::<$n>), "How fast the gain recovers"),
            desc(inert(unless(value(Rise, 1, 4, "Up", $params.up, Percent), beyond::<$n>), unsqueezed::<$n>), "Upward compression amount"),
            desc(inert(unless(value(Rise, 1, 5, "Down", $params.down, Percent), beyond::<$n>), unsqueezed::<$n>), "Downward compression amount"),
            desc(inert(unless(value(Rise, 1, 6, "Floor", $params.gate_floor, Decibels), beyond::<$n>), unsqueezed::<$n>), "Lift fades out over 12 dB below this; -120 is off"),
            desc(inert(unless(value(Rise, 1, 7, "Cap", $params.cap, Decibels), beyond::<$n>), unsqueezed::<$n>), "Maximum upward gain"),
            desc(inert(unless(value(Centre, 2, 0, "Gain", $params.gain, Decibels), beyond::<$n>), unsqueezed::<$n>), "Makeup gain after compression"),
            desc(inert(unless(value(Rise, 2, 1, "Clip", $params.soft_clip, TwoPlaces), beyond::<$n>), unsqueezed::<$n>), "Full-scale soft-clip level after gain; 0 is off"),
            desc(inert(unless(value(Rise, 2, 2, "Width", $params.width, Percent), beyond::<$n>), unsqueezed::<$n>), "Side level after gain; 0 is mono"),
            desc(inert(unless(value(Rise, 2, 3, "Mix", $params.mix, Percent), beyond::<$n>), unsqueezed::<$n>), "Squeezed/input blend"),
            desc(inert(unless(value(Rise, 3, 0, "Expand", $params.expand, Decibels), beyond::<$n>), unsqueezed::<$n>), "Expander threshold; -120 is off"),
            desc(inert(unless(value(Rise, 3, 1, "Ratio", $params.expand_ratio, Ratio), beyond::<$n>), unexpanded::<$n>), "Expander ratio"),
            desc(inert(unless(value(Rise, 3, 2, "Knee", $params.expand_knee, DecibelsUnsigned), beyond::<$n>), unexpanded::<$n>), "Expander knee; 0 is hard"),
            desc(inert(unless(value(Rise, 3, 3, "Attack", $params.expand_attack, Millis), beyond::<$n>), unexpanded::<$n>), "How fast the expander opens"),
            desc(inert(unless(value(Rise, 3, 4, "Release", $params.expand_release, Millis), beyond::<$n>), unexpanded::<$n>), "How fast the expander closes"),
        ]
    };
}

const GLOBALS: [Cell<Freak>; 1] = widened([desc(value(Rise, 6, 7, "Dry", params::freak::DRY, Percent), "Dry kept as the row's wet rises; 0 crossfades, 100% keeps full dry")], 2);

const FOOT1: [Cell<Freak>; 4] = spanned(widened(band_foot!(0, "BAND 1", 0, 1, 2), 2), 0, 3);

const FOOT2: [Cell<Freak>; 4] = spanned(widened(band_foot!(1, "BAND 2", 3, 4, 5), 2), 3, 3);

const FOOT3: [Cell<Freak>; 4] = spanned(widened(band_foot!(2, "BAND 3", 6, 7, 8), 2), 6, 3);

const FOOT4: [Cell<Freak>; 4] = spanned(widened(band_foot!(3, "BAND 4", 9, 10, 11), 2), 9, 3);

const SHAPE1: [Cell<Freak>; 28] = viewed(widened(shape_view!(0, "BAND 1", params::freak::SHAPE1), 2), 0);

const SHAPE2: [Cell<Freak>; 28] = viewed(widened(shape_view!(1, "BAND 2", params::freak::SHAPE2), 2), 3);

const SHAPE3: [Cell<Freak>; 28] = viewed(widened(shape_view!(2, "BAND 3", params::freak::SHAPE3), 2), 6);

const SHAPE4: [Cell<Freak>; 28] = viewed(widened(shape_view!(3, "BAND 4", params::freak::SHAPE4), 2), 9);

const SQUEEZE1: [Cell<Freak>; 24] = viewed(widened(squeeze_view!(0, "SQUEEZE 1", params::freak::SQUEEZE1), 2), 1);

const SQUEEZE2: [Cell<Freak>; 24] = viewed(widened(squeeze_view!(1, "SQUEEZE 2", params::freak::SQUEEZE2), 2), 4);

const SQUEEZE3: [Cell<Freak>; 24] = viewed(widened(squeeze_view!(2, "SQUEEZE 3", params::freak::SQUEEZE3), 2), 7);

const SQUEEZE4: [Cell<Freak>; 24] = viewed(widened(squeeze_view!(3, "SQUEEZE 4", params::freak::SQUEEZE4), 2), 10);

const GRID: [Cell<Freak>; 21] = widened([
    desc(value(Rise, 0, 0, "Drive", params::freak::DRIVE, Decibels), "Drive added to every band"),
    desc(value(Rise, 0, 1, "Punish", params::freak::PUNISH, Percent), "Up to 5x drive with added dynamics on every band"),
    desc(value(Rise, 0, 2, "Out", params::freak::OUTPUT, Decibels), "Effect output gain"),
    desc(value(Rise, 0, 3, "Sub", params::freak::SUB_CUT, Hertz), "Input sub cut before the split, dry included; 0 is off"),
    desc(inert(switch(0, 4, SubSlope::LABELS, "MASTER", "SUB CUT SLOPE", choose!(SubSlope::ALL, sub_cut_slope)), |p| p.sub_cut_hz <= 0.0), "Sub cut slope in dB/octave"),
    desc(held(unless(switch(0, 5, FreakSlope::LABELS, "BANDS", "SLOPE", choose!(FreakSlope::ALL, slope)), beyond::<1>)), "Crossover slope in dB/octave, or linear phase with about 9 ms latency"),
    desc(toggle(0, 6, "2x", "MASTER", "OVERSAMPLE", flag!(oversample)), "2x oversampling; two samples of latency per pass of the deepest band"),
    desc(inert(toggle(0, 7, "Flat top", "MASTER", "TOP FLAT", flag!(flat_top)), |p| !p.oversample), "Compensate oversampling high-frequency loss"),
    desc(toggle(2, 0, "Limit", "OUTPUT", "CEILING ON", flag!(ceiling)), "Limit the wet after Out"),
    desc(inert(value(Rise, 2, 1, "Ceiling", params::freak::CEILING, Decibels), |p| !p.ceiling), "Ceiling in dBFS"),
    desc(inert(value(Rise, 2, 2, "Knee", params::freak::KNEE, DecibelsUnsigned), |p| !p.ceiling), "Ceiling knee width below Ceiling; 0 hard-clips"),
    desc(inert(value(Centre, 2, 3, "Trim", params::freak::CEILING_TRIM, Decibels), |p| !p.ceiling), "Gain into the ceiling"),
    desc(inert(value(Rise, 2, 4, "Attack", params::freak::CEILING_ATTACK, MillisTenth), |p| !p.ceiling), "Ceiling attack; 0 clips at once"),
    desc(inert(value(Rise, 2, 5, "Release", params::freak::CEILING_RELEASE, MillisWhole), |p| !p.ceiling), "Ceiling release; 0 soft-clips only"),
    desc(inert(switch(2, 6, &CEILING_ON_LABELS, "OUTPUT", "CEILING ON MIX", flag!(ceiling_on_mix)), |p| !p.ceiling), "MIX limits after the dry/wet mix; WET, the wet only"),
    desc(toggle(3, 0, "Xfade", "MASTER", "CROSSFADE", flag!(crossfade)), "Equal-power dry/wet crossfade; off, linear"),
    desc(toggle(3, 1, "Raw dry", "MASTER", "DRY PLAIN", flag!(dry_plain)), "Use unfiltered dry; off, phase-match dry to the split"),
    desc(value(Rise, 4, 0, "LFO rate", params::freak::BIAS_LFO_RATE, LfoHertz), "Bias LFO rate, unsynced"),
    desc(value(Rise, 4, 1, "Depth", params::freak::BIAS_LFO_DEPTH, Percent), "Bias LFO depth, up to 4 Bias units"),
    desc(value(Rise, 4, 2, "Spread", params::freak::BIAS_LFO_SPREAD, Spread), "Right or side bias LFO phase lag; 180° is half a cycle"),
    desc(value(Rise, 4, 3, "Phase", params::freak::BIAS_LFO_PHASE, Degrees), "Bias LFO phase at playback start"),
], 2);

const fn master(page: usize, trays: &[usize]) -> [Cell<Freak>; 34] {
    let filters: [Cell<Freak>; 1] = [desc(wide(open(1, 0, "Filter", "FILTER", "OPEN", trays[0], trays.len()), 2), "Two filters on the output; opens them on the screen")];
    let filter1: [Cell<Freak>; 6] = filter_cells!(trays[0], "FILTER 1", "the output", params::freak::FILTER1, filter1);
    let filter2: [Cell<Freak>; 6] = filter_cells!(trays[1], "FILTER 2", "the output", params::freak::FILTER2, filter2);
    viewed(flat(&[&GRID, &filters, &filter1, &filter2]), page)
}

const MASTER1: [Cell<Freak>; 34] = master(2, Freak::MASTER_VIEW_TRAYS[0]);

const MASTER2: [Cell<Freak>; 34] = master(5, Freak::MASTER_VIEW_TRAYS[1]);

const MASTER3: [Cell<Freak>; 34] = master(8, Freak::MASTER_VIEW_TRAYS[2]);

const MASTER4: [Cell<Freak>; 34] = master(11, Freak::MASTER_VIEW_TRAYS[3]);

pub(crate) static CELLS: [Cell<Freak>; 361] = flat(&[&GLOBALS, &FOOT1, &FOOT2, &FOOT3, &FOOT4, &SHAPE1, &SHAPE2, &SHAPE3, &SHAPE4, &SQUEEZE1, &SQUEEZE2, &SQUEEZE3, &SQUEEZE4, &MASTER1, &MASTER2, &MASTER3, &MASTER4]);
