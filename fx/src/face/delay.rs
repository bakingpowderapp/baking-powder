use super::*;

const A: usize = 0;
const B: usize = 1;
const C: usize = 2;

fn left_tempo(p: &mut Delay, bpm: f64) {
    if p.sync_l {
        p.division_l = Division::nearest(p.time_l_ms, bpm, &Division::ALL);
    } else {
        p.time_l_ms = p.division_l.ms(bpm).clamp(Delay::MIN_TIME, Delay::MAX_TIME);
    }
}

fn right_tempo(p: &mut Delay, bpm: f64) {
    if p.sync_r {
        p.division_r = Division::nearest(p.time_r_ms, bpm, &Division::ALL);
    } else {
        p.time_r_ms = p.division_r.ms(bpm).clamp(Delay::MIN_TIME, Delay::MAX_TIME);
    }
}

fn center_tempo(p: &mut Delay, bpm: f64) {
    if p.sync_c {
        p.division_c = Division::nearest(p.time_c_ms, bpm, &Division::ALL);
    } else {
        p.time_c_ms = p.division_c.ms(bpm).clamp(Delay::MIN_TIME, Delay::MAX_TIME);
    }
}

fn uncentred(p: &Delay) -> bool {
    p.level_c <= 0.0
}

fn splice_tempo(p: &mut Delay, bpm: f64) {
    if p.splice_sync {
        p.splice_division = Division::nearest(p.splice_ms, bpm, &Division::ALL);
    } else {
        p.splice_ms = p.splice_division.ms(bpm).clamp(Delay::MIN_SPLICE, Delay::MAX_SPLICE);
    }
}

fn unspliced(p: &Delay) -> bool {
    p.splice <= 0.0
}

fn splice_synced(p: &Delay) -> bool {
    unspliced(p) || p.splice_sync
}

fn splice_free(p: &Delay) -> bool {
    unspliced(p) || !p.splice_sync
}

fn unducked(p: &Delay) -> bool {
    p.ducking.abs() < f64::EPSILON
}

fn unfollowed(p: &Delay) -> bool {
    !p.following()
}

fn right_keeps_linked_time(p: &mut Delay, _: f64) {
    if !p.link {
        (p.sync_r, p.time_r_ms, p.division_r, p.scale_r) = (p.sync_l, p.time_l_ms, p.division_l, p.scale_l);
    }
}

const TABS: [Cell<Delay>; 3] = [
    desc(page(0, 0, "A", "DELAY", "VIEW A", A), "Time, loop, tone and output"),
    desc(page(0, 1, "B", "DELAY", "VIEW B", B), "Wow and flutter, drive, ducking and follower"),
    desc(page(0, 2, "C", "DELAY", "VIEW C", C), "Pans, the right line, reverse, centre tap and splice"),
];

const TIME: [Cell<Delay>; 15] = viewed(
    [
        title(1, 0, "LEFT"),
        desc(unless(pick(2, 0, 3, "TIME", "LEFT DIVISION", choose!(&Division::ALL, division_l), DIVISIONS), |p| !p.sync_l), "Synced left division"),
        desc(unless(wide(value(Rise, 2, 0, "TIME", params::delay::LEFT_TIME, MillisWhole), 3), |p| p.sync_l), "Left delay time"),
        desc(tempo(toggle(2, 3, "SYNC", "TIME", "LEFT SYNC", flag!(sync_l)), left_tempo), "Sync left delay to tempo"),
        desc(unless(wide(value(Rise, 2, 4, "SCALE", params::delay::LEFT_SCALE, Times), 2), |p| !p.sync_l), "Multiply the synced left division"),
        desc(tempo(wide(toggle(2, 7, "LINK", "TIME", "LINK", flag!(link)), 2), right_keeps_linked_time), "Right time follows the left; unlinked, it keeps the linked time"),
        title(1, 10, "RIGHT"),
        desc(unless(pick(2, 10, 3, "TIME", "RIGHT DIVISION", choose!(&Division::ALL, division_r), DIVISIONS), |p| p.link || !p.sync_r), "Synced right division"),
        desc(unless(wide(value(Rise, 2, 10, "TIME", params::delay::RIGHT_TIME, MillisWhole), 3), |p| p.link || p.sync_r), "Right delay time"),
        desc(unless(pick(2, 10, 3, "TIME", "LEFT DIVISION", choose!(&Division::ALL, division_l), DIVISIONS), |p| !p.link || !p.sync_l), "Linked: the right follows the left division"),
        desc(unless(wide(value(Rise, 2, 10, "TIME", params::delay::LEFT_TIME, MillisWhole), 3), |p| !p.link || p.sync_l), "Linked: the right follows the left time"),
        desc(tempo(unless(toggle(2, 13, "SYNC", "TIME", "RIGHT SYNC", flag!(sync_r)), |p| p.link), right_tempo), "Sync right delay to tempo"),
        desc(tempo(unless(toggle(2, 13, "SYNC", "TIME", "LEFT SYNC", flag!(sync_l)), |p| !p.link), left_tempo), "Linked: the right follows the left sync"),
        desc(unless(wide(value(Rise, 2, 14, "SCALE", params::delay::RIGHT_SCALE, Times), 2), |p| p.link || !p.sync_r), "Multiply the synced right division"),
        desc(unless(wide(value(Rise, 2, 14, "SCALE", params::delay::LEFT_SCALE, Times), 2), |p| !p.link || !p.sync_l), "Linked: the right follows the left scale"),
    ],
    A,
);

const LOOP: [Cell<Delay>; 8] = viewed(
    [
        title(3, 0, "LOOP"),
        desc(wide(value(Rise, 4, 0, "FEEDBACK", params::delay::FEEDBACK, Percent), 3), "Left feedback; right inherits until R FEEDBACK is set"),
        desc(wide(open(4, 3, "FB FILTER", "FEEDBACK FILTER", "OPEN", 2, 1), 3), "A filter on the repeats only; opens it on the screen"),
        desc(wide(toggle(4, 7, "FREEZE", "LOOP", "FREEZE", flag!(freeze)), 2), "Freeze delay and mute input"),
        title(3, 10, "PING-PONG"),
        desc(wide(value(Rise, 4, 10, "AMOUNT", params::delay::PING_PONG, Percent), 3), "Left-to-right feedback share; right inherits until R PING-PONG is set"),
        desc(inert(toggle(4, 13, "EVEN", "LOOP", "EVEN PING-PONG", flag!(even)), |p| p.ping_pong <= 0.0), "Equal-level ping-pong pairs from mono input"),
        desc(wide(value(Pan, 4, 14, "LEAN", params::delay::LEAN, Balance), 2), "Input left/right balance"),
    ],
    A,
);

const TONE: [Cell<Delay>; 9] = viewed(
    [
        title(5, 0, "TONE"),
        desc(wide(open(6, 0, "FILTER", "FILTER", "OPEN", 0, 2), 3), "Two filters on every echo; opens them on the screen"),
        desc(wide(unless(value(Rise, 6, 3, "DRIVE", params::delay::DRIVE, Percent), |p| p.mode == DelayMode::Clean), 3), "Tape/BBD loop saturation"),
        title(5, 7, "MODE"),
        desc(pick(6, 7, 2, "TIME", "MODE", choose!(DelayMode::ALL, mode), DelayMode::LABELS), "Echo mode: clean, tape or BBD"),
        title(5, 10, "OUTPUT"),
        desc(wide(value(Rise, 6, 10, "WIDTH", params::delay::WIDTH, Percent), 2), "Echo width"),
        desc(wide(value(Rise, 6, 12, "TRIM", params::delay::WET_TRIM, Decibels), 2), "Wet trim"),
        desc(wide(value(Rise, 6, 14, "DRY", params::delay::DRY, Percent), 2), "Dry kept as WET rises; 0 crossfades, 100% keeps full dry"),
    ],
    A,
);

const FILTER1: [Cell<Delay>; 6] = viewed(filter_cells!(0, "FILTER 1", "every echo", params::delay::FILTER1, filter1), A);
const FILTER2: [Cell<Delay>; 6] = viewed(filter_cells!(1, "FILTER 2", "every echo", params::delay::FILTER2, filter2), A);
const FEEDBACK_FILTER: [Cell<Delay>; 6] = viewed(filter_cells!(2, "FEEDBACK FILTER", "the repeats only, sparing the first echo", params::delay::FEEDBACK_FILTER, feedback_filter), A);

const MORE_TONE: [Cell<Delay>; 11] = viewed(
    [
        title(0, 3, "TONE"),
        desc(wide(value(Rise, 1, 0, "MOD DEPTH", params::delay::MOD_DEPTH, Percent), 3), "Wow/flutter depth"),
        desc(wide(value(Rise, 1, 3, "RATE", params::delay::MOD_RATE, LfoHertz), 3), "Wow rate; flutter stays at 6 Hz"),
        desc(wide(value(Rise, 1, 6, "FLUTTER", params::delay::MOD_FLUTTER, Percent), 2), "6 Hz flutter amount; 0 leaves wow alone"),
        desc(wide(value(Centre, 1, 8, "SPREAD", params::delay::MOD_SPREAD, Degrees), 2), "Right wow phase relative to left"),
        desc(wide(value(Rise, 1, 10, "PHASE", params::delay::PHASE, Degrees), 2), "Wow phase at playback start"),
        desc(wide(value(Rise, 2, 0, "INPUT DRIVE", params::delay::INPUT_DRIVE, Percent), 3), "Input saturation before delay; 0 is off"),
        desc(wide(value(Rise, 2, 3, "WET DRIVE", params::delay::WET_DRIVE, Percent), 3), "Wet saturation after the loop; 0 is off"),
        desc(wide(value(Rise, 2, 6, "DIFFUSE", params::delay::DIFFUSE, Percent), 2), "Echo diffusion"),
        desc(wide(value(Rise, 2, 8, "CEILING", params::delay::FEEDBACK_CEILING, TwoPlaces), 2), "Maximum feedback gain"),
        desc(wide(toggle(2, 10, "SOFT", "LOOP", "SOFT LOOP", flag!(loop_soft)), 2), "Soft-limit feedback near full scale"),
    ],
    B,
);

const DUCK: [Cell<Delay>; 5] = viewed(
    [
        title(3, 0, "DUCK"),
        desc(wide(value(Centre, 4, 0, "AMOUNT", params::delay::DUCK_GATE, SignedPercent), 3), "Positive ducks above THRESHOLD; negative gates open above THRESHOLD"),
        desc(wide(unless(value(Rise, 4, 3, "THRESHOLD", params::delay::THRESHOLD, Decibels), unducked), 3), "Duck/gate threshold"),
        desc(wide(unless(value(Rise, 4, 6, "ATTACK", params::delay::ATTACK, MillisWhole), unducked), 2), "Duck/gate attack"),
        desc(wide(unless(value(Rise, 4, 8, "RELEASE", params::delay::RELEASE, MillisWhole), unducked), 2), "Duck/gate release"),
    ],
    B,
);

const FOLLOW: [Cell<Delay>; 11] = viewed(
    [
        title(5, 0, "FOLLOW"),
        desc(wide(value(Centre, 6, 0, "FEEDBACK", params::delay::FOLLOW_FEEDBACK, TwoPlaces), 3), "Left feedback gain added at full follower; total clamps to 0-2"),
        desc(wide(value(Centre, 6, 3, "R FEEDBACK", params::delay::FOLLOW_RIGHT_FEEDBACK, TwoPlaces), 3), "Right feedback gain added at full follower"),
        desc(wide(value(Centre, 6, 6, "CROSS", params::delay::FOLLOW_CROSS, TwoPlaces), 2), "Left-to-right cross-feed gain added at full follower"),
        desc(wide(value(Centre, 6, 8, "R CROSS", params::delay::FOLLOW_RIGHT_CROSS, TwoPlaces), 2), "Right-to-left cross-feed gain added at full follower"),
        desc(wide(value(Centre, 6, 10, "WET", params::delay::FOLLOW_WET, TwoPlaces), 2), "Wet gain added at full follower; total never below 0"),
        desc(wide(value(Centre, 6, 12, "DRY", params::delay::FOLLOW_DRY, TwoPlaces), 2), "Dry gain added at full follower; total clamps to 0-1"),
        desc(wide(unless(value(Rise, 7, 0, "THRESHOLD", params::delay::FOLLOW_THRESHOLD, Decibels), unfollowed), 3), "Follower threshold; full at THRESHOLD + RANGE"),
        desc(wide(unless(value(Rise, 7, 3, "RANGE", params::delay::FOLLOW_RANGE, DecibelsUnsigned), unfollowed), 3), "Follower range above THRESHOLD"),
        desc(wide(unless(value(Rise, 7, 6, "ATTACK", params::delay::FOLLOW_ATTACK, MillisWhole), unfollowed), 2), "Follower attack; 0 is immediate"),
        desc(wide(unless(value(Rise, 7, 8, "RELEASE", params::delay::FOLLOW_RELEASE, MillisWhole), unfollowed), 2), "Follower release"),
    ],
    B,
);

const STEREO: [Cell<Delay>; 9] = viewed(
    [
        title(0, 3, "STEREO"),
        desc(wide(value(Pan, 1, 0, "LEFT PAN", params::delay::LEFT_PAN, Balance), 3), "Left echo pan before width; centered level follows BALANCE"),
        desc(wide(value(Pan, 1, 3, "RIGHT PAN", params::delay::RIGHT_PAN, Balance), 3), "Right echo pan before width; centered level follows BALANCE"),
        desc(wide(toggle(1, 6, "BALANCE", "OUTPUT", "LINE PAN BALANCE", flag!(pan_balance)), 3), "Balance pan: centered lines at full level; off, half level"),
        desc(wide(value(Pan, 1, 9, "WET PAN", params::delay::WET_PAN, Balance), 3), "Wet pan after width; dry unchanged"),
        desc(wide(value(Rise, 2, 0, "R FEEDBACK", params::delay::RIGHT_FEEDBACK, Percent), 3), "Right feedback; inherits FEEDBACK until set"),
        desc(wide(value(Rise, 2, 3, "R PING-PONG", params::delay::RIGHT_PING_PONG, Percent), 3), "Right-to-left feedback share; inherits PING-PONG until set"),
        desc(inert(wide(toggle(2, 6, "EVEN STEREO", "LOOP", "EVEN STEREO", flag!(even_stereo)), 3), |p| !p.even || p.ping_pong <= 0.0), "Keep stereo input in EVEN mode"),
        desc(inert(wide(toggle(2, 9, "EVEN SWAY", "LOOP", "EVEN SWAY", flag!(even_sway)), 3), |p| !p.even || p.ping_pong <= 0.0 || p.mod_depth <= 0.0), "One modulation pass per EVEN pair; off, both lines modulate"),
    ],
    C,
);

const LINES: [Cell<Delay>; 9] = viewed(
    [
        title(3, 0, "LINES"),
        desc(wide(value(Rise, 4, 0, "FORWARD", params::delay::FORWARD, Percent), 3), "Forward echo level"),
        desc(wide(value(Rise, 4, 3, "REVERSE", params::delay::REVERSE, Percent), 3), "Reverse echo level before notes"),
        title(3, 7, "CENTER TAP"),
        desc(wide(value(Rise, 4, 7, "LEVEL", params::delay::CENTER_LEVEL, Percent), 3), "Centre tap level, a third line fed from the input; 0 is off"),
        desc(tempo(unless(toggle(4, 10, "SYNC", "TIME", "CENTER SYNC", flag!(sync_c)), uncentred), center_tempo), "Sync the centre tap to tempo"),
        desc(unless(pick(4, 11, 3, "TIME", "CENTER DIVISION", choose!(&Division::ALL, division_c), DIVISIONS), |p| uncentred(p) || !p.sync_c), "Synced centre division"),
        desc(unless(wide(value(Rise, 4, 11, "TIME", params::delay::CENTER_TIME, MillisWhole), 3), |p| uncentred(p) || p.sync_c), "Centre tap time"),
        desc(wide(unless(value(Pan, 4, 14, "PAN", params::delay::CENTER_PAN, Balance), uncentred), 2), "Centre tap pan"),
    ],
    C,
);

const SPLICE: [Cell<Delay>; 14] = viewed(
    [
        title(5, 0, "SPLICE"),
        desc(wide(value(Rise, 6, 0, "MIX", params::delay::MIX, Percent), 3), "Splice/plain echo blend; both feed back"),
        desc(wide(unless(value(Centre, 6, 3, "PITCH", params::delay::PITCH, Semitones), unspliced), 2), "Splice pitch shift, +/-36 semitones"),
        desc(wide(unless(value(Centre, 6, 5, "DETUNE", params::delay::DETUNE, SignedCents), unspliced), 2), "Splice detune: left down, right up, up to 4800 cents"),
        desc(tempo(unless(toggle(6, 7, "SYNC", "SPLICE", "SYNC", flag!(splice_sync)), unspliced), splice_tempo), "Sync splice length and starts to tempo"),
        desc(unless(pick(6, 8, 3, "SPLICE", "DIVISION", choose!(&Division::ALL, splice_division), DIVISIONS), splice_free), "Synced splice division; pieces cap at 2 s"),
        desc(unless(wide(value(Rise, 6, 8, "LENGTH", params::delay::LENGTH, MillisWhole), 3), splice_synced), "Splice length; adds about LENGTH delay, or half with SWEEP"),
        desc(wide(unless(value(Rise, 6, 11, "SMOOTH", params::delay::SMOOTHING, MillisWhole), unspliced), 2), "Splice crossfade, at most half LENGTH, or LENGTH with SWEEP"),
        desc(wide(unless(value(Rise, 6, 13, "OFFSET", params::delay::RIGHT_OFFSET, Percent), unspliced), 2), "Shorten right splice by this fraction"),
        desc(wide(unless(toggle(7, 0, "REVERSE", "SPLICE", "REVERSE", flag!(splice_reverse)), unspliced), 3), "Reverse each splice"),
        desc(wide(unless(toggle(7, 3, "HOLD", "SPLICE", "HOLD", flag!(hold)), unspliced), 2), "Loop current splice and suspend feedback"),
        desc(wide(unless(toggle(7, 5, "SWEEP", "SPLICE", "SWEEP", flag!(sweep)), unspliced), 2), "Continuous pitch/reverse sweep across LENGTH; no gaps between splices"),
        desc(wide(unless(toggle(7, 7, "CUTS AFTER", "SPLICE", "CUTS AFTER", flag!(cuts_after)), unspliced), 3), "Apply filters 1 and 2 after pitch shifting; off, before shifting"),
        desc(wide(unless(toggle(7, 10, "ALIASING", "SPLICE", "ALIASING", flag!(aliasing)), unspliced), 3), "Allow splice aliasing; off, band-limit to playback speed"),
    ],
    C,
);

pub(crate) static CELLS: [Cell<Delay>; 112] = flat(&[&TABS, &TIME, &LOOP, &TONE, &FILTER1, &FILTER2, &FEEDBACK_FILTER, &MORE_TONE, &DUCK, &FOLLOW, &STEREO, &LINES, &SPLICE]);
