use super::*;

fn unsignalled(p: &Sidechain) -> bool {
    p.signal.source < 0.5
}

pub(crate) const CELLS: [Cell<Sidechain>; 17] = [
    title(0, 2, "SIGNAL"),
    source!(1, "Sidechain source track, muted or not"),
    tap!(1, 3),
    desc(wide(seeded(switch(1, 9, &["RMS", "PEAK"], "SIGNAL", "DETECTOR", flag!(peak)), "KEY", "DETECTOR"), 3), "PEAK or 10 ms RMS detection"),
    desc(pick(1, 12, 2, "SIGNAL", "LOOKAHEAD", choose!(Lookahead::ALL, lookahead), Lookahead::LABELS), "Lookahead"),
    title(2, 2, "LEVEL"),
    desc(wide(value(Rise, 3, 2, "THRESHOLD", params::sidechain::THRESHOLD, Decibels), 3), "Signal threshold"),
    desc(wide(value(Rise, 3, 5, "RATIO", params::sidechain::RATIO, Ratio), 3), "Ducking ratio"),
    desc(wide(value(Rise, 3, 8, "KNEE", params::sidechain::KNEE, DecibelsUnsigned), 3), "Soft-knee width"),
    desc(wide(value(Rise, 3, 11, "RANGE", params::sidechain::RANGE, Range), 3), "Deepest duck; off sets no limit"),
    title(4, 2, "TIME"),
    desc(wide(value(Rise, 5, 2, "ATTACK", params::sidechain::ATTACK, Millis), 4), "Ducking attack"),
    desc(wide(value(Rise, 5, 6, "HOLD", params::sidechain::HOLD, MillisOffBelowHalf), 4), "Time the duck holds before RELEASE"),
    desc(wide(value(Rise, 5, 10, "RELEASE", params::sidechain::RELEASE, Millis), 4), "Ducking release"),
    desc(held(unless(wide(toggle(6, 2, "PREVIEW", "DUCK", "PREVIEW", flag!(preview)), 4), unsignalled)), "Draws the ducking over the track as its volume"),
    desc(unless(wide(button(6, 6, "FLATTEN", "DUCK", "FLATTEN", Act::Flatten), 4), unsignalled), "Prints the ducking into the track's volume lane and turns this row off"),
    desc(unless(wide(button(6, 10, "MATCH ALL", "SIGNAL", "MATCH ALL", Act::Match), 4), unsignalled), "Copies these settings to every Sidechain with the same SOURCE"),
];
