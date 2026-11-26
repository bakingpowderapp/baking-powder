use super::*;

params!(Compressor {
    flag peak = false, "peakDetector";
    flag sum = false, "sumDetector";
    choice lookahead: Lookahead = Lookahead::Off, "lookahead";
    flag auto = true, "autoReleaseMakeup";
    flag soft_clip = false, "softClip";
    flag branching = false, "branching";
    lead threshold_db = 0.0, "thresholdDb", -60.0..=0.0;
    lead ratio = 4.0, "ratio", 1.0..=20.0;
    number knee_db = 6.0, "kneeDb", 0.0..=18.0;
    lead attack_ms = 10.0, "attackMs", 0.1..=Compressor::MAX_ATTACK_MS;
    lead release_ms = 250.0, "releaseMs", 30.0..=3000.0;
    number range_db = 60.0, "rangeDb", 1.0..=60.0;
    number makeup_db = 0.0, "makeupDb", -24.0..=24.0;
    number dry = 0.0, "dry", 0.0..=1.0;
});

impl Compressor {
    pub const MAX_ATTACK_MS: f64 = 300.0;
}

table! {
    pub PARAMS: Compressor {
        THRESHOLD = "COMPRESS", "THRESHOLD", field!(threshold_db), linear(-60.0, 0.0);
        RATIO = "COMPRESS", "RATIO", field!(ratio), Scale::Ratio;
        KNEE = "COMPRESS", "KNEE", field!(knee_db), linear(0.0, 18.0);
        ATTACK = "COMPRESS", "ATTACK", field!(attack_ms), log(0.1, Compressor::MAX_ATTACK_MS) => log(0.1, 100.0);
        RELEASE = "COMPRESS", "RELEASE", field!(release_ms), log(30.0, 3000.0);
        RANGE = "OUTPUT", "RANGE", field!(range_db), Scale::Range;
        DRY = "OUTPUT", "DRY", field!(dry), UNIT;
        MAKEUP = "OUTPUT", "MAKEUP", field!(makeup_db), linear(-24.0, 24.0);
    }
}
