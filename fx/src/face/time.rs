use super::*;

pub(crate) const CELLS: [Cell<Time>; 6] = [
    title(3, 2, "PLAY"),
    desc(wide(switch(4, 2, &["STOP", "PLAY"], "TIME", "PLAY", flag!(play)), 3), "STOP slows down; PLAY speeds back up to live input"),
    title(3, 5, "RAMP"),
    desc(wide(value(Rise, 4, 5, "STOP", params::time::STOP, Millis), 3), "Time to stop, lowering pitch and fading out"),
    desc(wide(value(Rise, 4, 8, "START", params::time::START, Millis), 3), "Time to regain full speed; 0 is immediate"),
    desc(wide(value(Rise, 4, 11, "CURVE", params::time::CURVE, TwoPlaces), 3), "Speed curve: 1 linear, lower lingers slow, higher stays fast"),
];
