use super::*;

pub(crate) const CELLS: [Cell<Space>; 12] = [
    title(2, 2, "SIGNAL"),
    source!(3, "Sidechain source track, muted or not"),
    tap!(3, 2),
    title(2, 8, "RANGE"),
    desc(wide(value(Rise, 3, 8, "LOW", params::space::LOW, Hertz), 3), "Lowest signal/cut frequency"),
    desc(wide(value(Rise, 3, 11, "HIGH", params::space::HIGH, Hertz), 3), "Highest signal/cut frequency"),
    title(4, 2, "CUT"),
    desc(wide(value(Rise, 5, 2, "DEPTH", params::space::DEPTH, DecibelsUnsigned), 3), "Maximum cut at signal level -24 dBFS"),
    desc(wide(value(Rise, 5, 5, "Q", params::space::Q, TwoPlaces), 2), "Cut Q; never narrower than one critical band"),
    desc(wide(value(Rise, 5, 7, "ATTACK", params::space::ATTACK, Millis), 2), "Cut attack with matching lookahead"),
    desc(wide(value(Rise, 5, 9, "RECOVERY", params::space::RECOVERY, Millis), 2), "Release time per 6 dB of reduction"),
    desc(wide(toggle(5, 11, "HEAR", "CUT", "LISTEN", flag!(listen)), 3), "Hear only what is cut"),
];
