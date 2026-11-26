use super::*;

pub(crate) const CELLS: [Cell<Transient>; 8] = [
    title(2, 2, "SHAPE"),
    desc(wide(value(Centre, 3, 2, "ATTACK", params::transient::ATTACK, Decibels), 4), "Lifts or softens the attack of each hit"),
    desc(wide(value(Centre, 3, 6, "SUSTAIN", params::transient::SUSTAIN, Decibels), 4), "Lifts or cuts the tail after each hit"),
    title(4, 2, "TIME"),
    desc(wide(value(Centre, 5, 2, "SPEED", params::transient::SPEED, Percent), 4), "How fast the shaping reacts"),
    title(4, 6, "OUTPUT"),
    desc(wide(toggle(5, 6, "CLIP", "OUTPUT", "CLIP", flag!(clip)), 4), "Keeps peaks under full scale"),
    desc(wide(value(Centre, 5, 10, "LEVEL", params::transient::LEVEL, Decibels), 4), "Level after shaping"),
];
