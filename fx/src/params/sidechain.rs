use super::*;

params!(Sidechain {
    state signal: Signal = Signal::OFF;
    flag peak = true, "peakDetector";
    choice lookahead: Lookahead = Lookahead::Off, "lookahead";
    lead threshold_db = -24.0, "thresholdDb", -60.0..=0.0;
    lead ratio = 4.0, "ratio", 1.0..=20.0;
    number knee_db = 6.0, "kneeDb", 0.0..=18.0;
    number range_db = Sidechain::NO_RANGE_DB, "rangeDb", 1.0..=Sidechain::NO_RANGE_DB;
    lead attack_ms = 1.0, "attackMs", 0.1..=100.0;
    number hold_ms = 0.0, "holdMs", 0.0..=Sidechain::MAX_HOLD_MS;
    lead release_ms = 100.0, "releaseMs", 30.0..=3000.0;
    flag preview = false, "preview";
});

impl Sidechain {
    pub const MAX_HOLD_MS: f64 = 500.0;
    pub const NO_RANGE_DB: f64 = 60.0;
    pub const UNBOUND_DB: f64 = 240.0;

    pub fn range(&self) -> f64 {
        if self.range_db < Sidechain::NO_RANGE_DB { self.range_db } else { Sidechain::UNBOUND_DB }
    }

    pub fn matched(&self, like: &Sidechain) -> Sidechain {
        Sidechain { signal: Signal { source: self.signal.source, ..like.signal }, preview: self.preview, ..*like }
    }
}

table! {
    pub PARAMS: Sidechain {
        THRESHOLD = "DUCK", "THRESHOLD", field!(threshold_db), linear(-60.0, 0.0);
        RATIO = "DUCK", "RATIO", field!(ratio), Scale::Ratio;
        KNEE = "DUCK", "KNEE", field!(knee_db), linear(0.0, 18.0);
        RANGE = "DUCK", "RANGE", field!(range_db), Scale::Range;
        ATTACK = "DUCK", "ATTACK", field!(attack_ms), log(0.1, 100.0);
        HOLD = "DUCK", "HOLD", field!(hold_ms), skewed(0.0, Sidechain::MAX_HOLD_MS, 3.0);
        RELEASE = "DUCK", "RELEASE", field!(release_ms), log(30.0, 3000.0);
    }
}
