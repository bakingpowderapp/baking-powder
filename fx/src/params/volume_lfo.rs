use super::*;

table! {
    pub PARAMS: VolumeLfo {
        RATE = "LFO", "RATE", field!(rate_hz), RATE_HZ;
        PHASE = "LFO", "PHASE", field!(phase), UNIT;
    }
}
