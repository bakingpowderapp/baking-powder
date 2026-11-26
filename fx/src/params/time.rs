use super::*;

params!(Time {
    switch play = true, "play";
    lead stop_ms = 200.0, "stopMs", 0.0..=Time::MAX_MS;
    lead start_ms = 200.0, "startMs", 0.0..=Time::MAX_MS;
    lead curve = 1.0, "curve", Time::MIN_CURVE..=Time::MAX_CURVE;
});

impl Time {
    pub const MIN_MS: f64 = 1.0;
    pub const MAX_MS: f64 = 10_000.0;
    pub const MIN_CURVE: f64 = 0.25;
    pub const MAX_CURVE: f64 = 4.0;
    pub const KERNEL_HALF: usize = 8;
    pub const LATENCY: usize = Time::KERNEL_HALF - 1;
}

table! {
    pub PARAMS: Time {
        STOP = "TIME", "STOP", field!(stop_ms), floored(Time::MIN_MS, Time::MAX_MS);
        START = "TIME", "START", field!(start_ms), floored(Time::MIN_MS, Time::MAX_MS);
        CURVE = "TIME", "CURVE", field!(curve), log(Time::MIN_CURVE, Time::MAX_CURVE);
    }
}
