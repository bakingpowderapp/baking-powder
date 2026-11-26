use super::*;

params!(Space {
    state signal: Signal = Signal::OFF;
    lead depth_db = 12.0, "depthDb", 0.0..=crate::space::engine::DEPTH_MAX_DB;
    lead q = 4.0, "q", 1.0..=16.0;
    lead attack_ms = 5.0, "attackMs", 0.1..=10.0;
    lead recovery_ms = 60.0, "recoveryMs", 5.0..=1000.0;
    number low_hz = 20.0, "lowHz", 20.0..=15_000.0;
    number high_hz = 20_000.0, "highHz", 50.0..=20_000.0;
    flag listen = false, "listen";
});

table! {
    pub PARAMS: Space {
        DEPTH = "CUT", "DEPTH", field!(depth_db), linear(0.0, crate::space::engine::DEPTH_MAX_DB);
        Q = "CUT", "Q", field!(q), log(1.0, 16.0);
        ATTACK = "CUT", "ATTACK", field!(attack_ms), log(0.1, 10.0);
        RECOVERY = "CUT", "RECOVERY", field!(recovery_ms), log(5.0, 1000.0);
        LOW = "RANGE", "LOW" seeded "KEY", "LOW", field!(low_hz), log(20.0, 15_000.0);
        HIGH = "RANGE", "HIGH" seeded "KEY", "HIGH", field!(high_hz), log(50.0, 20_000.0);
    }
}
