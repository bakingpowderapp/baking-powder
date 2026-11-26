use super::*;
use sdk::face::{named, spanned};
use crate::params::motion::MotionBand;

const A: usize = 0;
const B: usize = 1;
const VIEWS: usize = 2;
const POLARITY_LABELS: [&str; 2] = ["NOTCH", "PEAK"];
const A_DESC: &str = "Mode, LFO, stereo, sweep, feedback and mix";
const B_DESC: &str = "Stages or second tap, the band, crossover and master";

const fn band_page(band: usize, view: usize) -> usize {
    band * VIEWS + view
}

fn stage_index(band: &MotionBand) -> f64 {
    STAGES.iter().position(|stages| *stages == band.phaser.stages).unwrap_or(1) as f64
}

fn set_stages(band: &mut MotionBand, v: f64) {
    band.phaser.stages = choice(&STAGES, v);
}

fn beyond<const N: usize>(p: &Motion) -> bool {
    p.count() <= N
}

fn not_in<const N: usize>(p: &Motion, mode: MotionMode) -> bool {
    beyond::<N>(p) || p.bands[N].mode != mode
}

fn not_chorus<const N: usize>(p: &Motion) -> bool {
    not_in::<N>(p, MotionMode::Chorus)
}

fn not_flanger<const N: usize>(p: &Motion) -> bool {
    not_in::<N>(p, MotionMode::Flanger)
}

fn not_phaser<const N: usize>(p: &Motion) -> bool {
    not_in::<N>(p, MotionMode::Phaser)
}

fn unmodulated<const N: usize>(p: &Motion) -> bool {
    beyond::<N>(p) || !p.bands[N].modulating()
}

fn chorus_synced<const N: usize>(p: &Motion) -> bool {
    not_chorus::<N>(p) || p.bands[N].chorus.sync
}

fn chorus_unsynced<const N: usize>(p: &Motion) -> bool {
    not_chorus::<N>(p) || !p.bands[N].chorus.sync
}

fn spatial<const N: usize>(p: &Motion) -> bool {
    not_chorus::<N>(p) || p.bands[N].chorus.mode == ChorusMode::Spatial
}

fn unfar<const N: usize>(p: &Motion) -> bool {
    spatial::<N>(p) || p.bands[N].chorus.far_center_ms <= 0.0
}

fn flanger_synced<const N: usize>(p: &Motion) -> bool {
    not_flanger::<N>(p) || p.bands[N].flanger.sync
}

fn flanger_unsynced<const N: usize>(p: &Motion) -> bool {
    not_flanger::<N>(p) || !p.bands[N].flanger.sync
}

fn standard<const N: usize>(p: &Motion) -> bool {
    not_flanger::<N>(p) || p.bands[N].flanger.mode == FlangerMode::Standard
}

fn phaser_synced<const N: usize>(p: &Motion) -> bool {
    not_phaser::<N>(p) || p.bands[N].phaser.sync
}

fn phaser_unsynced<const N: usize>(p: &Motion) -> bool {
    not_phaser::<N>(p) || !p.bands[N].phaser.sync
}

fn single<const N: usize>(p: &Motion) -> bool {
    not_phaser::<N>(p) || p.bands[N].phaser.routing == Routing::Single
}

fn unclamped<const N: usize>(p: &Motion) -> bool {
    not_phaser::<N>(p) || !p.bands[N].phaser.stage_clamp
}

fn not_vintage<const N: usize>(p: &Motion) -> bool {
    not_phaser::<N>(p) || !p.bands[N].phaser.vintage
}

fn own_title<const N: usize>(p: &Motion) -> &'static str {
    match p.bands[N].mode {
        MotionMode::Chorus => "SECOND TAP",
        MotionMode::Phaser => "STAGES",
        MotionMode::None | MotionMode::Flanger => "",
    }
}

fn second_title<const N: usize>(p: &Motion) -> &'static str {
    match p.bands[N].mode {
        MotionMode::Phaser => "SECOND PHASER",
        MotionMode::None | MotionMode::Chorus | MotionMode::Flanger => "",
    }
}

macro_rules! tabs {
    ($n:literal, $band:literal) => {
        [
            desc(unless(page(0, 0, "A", $band, "VIEW A", band_page($n, A)), beyond::<$n>), A_DESC),
            desc(unless(page(0, 1, "B", $band, "VIEW B", band_page($n, B)), beyond::<$n>), B_DESC),
        ]
    };
}

macro_rules! view_a {
    ($n:literal, $band:literal, $chorus_band:literal, $flanger_band:literal, $phaser_band:literal, $filter_band:literal, $chorus:expr, $flanger:expr, $phaser:expr, $levels:expr) => {
        [
            title(1, 0, "MODE"),
            desc(unless(pick(2, 0, 3, $band, "MODE", choose!(MotionMode::ALL, bands[$n].mode), MotionMode::LABELS), beyond::<$n>), "Modulation in this band: chorus, flanger, phaser or none"),
            desc(unless(pick(2, 3, 3, $chorus_band, "MODE", choose!(ChorusMode::ALL, bands[$n].chorus.mode), ChorusMode::LABELS), not_chorus::<$n>), "Chorus mode"),
            desc(unless(pick(2, 3, 3, $flanger_band, "MODE", choose!(FlangerMode::ALL, bands[$n].flanger.mode), FlangerMode::LABELS), not_flanger::<$n>), "Flanger mode"),
            desc(unless(pick(2, 3, 3, $phaser_band, "ROUTING", choose!(Routing::ALL, bands[$n].phaser.routing), Routing::LABELS), not_phaser::<$n>), "Dual-phaser routing"),
            title(1, 7, "LFO"),
            desc(unless(pick(2, 7, 3, $chorus_band, "DIVISION", choose!(&Division::ALL, bands[$n].chorus.division), DIVISIONS), chorus_unsynced::<$n>), "Synced LFO division"),
            desc(unless(pick(2, 7, 3, $flanger_band, "DIVISION", choose!(&Division::ALL, bands[$n].flanger.division), DIVISIONS), flanger_unsynced::<$n>), "Synced LFO division"),
            desc(unless(pick(2, 7, 3, $phaser_band, "DIVISION", choose!(&Division::ALL, bands[$n].phaser.division), DIVISIONS), phaser_unsynced::<$n>), "Synced LFO division"),
            desc(unless(wide(value(Rise, 2, 7, "RATE", $chorus.rate, LfoHertz), 3), chorus_synced::<$n>), "LFO rate"),
            desc(unless(wide(value(Rise, 2, 7, "RATE", $flanger.rate, LfoHertz), 3), flanger_synced::<$n>), "LFO rate"),
            desc(unless(wide(value(Rise, 2, 7, "RATE", $phaser.rate, LfoHertz), 3), phaser_synced::<$n>), "LFO rate, up to 400 Hz"),
            desc(unless(toggle(2, 10, "SYNC", $chorus_band, "SYNC", flag!(bands[$n].chorus.sync)), not_chorus::<$n>), "Use tempo division instead of RATE"),
            desc(unless(toggle(2, 10, "SYNC", $flanger_band, "SYNC", flag!(bands[$n].flanger.sync)), not_flanger::<$n>), "Use tempo division instead of RATE"),
            desc(unless(toggle(2, 10, "SYNC", $phaser_band, "SYNC", flag!(bands[$n].phaser.sync)), not_phaser::<$n>), "Use tempo division instead of RATE"),
            desc(unless(wide(value(Rise, 2, 11, "PHASE", $chorus.phase, Degrees), 2), not_chorus::<$n>), "LFO phase at playback start"),
            desc(unless(wide(value(Rise, 2, 11, "PHASE", $flanger.phase, Degrees), 2), not_flanger::<$n>), "LFO phase at playback start"),
            desc(unless(wide(value(Rise, 2, 11, "PHASE", $phaser.phase, Degrees), 2), not_phaser::<$n>), "LFO phase at playback start"),
            desc(unless(pick(2, 13, 3, $flanger_band, "SHAPE", choose!(FlangerShape::ALL, bands[$n].flanger.shape), FlangerShape::LABELS), not_flanger::<$n>), "LFO shape"),
            desc(unless(pick(2, 13, 3, $phaser_band, "SHAPE", choose!(PhaserShape::ALL, bands[$n].phaser.shape), PhaserShape::LABELS), not_phaser::<$n>), "LFO shape"),
            title(3, 0, "STEREO"),
            desc(unless(wide(value(Rise, 4, 0, "SPREAD", $chorus.spread, Spread), 3), spatial::<$n>), "Left/right LFO phase offset"),
            desc(unless(wide(value(Rise, 4, 0, "SPREAD", $flanger.spread, Spread), 3), not_flanger::<$n>), "Left/right LFO phase offset"),
            desc(unless(wide(value(Rise, 4, 0, "SPREAD", $phaser.spread, Spread), 3), not_phaser::<$n>), "Left/right LFO phase offset"),
            desc(unless(wide(value(Rise, 4, 3, "WIDTH", $chorus.width, Percent), 3), not_chorus::<$n>), "Wet width"),
            desc(unless(wide(value(Rise, 4, 3, "WIDTH", $flanger.width, Percent), 3), not_flanger::<$n>), "Wet width"),
            desc(unless(wide(value(Rise, 4, 3, "WIDTH", $phaser.width, Percent), 3), not_phaser::<$n>), "Wet width"),
            title(3, 7, "SWEEP"),
            desc(unless(wide(value(Rise, 4, 7, "CENTER", $chorus.center, MillisHundredths), 3), spatial::<$n>), "Modulation center delay"),
            desc(unless(wide(value(Rise, 4, 7, "CENTER", $flanger.center, MillisHundredths), 3), not_flanger::<$n>), "Modulation center delay"),
            desc(unless(wide(value(Rise, 4, 7, "CENTER", $phaser.center, Hertz), 3), not_phaser::<$n>), "Sweep center frequency"),
            desc(unless(wide(value(Rise, 4, 10, "DEPTH", $chorus.depth, Percent), 3), not_chorus::<$n>), "Delay modulation depth"),
            desc(unless(wide(value(Rise, 4, 10, "DEPTH", $flanger.depth, Percent), 3), not_flanger::<$n>), "Delay modulation depth"),
            desc(unless(wide(value(Rise, 4, 10, "DEPTH", $phaser.depth, Octaves), 3), not_phaser::<$n>), "Sweep depth, up to 24 octaves"),
            desc(unless(wide(value(Centre, 4, 13, "RIGHT OFFSET", $phaser.right_offset, Octaves), 3), not_phaser::<$n>), "Right-channel frequency offset in octaves"),
            title(5, 0, "FEEDBACK"),
            desc(unless(wide(value(Rise, 6, 0, "AMOUNT", $chorus.feedback, Percent), 2), spatial::<$n>), "Wet feedback"),
            desc(unless(wide(value(Centre, 6, 0, "AMOUNT", $flanger.feedback, SignedPercent), 2), not_flanger::<$n>), "Feedback; negative inverts polarity"),
            desc(unless(wide(value(Centre, 6, 0, "AMOUNT", $phaser.feedback, SignedPercent), 2), not_phaser::<$n>), "Feedback, either polarity"),
            desc(unless(wide(open(6, 2, "FILTER", $filter_band, "OPEN", Motion::FILTER_TRAYS[$n][0], Motion::FILTER_TRAYS[$n].len()), 2), not_chorus::<$n>), "Two filters on the wet; opens them on the screen"),
            desc(unless(wide(toggle(6, 2, "VINTAGE", $phaser_band, "VINTAGE", flag!(bands[$n].phaser.vintage)), 2), not_phaser::<$n>), "Vintage feedback coloration"),
            desc(unless(wide(value(Rise, 6, 4, "DRIVE", $phaser.drive, Times), 2), not_vintage::<$n>), "Vintage feedback saturation"),
            desc(unless(wide(value(Rise, 7, 0, "LOW CUT", $phaser.low_cut, Hertz), 3), not_phaser::<$n>), "Low cut in the feedback"),
            desc(unless(wide(value(Rise, 7, 3, "HIGH CUT", $phaser.high_cut, HertzOffAt20k), 3), not_phaser::<$n>), "High cut in the feedback; off at the top"),
            title(5, 7, "MIX"),
            desc(unless(wide(value(Centre, 6, 7, "TRIM", $chorus.trim, Decibels), 3), not_chorus::<$n>), "Output gain after dry/wet mix"),
            desc(unless(wide(value(Rise, 6, 7, "DRY", $flanger.dry, Percent), 3), not_flanger::<$n>), "Dry kept as WET rises; 0 crossfades, 100% keeps full dry"),
            desc(unless(wide(value(Rise, 6, 7, "DRY", $phaser.dry, Percent), 3), not_phaser::<$n>), "Dry kept as WET rises; 0 crossfades, 100% keeps full dry"),
            desc(inert(unless(wide(value(Rise, 6, 10, "WET", $levels.wet, Percent), 3), beyond::<$n>), unmodulated::<$n>), "The band's share of the row's wet"),
            desc(unless(wide(toggle(6, 13, "CROSSFADE", $chorus_band, "CROSSFADE", flag!(bands[$n].chorus.crossfade)), 3), not_chorus::<$n>), "Equal-power dry/wet crossfade"),
            desc(unless(wide(switch(6, 13, &POLARITY_LABELS, $flanger_band, "POLARITY", flag!(bands[$n].flanger.peak)), 3), standard::<$n>), "PEAK adds wet; NOTCH subtracts it"),
            desc(unless(wide(value(Rise, 6, 13, "BLEND", $phaser.blend, Percent), 3), not_phaser::<$n>), "Swept signal against the input; 50% is equal parts"),
        ]
    };
}

macro_rules! view_b {
    ($n:literal, $band:literal, $chorus_band:literal, $phaser_band:literal, $chorus:expr, $phaser:expr, $levels:expr $(, $xov:expr)?) => {
        [
            named(title(1, 0, ""), own_title::<$n>),
            desc(unless(pick(2, 0, 2, $phaser_band, "STAGES", field!(|p| stage_index(&p.bands[$n]), |p, v| set_stages(&mut p.bands[$n], v)), &STAGE_LABELS), not_phaser::<$n>), "Phaser stage count"),
            desc(unless(wide(toggle(2, 0, "DUAL", $chorus_band, "DUAL", flag!(bands[$n].chorus.dual)), 2), not_chorus::<$n>), "Second tap at 1.5x CENTER"),
            desc(unless(wide(value(Rise, 2, 2, "SPACING", $phaser.stage_spread, Octaves), 2), not_phaser::<$n>), "Stage spacing in octaves around CENTER; 0 aligns stages"),
            desc(unless(wide(value(Rise, 2, 2, "DELAY", $chorus.far_center, MillisHundredths), 2), spatial::<$n>), "Second-tap delay on the same LFO; 0 is off, unused in Ensemble"),
            desc(unless(wide(toggle(2, 4, "CLAMP", $phaser_band, "STAGE CLAMP", flag!(bands[$n].phaser.stage_clamp)), 2), not_phaser::<$n>), "Clamp each stage between FLOOR and 18 kHz, not just the center"),
            desc(unless(wide(value(Rise, 2, 4, "PHASE", $chorus.far_phase, Degrees), 2), unfar::<$n>), "Second-tap phase relative to the first"),
            desc(unless(wide(value(Rise, 2, 6, "FLOOR", $phaser.sweep_floor, Hertz), 2), unclamped::<$n>), "Stage frequency floor under CLAMP, 1-20 Hz"),
            named(title(1, 9, ""), second_title::<$n>),
            desc(unless(wide(value(Rise, 2, 9, "RATE RATIO", $phaser.rate_ratio, Times), 3), single::<$n>), "Second-phaser rate multiplier"),
            desc(unless(wide(value(Centre, 2, 12, "STAGE OFFSET", $phaser.stage_offset, SignedInteger), 3), single::<$n>), "Second-phaser stage offset"),
            title(3, 0, $band),
            desc(unless(wide(toggle(4, 0, "ON", $band, "ON", flag!(bands[$n].on)), 2), beyond::<$n>), "Band on; off passes its range through"),
            desc(unless(wide(toggle(4, 2, "MUTE", $band, "MUTE", flag!(bands[$n].mute)), 2), beyond::<$n>), "Silence the band's range, on or off"),
            desc(unless(wide(toggle(4, 4, "SOLO", $band, "SOLO", flag!(bands[$n].solo)), 2), beyond::<$n>), "Hear this band alone"),
            desc(unless(wide(value(Centre, 4, 9, "INPUT", $levels.input, Decibels), 3), beyond::<$n>), "Band input gain"),
            desc(unless(wide(value(Rise, 4, 12, "OUTPUT", $levels.output, Decibels), 3), beyond::<$n>), "Band output gain after its stage"),
            title(5, 0, "CROSSOVER"),
            desc(held(unless(wide(switch(6, 0, FreakSlope::LABELS, "BANDS", "SLOPE", choose!(FreakSlope::ALL, slope)), 2), beyond::<1>)), "Crossover slope in dB/octave, or linear phase with about 9 ms latency"),
            $(desc(unless(wide(value(Rise, 6, 2, "FREQUENCY", $xov, Hertz), 3), beyond::<$n>), "The band's lower crossover"),)?
            title(5, 9, "MASTER"),
            desc(wide(value(Rise, 6, 9, "OUTPUT", params::motion::OUTPUT, Decibels), 3), "Effect output gain"),
        ]
    };
}

macro_rules! filters {
    ($n:literal, $first:literal, $second:literal, $params:expr) => {{
        let first: [Cell<Motion>; 6] = filter_cells!(Motion::FILTER_TRAYS[$n][0], $first, "the wet", $params[0], bands[$n].chorus.filter1);
        let second: [Cell<Motion>; 6] = filter_cells!(Motion::FILTER_TRAYS[$n][1], $second, "the wet and feedback", $params[1], bands[$n].chorus.filter2);
        flat(&[&first, &second])
    }};
}

const MOTION_TABS1: [Cell<Motion>; 2] = spanned(tabs!(0, "BAND 1"), band_page(0, A), VIEWS);

const MOTION_TABS2: [Cell<Motion>; 2] = spanned(tabs!(1, "BAND 2"), band_page(1, A), VIEWS);

const MOTION_TABS3: [Cell<Motion>; 2] = spanned(tabs!(2, "BAND 3"), band_page(2, A), VIEWS);

const MOTION_TABS4: [Cell<Motion>; 2] = spanned(tabs!(3, "BAND 4"), band_page(3, A), VIEWS);

static MOTION_A1: [Cell<Motion>; 52] = viewed(view_a!(0, "BAND 1", "CHORUS 1", "FLANGER 1", "PHASER 1", "CHORUS 1 FILTER", params::motion::CHORUS1, params::motion::FLANGER1, params::motion::PHASER1, params::motion::BAND1), band_page(0, A));

static MOTION_A2: [Cell<Motion>; 52] = viewed(view_a!(1, "BAND 2", "CHORUS 2", "FLANGER 2", "PHASER 2", "CHORUS 2 FILTER", params::motion::CHORUS2, params::motion::FLANGER2, params::motion::PHASER2, params::motion::BAND2), band_page(1, A));

static MOTION_A3: [Cell<Motion>; 52] = viewed(view_a!(2, "BAND 3", "CHORUS 3", "FLANGER 3", "PHASER 3", "CHORUS 3 FILTER", params::motion::CHORUS3, params::motion::FLANGER3, params::motion::PHASER3, params::motion::BAND3), band_page(2, A));

static MOTION_A4: [Cell<Motion>; 52] = viewed(view_a!(3, "BAND 4", "CHORUS 4", "FLANGER 4", "PHASER 4", "CHORUS 4 FILTER", params::motion::CHORUS4, params::motion::FLANGER4, params::motion::PHASER4, params::motion::BAND4), band_page(3, A));

const MOTION_B1: [Cell<Motion>; 21] = viewed(view_b!(0, "BAND 1", "CHORUS 1", "PHASER 1", params::motion::CHORUS1, params::motion::PHASER1, params::motion::BAND1), band_page(0, B));

const MOTION_B2: [Cell<Motion>; 22] = viewed(view_b!(1, "BAND 2", "CHORUS 2", "PHASER 2", params::motion::CHORUS2, params::motion::PHASER2, params::motion::BAND2, params::motion::BAND_2_CROSSOVER), band_page(1, B));

const MOTION_B3: [Cell<Motion>; 22] = viewed(view_b!(2, "BAND 3", "CHORUS 3", "PHASER 3", params::motion::CHORUS3, params::motion::PHASER3, params::motion::BAND3, params::motion::BAND_3_CROSSOVER), band_page(2, B));

const MOTION_B4: [Cell<Motion>; 22] = viewed(view_b!(3, "BAND 4", "CHORUS 4", "PHASER 4", params::motion::CHORUS4, params::motion::PHASER4, params::motion::BAND4, params::motion::BAND_4_CROSSOVER), band_page(3, B));

const MOTION_FILTERS1: [Cell<Motion>; 12] = viewed(filters!(0, "CHORUS 1 FILTER 1", "CHORUS 1 FILTER 2", params::motion::FILTERS1), band_page(0, A));

const MOTION_FILTERS2: [Cell<Motion>; 12] = viewed(filters!(1, "CHORUS 2 FILTER 1", "CHORUS 2 FILTER 2", params::motion::FILTERS2), band_page(1, A));

const MOTION_FILTERS3: [Cell<Motion>; 12] = viewed(filters!(2, "CHORUS 3 FILTER 1", "CHORUS 3 FILTER 2", params::motion::FILTERS3), band_page(2, A));

const MOTION_FILTERS4: [Cell<Motion>; 12] = viewed(filters!(3, "CHORUS 4 FILTER 1", "CHORUS 4 FILTER 2", params::motion::FILTERS4), band_page(3, A));

pub(crate) static CELLS: [Cell<Motion>; 351] = flat(&[&MOTION_TABS1, &MOTION_TABS2, &MOTION_TABS3, &MOTION_TABS4, &MOTION_A1, &MOTION_A2, &MOTION_A3, &MOTION_A4, &MOTION_B1, &MOTION_B2, &MOTION_B3, &MOTION_B4, &MOTION_FILTERS1, &MOTION_FILTERS2, &MOTION_FILTERS3, &MOTION_FILTERS4]);
