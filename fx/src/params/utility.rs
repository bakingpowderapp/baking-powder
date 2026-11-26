use super::*;

choice!(pub LookaheadCurve { Linear = "LINE" / "linear", Smooth = "CURVE" / "smooth" });

params!(Utility {
    number gain_db = 0.0, "gainDb", -24.0..=24.0;
    number balance = 0.0, "pan", -1.0..=1.0;
    number width = 1.0, "width", 0.0..=4.0;
    flag plain = false, "plainWidth";
    flag swap = false, "swap";
    switch limit = false, "limiterOn";
    flag hard = false, "hardClip";
    flag true_peak = false, "truePeak";
    lead ceiling_db = -1.0, "ceilingDb", -24.0..=0.0;
    custom release_db_per_s: f64 = 60.0, "releaseMs", "number 1 to 1000, default 100", |p, d, f| f.number("releaseMs", release_ms(p.release_db_per_s), release_ms(d.release_db_per_s)), |json, d| tidy(6000.0 / number(json, "releaseMs", release_ms(d.release_db_per_s)).clamp(*LIMITER_RELEASE_MS.start(), *LIMITER_RELEASE_MS.end()));
    number hold_ms = 0.0, "holdMs", 0.0..=Utility::MAX_HOLD_MS;
    number tail_ms = 0.0, "tailMs", 0.0..=Utility::MAX_TAIL_MS;
    number tail_share = 0.0, "tailShare", 0.0..=1.0;
    number lookahead_ms = 1.5, "lookaheadMs", Utility::MIN_LOOKAHEAD_MS..=Utility::MAX_LOOKAHEAD_MS;
    choice lookahead_curve: LookaheadCurve = LookaheadCurve::Linear, "lookaheadCurve";
});

impl Utility {
    pub fn lookahead_samples(&self, sample_rate: f64) -> usize {
        ((self.lookahead_ms * 0.001 * sample_rate).round() as usize).max(1)
    }

    pub fn latency(&self, sample_rate: f64) -> usize {
        if self.limit && !self.hard { self.lookahead_samples(sample_rate) - 1 } else { 0 }
    }

    pub const MAX_HOLD_MS: f64 = 10.0;
    pub const MAX_TAIL_MS: f64 = 1000.0;
    pub const MIN_LOOKAHEAD_MS: f64 = 0.1;
    pub const MAX_LOOKAHEAD_MS: f64 = 10.0;
}

table! {
    pub PARAMS: Utility {
        GAIN = "STRIP", "GAIN", field!(gain_db), linear(-24.0, 24.0);
        PAN = "STRIP", "PAN", field!(balance), BIPOLAR;
        WIDTH = "STRIP", "WIDTH", field!(width), linear(0.0, 4.0);
        CEILING = "LIMIT", "CEILING", field!(ceiling_db), linear(-24.0, 0.0);
        RELEASE = "LIMIT", "RELEASE", field!(release_db_per_s), Scale::LimiterRelease, release_ms, release_ms;
        HOLD = "LIMIT", "HOLD", field!(hold_ms), linear(0.0, Utility::MAX_HOLD_MS);
        TAIL = "LIMIT", "TAIL", field!(tail_ms), skewed(0.0, Utility::MAX_TAIL_MS, 3.0);
        SHARE = "LIMIT", "SHARE", field!(tail_share), UNIT;
        LOOKAHEAD = "LIMIT", "LOOKAHEAD", field!(lookahead_ms), log(Utility::MIN_LOOKAHEAD_MS, Utility::MAX_LOOKAHEAD_MS);
    }
}
