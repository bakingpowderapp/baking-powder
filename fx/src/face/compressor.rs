use super::*;

pub(crate) const CELLS: [Cell<Compressor>; 19] = [
    title(1, 2, "DETECT"),
    desc(wide(switch(2, 2, &["RMS", "PEAK"], "DETECT", "DETECTOR", flag!(peak)), 3), "PEAK or 10 ms RMS detection"),
    desc(wide(switch(2, 5, &["MAX", "SUM"], "DETECT", "STEREO", flag!(sum)), 3), "SUM sums channels; MAX uses louder channel"),
    desc(wide(toggle(2, 8, "BRANCHING", "DETECT", "BRANCHING", flag!(branching)), 3), "Separate attack and release smoothing; off, ATTACK smooths both"),
    title(1, 11, "LOOKAHEAD"),
    desc(pick(2, 11, 3, "DETECT", "LOOKAHEAD", choose!(Lookahead::ALL, lookahead), Lookahead::LABELS), "Lookahead"),
    title(3, 2, "COMPRESS"),
    desc(wide(value(Rise, 4, 2, "THRESHOLD", params::compressor::THRESHOLD, Decibels), 3), "Compression threshold"),
    desc(wide(value(Rise, 4, 5, "RATIO", params::compressor::RATIO, Ratio), 3), "Compression ratio"),
    desc(wide(value(Rise, 4, 8, "KNEE", params::compressor::KNEE, DecibelsUnsigned), 3), "Soft-knee width"),
    desc(wide(value(Rise, 4, 11, "RANGE", params::compressor::RANGE, Range), 3), "Maximum reduction; 60 dB disables cap"),
    title(5, 2, "TIME"),
    desc(wide(value(Rise, 6, 2, "ATTACK", params::compressor::ATTACK, Millis), 2), "Attack"),
    desc(inert(wide(value(Rise, 6, 4, "RELEASE", params::compressor::RELEASE, Millis), 2), |p| p.auto), "Release"),
    desc(wide(toggle(6, 6, "AUTO", "DETECT", "AUTO", flag!(auto)), 2), "Automatic release and makeup"),
    title(5, 8, "OUTPUT"),
    desc(inert(wide(value(Centre, 6, 8, "MAKEUP", params::compressor::MAKEUP, Decibels), 2), |p| p.auto), "Makeup gain"),
    desc(wide(toggle(6, 10, "CLIP", "OUTPUT", "SOFT CLIP", flag!(soft_clip)), 2), "Post-makeup saturation"),
    desc(wide(value(Rise, 6, 12, "DRY", params::compressor::DRY, Percent), 2), "Dry kept as WET rises; 0 crossfades, 100% keeps full dry"),
];
