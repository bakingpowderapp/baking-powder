use super::*;

pub(crate) const CELLS: [Cell<Gate>; 14] = [
    title(1, 2, "SIGNAL"),
    source!(2, "Sidechain source track, muted or not; off, the gate follows its own input"),
    tap!(2, 3),
    title(3, 2, "LEVEL"),
    desc(wide(value(Rise, 4, 2, "THRESHOLD", params::gate::THRESHOLD, Decibels), 3), "Gate opening threshold"),
    desc(wide(value(Rise, 4, 5, "RETURN", params::gate::RETURN, DecibelsUnsigned), 3), "Closing threshold offset below THRESHOLD"),
    desc(wide(value(Rise, 4, 8, "FLOOR", params::gate::FLOOR, Floor), 3), "Closed-gate level; -inf mutes"),
    desc(wide(toggle(4, 11, "FLIP", "GATE", "FLIP", flag!(flip)), 3), "Invert gate: open below THRESHOLD"),
    title(5, 2, "TIME"),
    desc(wide(value(Rise, 6, 2, "ATTACK", params::gate::ATTACK, Millis), 3), "Opening time"),
    desc(wide(value(Rise, 6, 5, "HOLD", params::gate::HOLD, Millis), 3), "Hold time below threshold"),
    desc(wide(value(Rise, 6, 8, "RELEASE", params::gate::RELEASE, Millis), 3), "Closing time after HOLD"),
    title(5, 11, "LOOKAHEAD"),
    desc(pick(6, 11, 3, "GATE", "LOOKAHEAD", choose!(Lookahead::ALL, lookahead), Lookahead::LABELS), "Lookahead"),
];
