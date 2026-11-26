use super::*;

pub(crate) const CELLS: [Cell<Utility>; 15] = [
    desc(wide(value(Centre, 2, 2, "GAIN", params::utility::GAIN, Decibels), 2), "Gain"),
    desc(wide(value(Pan, 2, 4, "PAN", params::utility::PAN, Balance), 2), "Stereo balance"),
    desc(wide(value(Rise, 2, 6, "WIDTH", params::utility::WIDTH, Percent), 2), "Stereo width; 0 mono"),
    desc(wide(toggle(2, 8, "SWAP L/R", "STRIP", "SWAP L/R", flag!(swap)), 3), "Swap left/right"),
    desc(wide(toggle(2, 11, "PLAIN WIDTH", "STRIP", "PLAIN WIDTH", flag!(plain)), 3), "Scale sides only; no 80 Hz bass mono or loudness compensation"),
    desc(held(wide(toggle(4, 2, "LIMIT", "LIMIT", "LIMIT", flag!(limit)), 2)), "Enable limiter"),
    desc(inert(wide(value(Rise, 4, 4, "CEILING", params::utility::CEILING, Decibels), 2), |p| !p.limit), "Output ceiling"),
    desc(inert(wide(value(Rise, 4, 6, "RELEASE", params::utility::RELEASE, Release), 2), |p| !p.limit), "Limiter release; first stage with TAIL"),
    desc(inert(wide(value(Rise, 4, 8, "HOLD", params::utility::HOLD, MillisTenth), 2), |p| !p.limit), "Hold the deepest reduction this long; 0 is off"),
    desc(inert(wide(value(Rise, 4, 10, "TAIL", params::utility::TAIL, MillisOffBelowHalf), 2), |p| !p.limit), "Slow release for the SHARE of reduction; 0 is off"),
    desc(inert(wide(value(Rise, 4, 12, "SHARE", params::utility::SHARE, Percent), 2), |p| !p.limit), "Reduction share assigned to TAIL"),
    desc(held(inert(wide(value(Rise, 5, 2, "LOOKAHEAD", params::utility::LOOKAHEAD, MillisTenth), 3), |p| !p.limit)), "Limiter lookahead and latency"),
    desc(inert(wide(switch(5, 5, LookaheadCurve::LABELS, "LIMIT", "LOOKAHEAD CURVE", choose!(LookaheadCurve::ALL, lookahead_curve)), 3), |p| !p.limit), "Lookahead ramp: LINE linear in dB, CURVE smooth"),
    desc(held(inert(wide(toggle(5, 8, "HARD CLIP", "LIMIT", "HARD CLIP", flag!(hard)), 3), |p| !p.limit)), "Hard-clip at the ceiling; no lookahead, release or latency"),
    desc(inert(wide(toggle(5, 11, "TRUE PEAK", "LIMIT", "TRUE PEAK", flag!(true_peak)), 3), |p| !p.limit), "4x true-peak detection; needs about two samples of LOOKAHEAD"),
];
