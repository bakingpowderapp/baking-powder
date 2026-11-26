use super::*;

const A: usize = 0;
const B: usize = 1;

fn unducked(p: &Reverb) -> bool {
    p.ducking.abs() < f64::EPSILON
}

fn crossed(p: &Reverb) -> bool {
    p.cross_feed <= 0.0 && p.early_cross <= 0.0
}

fn unplateaued(p: &Reverb) -> bool {
    p.plateau_ms <= 0.0
}

fn tap_tempo(synced: bool, ms: &mut f64, division: &mut Division, bpm: f64) {
    if synced {
        *division = Division::nearest(ms.max(1.0), bpm, &Division::ALL);
    } else {
        *ms = division.ms(bpm).min(Reverb::MAX_TAP_MS);
    }
}

macro_rules! tap_cells {
    ($row:expr, $column:expr, $n:literal, $tm:expr, $lv:expr, $sd:expr, $cr:expr, $ms:ident, $sync:ident, $division:ident, $level:ident) => {
        [
            desc(tempo(unless(toggle($row, $column, concat!("SY", $n), "TAPS", concat!("TAP ", $n, " SYNC"), flag!($sync)), |p| p.$level <= 0.0), |p, bpm| tap_tempo(p.$sync, &mut p.$ms, &mut p.$division, bpm)), concat!("Sync free tap ", $n, " to tempo")),
            desc(unless(value(Rise, $row, $column + 1, concat!("TM", $n), $tm, MillisTenth), |p| p.$level <= 0.0 || p.$sync), concat!("Free tap ", $n, " time; ignores PRE")),
            desc(unless(pick($row, $column + 2, 2, "TAPS", concat!("TAP ", $n, " DIVISION"), choose!(&Division::ALL, $division), DIVISIONS), |p| p.$level <= 0.0 || !p.$sync), concat!("Synced free tap ", $n, " division; caps at 2 s")),
            desc(value(Rise, $row, $column + 4, concat!("LV", $n), $lv, Percent), concat!("Free tap ", $n, " level; 0 is off")),
            desc(unless(value(Pan, $row, $column + 5, concat!("SD", $n), $sd, Balance), |p| p.$level <= 0.0), concat!("Free tap ", $n, " pan")),
            desc(unless(value(Rise, $row, $column + 6, concat!("CR", $n), $cr, Percent), |p| p.$level <= 0.0), concat!("Free tap ", $n, " input at full STE: 0 own channel, 100% opposite")),
        ]
    };
}

const TABS: [Cell<Reverb>; 2] = [
    desc(page(0, 0, "A", "SPACE", "VIEW A", A), "Modulation, tone, shimmer, plateau, onset, early, tail and duck"),
    desc(page(0, 1, "B", "SPACE", "VIEW B", B), "Five free taps and their scatter"),
];

static HEAD: [Cell<Reverb>; 13] = [
    desc(value(Rise, 0, 2, "PRE", params::reverb::PRE_DELAY, MillisWhole), "Pre-delay for tail and early reflections"),
    desc(value(Rise, 0, 3, "DEC", params::reverb::DECAY, Seconds), "Tail decay time"),
    desc(value(Rise, 0, 4, "SIZ", params::reverb::SIZE, Times), "Space size, scaling delays"),
    desc(value(Rise, 0, 5, "WID", params::reverb::WIDTH, Percent), "Tail width; 0 mono, above 100% boosts sides"),
    desc(toggle(0, 6, "FRZ", "SPACE", "FREEZE", flag!(freeze)), "Freeze tail and mute input"),
    desc(value(Rise, 0, 7, "FWD", params::reverb::FORWARD, Percent), "Forward tail level"),
    desc(value(Rise, 0, 8, "REV", params::reverb::REVERSE, Percent), "Reverse tail level; swells over DEC into each note"),
    desc(value(Rise, 0, 9, "DIF", params::reverb::DIFFUSION, Percent), "Echo density"),
    desc(value(Rise, 0, 10, "ATK", params::reverb::ATTACK, Millis), "Tail onset spread; 0 is immediate"),
    desc(value(Rise, 0, 11, "ERL", params::reverb::EARLY, Percent), "Early-reflection level; follows PRE and stops with input"),
    desc(unless(value(Rise, 0, 12, "SPN", params::reverb::EARLY_SPAN, MillisWhole), |p| p.early_level <= 0.0), "Early-reflection span"),
    desc(unless(value(Rise, 0, 13, "EWD", params::reverb::EARLY_WIDTH, Percent), |p| p.early_level <= 0.0), "Early width; 0 centered, 100% full stereo pattern"),
    desc(pick(0, 14, 2, "SPACE", "TANK", choose!(Tank::ALL, tank), Tank::LABELS), "Plate builds slowly; Hall starts quickly"),
];

static MAIN: [Cell<Reverb>; 45] = viewed(
    [
        desc(wide(value(Rise, 1, 0, "DEP", params::reverb::DEPTH, Millis), 2), "Delay modulation depth"),
        desc(value(Rise, 1, 2, "PHS", params::reverb::PHASE, Degrees), "LFO phase at playback start"),
        desc(toggle(1, 3, "SYN", "MODULATION", "SYNC", flag!(mod_sync)), "Use tempo division instead of RATE"),
        desc(unless(value(Rise, 1, 4, "RAT", params::reverb::RATE, LfoHertz), |p| p.mod_sync), "LFO rate"),
        desc(unless(pick(1, 5, 2, "MODULATION", "DIVISION", choose!(&Division::ALL, mod_division), DIVISIONS), |p| !p.mod_sync), "Synced LFO division"),
        desc(value(Rise, 1, 7, "RND", params::reverb::RANDOM, Percent), "Random modulation share instead of sine"),
        desc(unless(value(Rise, 1, 8, "SPR", params::reverb::SPARSE, Percent), |p| p.mod_random <= 0.0), "Random modulation sparsity; higher gives rarer jumps"),
        desc(toggle(1, 10, "CUB", "SPACE", "CUBIC LINES", flag!(cubic_lines)), "Keeps more highs in the tail; FUL overrides"),
        desc(toggle(1, 11, "EVN", "SPACE", "EVEN DECAY", flag!(even_decay)), "Keeps DEC consistent across frequencies, SIZ, DIF and MIX"),
        desc(toggle(1, 12, "FUL", "SPACE", "FULL BAND", flag!(full_band)), "Keeps highs; DMP at 20 kHz bypasses; overrides CUB"),
        desc(unless(value(Rise, 1, 13, "BLD", params::reverb::BUILD, Percent), |p| p.tank != Tank::Plate), "Plate onset speed; raises wet up to 2.5 dB"),
        desc(pick(1, 14, 2, "SPACE", "AGE", choose!(Age::ALL, age), Age::LABELS), "Now clean; 1980s 11 kHz/12-bit; 1970s 6.5 kHz/12-bit at half rate"),
        desc(wide(value(Rise, 2, 0, "DMP", params::reverb::DAMPING, Hertz), 2), "High-frequency damping"),
        desc(wide(open(2, 2, "FILTER", "FILTER", "OPEN", 0, 2), 2), "Two filters on the reverb; opens them on the screen"),
        desc(value(Centre, 2, 5, "BAS", params::reverb::LOW_DECAY, Times), "Decay multiplier below XOV"),
        desc(value(Rise, 2, 6, "XOV", params::reverb::LOW_CROSSOVER, Hertz), "Bass decay crossover"),
        desc(value(Centre, 2, 7, "TRE", params::reverb::HIGH_DECAY, Times), "Decay multiplier above TXO"),
        desc(value(Rise, 2, 8, "TXO", params::reverb::HIGH_CROSSOVER, Hertz), "Treble decay crossover"),
        desc(toggle(2, 9, "STP", "FILTER", "STEEP CROSSOVERS", flag!(steep_crossovers)), "Use 24 dB/octave decay crossovers"),
        desc(wide(value(Centre, 3, 0, "SFT", params::reverb::SHIFT, Semitones), 2), "Shimmer pitch shift"),
        desc(wide(value(Rise, 3, 2, "SHI", params::reverb::SHIMMER, TwoPlaces), 2), "Shimmer feedback level"),
        desc(wide(value(Rise, 3, 7, "PLT", params::reverb::PLATEAU, MillisWhole), 2), "Hold tail level after PRE before decay; 0 is off"),
        desc(unless(value(Rise, 3, 9, "PDR", params::reverb::PLATEAU_DRIFT, Percent), unplateaued), "Plateau drift, reducing comb cancellation; 0 is fixed"),
        desc(held(unless(value(Rise, 3, 10, "PCP", params::reverb::PLATEAU_COPIES, Integer), unplateaued)), "Plateau echo count; fewer gives distinct echoes"),
        desc(unless(value(Centre, 3, 11, "PSH", params::reverb::PLATEAU_SHAPE, Flat), unplateaued), "Plateau envelope: 0 flat, positive swells, negative decays"),
        desc(value(Rise, 4, 0, "STE", params::reverb::STEREO, Percent), "Input stereo preservation; 0 mono sum, 100% original placement"),
        desc(wide(value(Rise, 4, 1, "EDF", params::reverb::INPUT_DIFFUSION, Percent), 2), "Input diffusion before the tail; 0 leaves the onset sharp"),
        desc(unless(value(Centre, 4, 4, "SHP", params::reverb::ATTACK_SHAPE, Flat), |p| p.attack_ms <= 0.0), "Attack envelope: negative immediate then fading, positive swelling"),
        desc(value(Rise, 4, 5, "DSP", params::reverb::DISPERSE, Percent), "Dispersion amount; 0 is off"),
        desc(unless(value(Rise, 4, 6, "FRT", params::reverb::HALL_FRONT, Percent), |p| p.tank != Tank::Hall), "Immediate Hall onset level"),
        desc(unless(value(Rise, 4, 7, "ENT", params::reverb::ENTRY_SPREAD, Percent), |p| p.tank != Tank::Hall), "Hall onset spread over 40 ms; 0 is immediate"),
        desc(value(Rise, 4, 8, "SCT", params::reverb::EARLY_SCATTER, Percent), "Early and Hall-onset stereo spread over 16 ms; 0 is off"),
        desc(unless(pick(5, 0, 2, "SPACE", "EARLY PATTERN", choose!(EarlyPattern::ALL, early_pattern), EarlyPattern::LABELS), |p| p.early_level <= 0.0), "Scatter irregular, Room clustered, Chamber even, Stage wide, Hall discrete"),
        desc(unless(toggle(5, 2, "EOWN", "SPACE", "EARLY OWN LEVEL", flag!(early_own_level)), |p| p.early_level <= 0.0), "Early reflections use ERL independently of FWD"),
        desc(wide(value(Rise, 6, 0, "XFD", params::reverb::CROSS_FEED, Percent), 2), "Opposite-channel feed into the tail; 0 is off"),
        desc(value(Rise, 6, 2, "ECR", params::reverb::EARLY_CROSS, Percent), "Opposite-channel early reflections and free taps; 0 is off"),
        desc(unless(value(Rise, 6, 3, "XMS", params::reverb::CROSS_TIME, MillisTenth), crossed), "Cross-feed delay for XFD and ECR"),
        desc(value(Rise, 6, 5, "HOLD", params::reverb::FADE_AFTER, SecondsOff), "Wait after the last input peak before FALL; 0 is off"),
        desc(unless(value(Rise, 6, 6, "FALL", params::reverb::FADE_RATE, DecibelsPerSecond), |p| p.fade_after_s <= 0.0), "Extra decay after HOLD, in dB/s; FRZ disables it"),
        desc(value(Rise, 6, 7, "MIX", params::reverb::MIXING, Percent), "Tail density buildup; decay time unchanged"),
        desc(wide(value(Centre, 7, 0, "DUC", params::reverb::DUCK_GATE, SignedPercent), 2), "Positive ducks above THR; negative gates open above THR"),
        desc(unless(value(Rise, 7, 2, "THR", params::reverb::DUCK_THRESHOLD, Decibels), unducked), "Duck/gate threshold"),
        desc(unless(value(Rise, 7, 3, "ATK", params::reverb::DUCK_ATTACK, MillisWhole), unducked), "Duck/gate attack"),
        desc(unless(value(Rise, 7, 4, "REL", params::reverb::DUCK_RELEASE, MillisWhole), unducked), "Duck/gate release"),
        desc(value(Rise, 7, 15, "DRY", params::reverb::DRY, Percent), "Dry kept as WET rises; 0 crossfades, 100% keeps full dry"),
    ],
    A,
);

const FILTER1: [Cell<Reverb>; 6] = viewed(filter_cells!(0, "FILTER 1", "the reverb", params::reverb::FILTER1, filter1), A);
const FILTER2: [Cell<Reverb>; 6] = viewed(filter_cells!(1, "FILTER 2", "the reverb", params::reverb::FILTER2, filter2), A);

const TAP1: [Cell<Reverb>; 6] = viewed(tap_cells!(1, 0, "1", params::reverb::TAP1_TIME, params::reverb::TAP1_LEVEL, params::reverb::TAP1_SIDE, params::reverb::TAP1_CROSS, tap1_ms, tap1_sync, tap1_division, tap1_level), B);
const TAP2: [Cell<Reverb>; 6] = viewed(tap_cells!(2, 0, "2", params::reverb::TAP2_TIME, params::reverb::TAP2_LEVEL, params::reverb::TAP2_SIDE, params::reverb::TAP2_CROSS, tap2_ms, tap2_sync, tap2_division, tap2_level), B);
const TAP3: [Cell<Reverb>; 6] = viewed(tap_cells!(3, 0, "3", params::reverb::TAP3_TIME, params::reverb::TAP3_LEVEL, params::reverb::TAP3_SIDE, params::reverb::TAP3_CROSS, tap3_ms, tap3_sync, tap3_division, tap3_level), B);
const TAP4: [Cell<Reverb>; 6] = viewed(tap_cells!(4, 0, "4", params::reverb::TAP4_TIME, params::reverb::TAP4_LEVEL, params::reverb::TAP4_SIDE, params::reverb::TAP4_CROSS, tap4_ms, tap4_sync, tap4_division, tap4_level), B);
const TAP5: [Cell<Reverb>; 6] = viewed(tap_cells!(5, 0, "5", params::reverb::TAP5_TIME, params::reverb::TAP5_LEVEL, params::reverb::TAP5_SIDE, params::reverb::TAP5_CROSS, tap5_ms, tap5_sync, tap5_division, tap5_level), B);

const SCATTER: [Cell<Reverb>; 1] = viewed([desc(unless(value(Rise, 6, 0, "TSC", params::reverb::TAP_SCATTER, Percent), |p| !p.tapping()), "Free-tap stereo spread over 16 ms; 0 is off")], B);

pub(crate) static CELLS: [Cell<Reverb>; 103] = flat(&[&TABS, &HEAD, &MAIN, &FILTER1, &FILTER2, &TAP1, &TAP2, &TAP3, &TAP4, &TAP5, &SCATTER]);
