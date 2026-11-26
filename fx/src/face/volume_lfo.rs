use super::*;

pub(crate) const CELLS: [Cell<VolumeLfo>; 5] = [
    title(3, 2, "LFO"),
    desc(unless(pick(4, 2, 4, "LFO", "DIVISION", choose!(&Division::ALL, division), DIVISIONS), |p| !p.sync), "Synced LFO division"),
    desc(unless(wide(value(Rise, 4, 2, "RATE", params::volume_lfo::RATE, LfoHertz), 4), |p| p.sync), "LFO rate"),
    desc(wide(toggle(4, 6, "SYNC", "LFO", "SYNC", flag!(sync)), 4), "Use tempo division instead of RATE"),
    desc(wide(value(Rise, 4, 10, "PHASE", params::volume_lfo::PHASE, Degrees), 4), "Start phase within the drawn shape"),
];
