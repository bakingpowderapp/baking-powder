use super::*;

choice!(pub Tank { Plate = "Plate" / "plate", Hall = "Hall" / "hall" });
choice!(pub EarlyPattern { Scatter = "Scatter" / "scatter", Room = "Room" / "room", Chamber = "Chamber" / "chamber", Stage = "Stage" / "stage", Hall = "Hall" / "hall" });
choice!(pub Age { Now = "Now" / "now", Eighties = "1980s" / "1980s", Seventies = "1970s" / "1970s" });

params!(Reverb {
    pick tank: Tank = Tank::Plate, "tank";
    lead pre_delay_ms = 20.0, "preDelayMs", 0.0..=Reverb::MAX_PRE_DELAY_MS;
    number attack_ms = 0.0, "attackMs", 0.0..=Reverb::MAX_ATTACK_MS;
    number early_level = 0.0, "earlyLevel", 0.0..=1.0;
    number early_span_ms = 40.0, "earlySpanMs", Reverb::MIN_EARLY_SPAN_MS..=Reverb::MAX_EARLY_SPAN_MS;
    number early_width = 1.0, "earlyWidth", 0.0..=1.0;
    pick early_pattern: EarlyPattern = EarlyPattern::Scatter, "earlyPattern";
    flag early_own_level = false, "earlyOwnLevel";
    number input_diffusion = 1.0, "inputDiffusion", 0.0..=1.0;
    number hall_front = Reverb::HALL_FRONT, "hallFront", 0.0..=1.0;
    number attack_shape = 0.0, "attackShape", -1.0..=1.0;
    pick age: Age = Age::Now, "age";
    number disperse = 0.0, "disperse", 0.0..=1.0;
    number stereo = 0.0, "stereoInput", 0.0..=1.0;
    lead decay_s = 1.8, "decayS", Reverb::MIN_DECAY_S..=60.0;
    lead size = 1.0, "sizeTimes", 0.1..=Reverb::MAX_SIZE;
    number diffusion = 0.5, "diffusion", 0.0..=0.7;
    number damping_hz = 7000.0, "dampingHz", Reverb::MIN_DAMPING_HZ..=20_000.0;
    number low_decay = 1.0, "lowDecayTimes", 0.25..=Reverb::MAX_LOW_DECAY;
    number low_crossover_hz = 300.0, "lowCrossoverHz", 100.0..=2000.0;
    number high_decay = 1.0, "highDecayTimes", 0.25..=4.0;
    number high_crossover_hz = 4000.0, "highCrossoverHz", 1000.0..=16_000.0;
    number mod_depth = 4.0 / 48.0, "modDepthMs", 0.0..=Reverb::MAX_MOD_DEPTH_MS;
    switch mod_sync = false, "modSync";
    pick mod_division: Division = Division::Quarter, "modDivision";
    lead mod_rate_hz = 0.7, "modRateHz", RATE_MIN_HZ..=RATE_MAX_HZ;
    number mod_random = 0.0, "modRandom", 0.0..=1.0;
    nested filter1: Filter = Filter::LOW_CUT, "filter1";
    nested filter2: Filter = Filter::HIGH_CUT, "filter2";
    number width = 1.0, "width", 0.0..=4.0;
    number dry = 0.0, "dry", 0.0..=1.0;
    flag freeze = false, "freeze";
    number forward = 1.0, "forwardLevel", 0.0..=1.0;
    number reverse = 0.0, "reverseLevel", 0.0..=1.0;
    number start_phase = 0.0, "modStartPhase", 0.0..=1.0;
    number shift = 12.0, "shiftSemitones", -24.0..=24.0;
    number shimmer = 0.0, "shimmer", 0.0..=0.95;
    number ducking = 0.0, "duckGate", -1.0..=1.0;
    number threshold_db = -30.0, "duckThresholdDb", -60.0..=0.0;
    number duck_attack_ms = 5.0, "duckAttackMs", 1.0..=500.0;
    number duck_release_ms = 200.0, "duckReleaseMs", 10.0..=2000.0;
    number fade_after_s = 0.0, "fadeAfterS", 0.0..=Reverb::MAX_FADE_AFTER_S;
    number fade_db_per_s = 30.0, "fadeDbPerS", Reverb::MIN_FADE_DB_PER_S..=Reverb::MAX_FADE_DB_PER_S;
    number tap1_ms = 10.0, "tap1Ms", 0.0..=Reverb::MAX_TAP_MS;
    flag tap1_sync = false, "tap1Sync";
    choice tap1_division: Division = Division::Quarter, "tap1Division";
    number tap1_level = 0.0, "tap1Level", 0.0..=1.0;
    number tap1_side = 0.0, "tap1Side", -1.0..=1.0;
    number tap2_ms = 20.0, "tap2Ms", 0.0..=Reverb::MAX_TAP_MS;
    flag tap2_sync = false, "tap2Sync";
    choice tap2_division: Division = Division::Quarter, "tap2Division";
    number tap2_level = 0.0, "tap2Level", 0.0..=1.0;
    number tap2_side = 0.0, "tap2Side", -1.0..=1.0;
    number tap3_ms = 40.0, "tap3Ms", 0.0..=Reverb::MAX_TAP_MS;
    flag tap3_sync = false, "tap3Sync";
    choice tap3_division: Division = Division::Quarter, "tap3Division";
    number tap3_level = 0.0, "tap3Level", 0.0..=1.0;
    number tap3_side = 0.0, "tap3Side", -1.0..=1.0;
    number tap4_ms = 80.0, "tap4Ms", 0.0..=Reverb::MAX_TAP_MS;
    flag tap4_sync = false, "tap4Sync";
    choice tap4_division: Division = Division::Quarter, "tap4Division";
    number tap4_level = 0.0, "tap4Level", 0.0..=1.0;
    number tap4_side = 0.0, "tap4Side", -1.0..=1.0;
    number early_scatter = 0.0, "earlyScatter", 0.0..=1.0;
    flag cubic_lines = false, "cubicLines";
    flag steep_crossovers = false, "steepCrossovers";
    number mixing = 1.0, "mixing", 0.0..=1.0;
    number plateau_ms = 0.0, "plateauMs", 0.0..=Reverb::MAX_PLATEAU_MS;
    number mod_sparse = 0.0, "modSparse", 0.0..=1.0;
    number tap_scatter = 0.0, "tapScatter", 0.0..=1.0;
    number entry_spread = 0.0, "entrySpread", 0.0..=1.0;
    number tap5_ms = 160.0, "tap5Ms", 0.0..=Reverb::MAX_TAP_MS;
    flag tap5_sync = false, "tap5Sync";
    choice tap5_division: Division = Division::Quarter, "tap5Division";
    number tap5_level = 0.0, "tap5Level", 0.0..=1.0;
    number tap5_side = 0.0, "tap5Side", -1.0..=1.0;
    number tap1_cross = 0.0, "tap1Cross", 0.0..=1.0;
    number tap2_cross = 0.0, "tap2Cross", 0.0..=1.0;
    number tap3_cross = 0.0, "tap3Cross", 0.0..=1.0;
    number tap4_cross = 0.0, "tap4Cross", 0.0..=1.0;
    number tap5_cross = 0.0, "tap5Cross", 0.0..=1.0;
    number cross_feed = 0.0, "crossFeed", 0.0..=1.0;
    number early_cross = 0.0, "earlyCross", 0.0..=1.0;
    number cross_ms = 10.0, "crossMs", 0.0..=Reverb::MAX_CROSS_MS;
    number plateau_drift = 0.0, "plateauDrift", 0.0..=1.0;
    whole plateau_copies: usize = Reverb::PLATEAU_COPIES, "plateauCopies", Reverb::MIN_PLATEAU_COPIES as f64..=Reverb::PLATEAU_COPIES as f64;
    number plateau_shape = 0.0, "plateauShape", -1.0..=1.0;
    number build = 0.0, "build", 0.0..=1.0;
    flag full_band = false, "fullBand";
    flag even_decay = false, "evenDecay";
});

impl Reverb {
    pub const MAX_PRE_DELAY_MS: f64 = 500.0;
    pub const MAX_MOD_DEPTH_MS: f64 = 20.0;
    pub const MAX_SIZE: f64 = 10.0;
    pub const MAX_ATTACK_MS: f64 = 3000.0;
    pub const MIN_EARLY_SPAN_MS: f64 = 10.0;
    pub const MAX_EARLY_SPAN_MS: f64 = 400.0;
    pub const MIN_DECAY_S: f64 = 0.03;
    pub const MIN_DAMPING_HZ: f64 = 300.0;
    pub const MAX_LOW_DECAY: f64 = 8.0;
    pub const HALL_FRONT: f64 = 0.3;
    pub const MAX_FADE_AFTER_S: f64 = 10.0;
    pub const MIN_FADE_DB_PER_S: f64 = 10.0;
    pub const MAX_FADE_DB_PER_S: f64 = 120.0;
    pub const MAX_TAP_MS: f64 = 2000.0;
    pub const TAPS: usize = 5;
    pub const FILTERS: usize = 2;
    pub const MAX_PLATEAU_MS: f64 = 500.0;
    pub const MAX_CROSS_MS: f64 = 80.0;
    pub const OPEN_HZ: f64 = 20_000.0;
    pub const PLATEAU_COPIES: usize = 48;
    pub const MIN_PLATEAU_COPIES: usize = 4;

    pub fn taps(&self) -> [(f64, Option<Division>, f64, f64, f64); Reverb::TAPS] {
        [
            (self.tap1_ms, self.tap1_sync.then_some(self.tap1_division), self.tap1_level, self.tap1_side, self.tap1_cross),
            (self.tap2_ms, self.tap2_sync.then_some(self.tap2_division), self.tap2_level, self.tap2_side, self.tap2_cross),
            (self.tap3_ms, self.tap3_sync.then_some(self.tap3_division), self.tap3_level, self.tap3_side, self.tap3_cross),
            (self.tap4_ms, self.tap4_sync.then_some(self.tap4_division), self.tap4_level, self.tap4_side, self.tap4_cross),
            (self.tap5_ms, self.tap5_sync.then_some(self.tap5_division), self.tap5_level, self.tap5_side, self.tap5_cross),
        ]
    }

    pub fn tapping(&self) -> bool {
        self.taps().iter().any(|(_, _, level, _, _)| *level > 0.0)
    }

    pub fn filters(&self) -> [Filter; Reverb::FILTERS] {
        [self.filter1, self.filter2]
    }
}

impl Filtered for Reverb {
    fn sites(&self) -> &'static [&'static [usize]] {
        &[&[0, 1]]
    }

    fn filter(&self, unit: usize) -> &Filter {
        if unit == 0 { &self.filter1 } else { &self.filter2 }
    }

    fn filter_mut(&mut self, unit: usize) -> &mut Filter {
        if unit == 0 { &mut self.filter1 } else { &mut self.filter2 }
    }
}

pub const FILTER1: FilterKnobs<Reverb> = filter_params!("FILTER 1", filter1);
pub const FILTER2: FilterKnobs<Reverb> = filter_params!("FILTER 2", filter2);

table! {
    LISTED: Reverb {
        PRE_DELAY = "SPACE", "PRE-DELAY", field!(pre_delay_ms), linear(0.0, Reverb::MAX_PRE_DELAY_MS);
        ATTACK = "SPACE", "ATTACK", field!(attack_ms), skewed(0.0, Reverb::MAX_ATTACK_MS, 3.0);
        EARLY = "SPACE", "EARLY", field!(early_level), UNIT;
        EARLY_SPAN = "SPACE", "EARLY SPAN", field!(early_span_ms), log(Reverb::MIN_EARLY_SPAN_MS, Reverb::MAX_EARLY_SPAN_MS) => log(10.0, 150.0);
        EARLY_WIDTH = "SPACE", "EARLY WIDTH", field!(early_width), UNIT;
        STEREO = "SPACE", "STEREO INPUT", field!(stereo), UNIT;
        INPUT_DIFFUSION = "SPACE", "INPUT DIFFUSION", field!(input_diffusion), UNIT;
        HALL_FRONT = "SPACE", "FRONT", field!(hall_front), UNIT;
        ATTACK_SHAPE = "SPACE", "ATTACK SHAPE", field!(attack_shape), BIPOLAR;
        DISPERSE = "SPACE", "DISPERSE", field!(disperse), UNIT;
        DECAY = "SPACE", "DECAY", field!(decay_s), log(Reverb::MIN_DECAY_S, 60.0) => log(0.1, 60.0);
        SIZE = "SPACE", "SIZE", field!(size), log(0.1, Reverb::MAX_SIZE);
        DIFFUSION = "SPACE", "DIFFUSION", field!(diffusion), linear(0.0, 0.7);
        WIDTH = "SPACE", "WIDTH", field!(width), linear(0.0, 4.0);
        FORWARD = "SPACE", "FORWARD", field!(forward), UNIT;
        REVERSE = "SPACE", "REVERSE", field!(reverse), UNIT;
        DRY = "SPACE", "DRY", field!(dry), UNIT;
        FADE_AFTER = "SPACE", "TAIL HOLD", field!(fade_after_s), skewed(0.0, Reverb::MAX_FADE_AFTER_S, 2.0);
        FADE_RATE = "SPACE", "TAIL FALL", field!(fade_db_per_s), log(Reverb::MIN_FADE_DB_PER_S, Reverb::MAX_FADE_DB_PER_S);
        DEPTH = "MODULATION", "DEPTH", field!(mod_depth), skewed(0.0, Reverb::MAX_MOD_DEPTH_MS, 4.0);
        PHASE = "MODULATION", "PHASE", field!(start_phase), UNIT;
        RATE = "MODULATION", "RATE", field!(mod_rate_hz), RATE_HZ;
        DUCK_GATE = "DUCK", "DUCK / GATE", field!(ducking), BIPOLAR;
        DUCK_THRESHOLD = "DUCK", "THRESHOLD", field!(threshold_db), linear(-60.0, 0.0);
        DUCK_ATTACK = "DUCK", "ATTACK", field!(duck_attack_ms), log(1.0, 500.0);
        DUCK_RELEASE = "DUCK", "RELEASE", field!(duck_release_ms), log(10.0, 2000.0);
        RANDOM = "MODULATION", "RANDOM", field!(mod_random), UNIT;
        DAMPING = "FILTER", "DAMPING", field!(damping_hz), log(Reverb::MIN_DAMPING_HZ, 20_000.0);
        LOW_DECAY = "FILTER", "LOW DECAY", field!(low_decay), log(0.25, Reverb::MAX_LOW_DECAY);
        LOW_CROSSOVER = "FILTER", "LOW CROSSOVER", field!(low_crossover_hz), log(100.0, 2000.0);
        HIGH_DECAY = "FILTER", "HIGH DECAY", field!(high_decay), log(0.25, 4.0);
        HIGH_CROSSOVER = "FILTER", "HIGH CROSSOVER", field!(high_crossover_hz), log(1000.0, 16_000.0);
        SHIFT = "SHIMMER", "SHIFT", field!(shift), linear(-24.0, 24.0);
        SHIMMER = "SHIMMER", "SHIMMER", field!(shimmer), linear(0.0, 0.95);
        TAP1_TIME = "TAPS", "TAP 1 TIME", field!(tap1_ms), skewed(0.0, Reverb::MAX_TAP_MS, 3.0);
        TAP1_LEVEL = "TAPS", "TAP 1 LEVEL", field!(tap1_level), UNIT;
        TAP1_SIDE = "TAPS", "TAP 1 SIDE", field!(tap1_side), BIPOLAR;
        TAP2_TIME = "TAPS", "TAP 2 TIME", field!(tap2_ms), skewed(0.0, Reverb::MAX_TAP_MS, 3.0);
        TAP2_LEVEL = "TAPS", "TAP 2 LEVEL", field!(tap2_level), UNIT;
        TAP2_SIDE = "TAPS", "TAP 2 SIDE", field!(tap2_side), BIPOLAR;
        TAP3_TIME = "TAPS", "TAP 3 TIME", field!(tap3_ms), skewed(0.0, Reverb::MAX_TAP_MS, 3.0);
        TAP3_LEVEL = "TAPS", "TAP 3 LEVEL", field!(tap3_level), UNIT;
        TAP3_SIDE = "TAPS", "TAP 3 SIDE", field!(tap3_side), BIPOLAR;
        TAP4_TIME = "TAPS", "TAP 4 TIME", field!(tap4_ms), skewed(0.0, Reverb::MAX_TAP_MS, 3.0);
        TAP4_LEVEL = "TAPS", "TAP 4 LEVEL", field!(tap4_level), UNIT;
        TAP4_SIDE = "TAPS", "TAP 4 SIDE", field!(tap4_side), BIPOLAR;
        EARLY_SCATTER = "SPACE", "EARLY SCATTER", field!(early_scatter), UNIT;
        MIXING = "SPACE", "MIXING", field!(mixing), UNIT;
        PLATEAU = "SPACE", "PLATEAU", field!(plateau_ms), skewed(0.0, Reverb::MAX_PLATEAU_MS, 2.0);
        SPARSE = "MODULATION", "SPARSE", field!(mod_sparse), UNIT;
        TAP_SCATTER = "TAPS", "TAP SCATTER", field!(tap_scatter), UNIT;
        ENTRY_SPREAD = "SPACE", "ENTRY SPREAD", field!(entry_spread), UNIT;
        TAP5_TIME = "TAPS", "TAP 5 TIME", field!(tap5_ms), skewed(0.0, Reverb::MAX_TAP_MS, 3.0);
        TAP5_LEVEL = "TAPS", "TAP 5 LEVEL", field!(tap5_level), UNIT;
        TAP5_SIDE = "TAPS", "TAP 5 SIDE", field!(tap5_side), BIPOLAR;
        TAP1_CROSS = "TAPS", "TAP 1 CROSS", field!(tap1_cross), UNIT;
        TAP2_CROSS = "TAPS", "TAP 2 CROSS", field!(tap2_cross), UNIT;
        TAP3_CROSS = "TAPS", "TAP 3 CROSS", field!(tap3_cross), UNIT;
        TAP4_CROSS = "TAPS", "TAP 4 CROSS", field!(tap4_cross), UNIT;
        TAP5_CROSS = "TAPS", "TAP 5 CROSS", field!(tap5_cross), UNIT;
        CROSS_FEED = "SPACE", "CROSS FEED", field!(cross_feed), UNIT;
        EARLY_CROSS = "SPACE", "EARLY CROSS", field!(early_cross), UNIT;
        CROSS_TIME = "SPACE", "CROSS TIME", field!(cross_ms), skewed(0.0, Reverb::MAX_CROSS_MS, 2.0);
        PLATEAU_DRIFT = "SPACE", "PLATEAU DRIFT", field!(plateau_drift), UNIT;
        PLATEAU_COPIES = "SPACE", "PLATEAU COPIES", int!(plateau_copies), linear(Reverb::MIN_PLATEAU_COPIES as f64, Reverb::PLATEAU_COPIES as f64);
        PLATEAU_SHAPE = "SPACE", "PLATEAU SHAPE", field!(plateau_shape), BIPOLAR;
        BUILD = "SPACE", "BUILD", field!(build), UNIT;
    }
}

static FLAT: [Param<Reverb>; LISTED.len() + 2 * FilterKnobs::<Reverb>::COUNT] = flat(&[LISTED, &FILTER1.all(), &FILTER2.all()]);
pub static PARAMS: &[Param<Reverb>] = &FLAT;
