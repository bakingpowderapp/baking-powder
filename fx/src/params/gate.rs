use super::*;

params!(Gate {
    state signal: Signal = Signal::OFF;
    flag flip = false, "flip";
    choice lookahead: Lookahead = Lookahead::Off, "lookahead";
    lead threshold_db = -30.0, "thresholdDb", -60.0..=0.0;
    number return_db = 3.0, "returnDb", 0.0..=24.0;
    lead attack_ms = 1.0, "attackMs", 0.1..=100.0;
    number hold_ms = 10.0, "holdMs", 1.0..=2000.0;
    lead release_ms = 50.0, "releaseMs", 1.0..=3000.0;
    number floor_db = -80.0, "floorDb", -80.0..=0.0;
});

impl Gate {
    pub fn signalled(&self) -> bool {
        self.signal.source >= 0.5
    }
}

table! {
    pub PARAMS: Gate {
        THRESHOLD = "GATE", "THRESHOLD", field!(threshold_db), linear(-60.0, 0.0);
        RETURN = "GATE", "RETURN", field!(return_db), linear(0.0, 24.0);
        ATTACK = "GATE", "ATTACK", field!(attack_ms), log(0.1, 100.0);
        HOLD = "GATE", "HOLD", field!(hold_ms), log(1.0, 2000.0);
        RELEASE = "GATE", "RELEASE", field!(release_ms), log(1.0, 3000.0);
        FLOOR = "GATE", "FLOOR", field!(floor_db), linear(-80.0, 0.0);
    }
}
