use super::*;

choice!(pub DelayMode { Clean = "Clean" / "clean", Tape = "Tape" / "tape", Bbd = "BBD" / "bbd" });

params!(Delay {
    pick mode: DelayMode = DelayMode::Tape, "mode";
    flag link = false, "link";
    switch sync_l = false, "leftSync";
    lead time_l_ms = 375.0, "leftTimeMs", Delay::MIN_TIME..=Delay::MAX_TIME;
    pick division_l: Division = Division::Quarter, "leftDivision";
    number scale_l = 1.0, "leftSyncScale", Delay::MIN_SCALE..=Delay::MAX_SCALE;
    switch sync_r = false, "rightSync";
    lead time_r_ms = 500.0, "rightTimeMs", Delay::MIN_TIME..=Delay::MAX_TIME;
    pick division_r: Division = Division::Quarter, "rightDivision";
    number scale_r = 1.0, "rightSyncScale", Delay::MIN_SCALE..=Delay::MAX_SCALE;
    flag sync_c = false, "centerSync";
    number time_c_ms = 375.0, "centerTimeMs", Delay::MIN_TIME..=Delay::MAX_TIME;
    choice division_c: Division = Division::Quarter, "centerDivision";
    number level_c = 0.0, "centerLevel", 0.0..=1.0;
    number pan_c = 0.0, "centerPan", -1.0..=1.0;
    number forward = 1.0, "forwardLevel", 0.0..=1.0;
    number reverse = 0.0, "reverseLevel", 0.0..=1.0;
    lead feedback = 0.35, "feedback", 0.0..=1.0;
    number feedback_ceiling = Delay::FEEDBACK_CEILING, "feedbackCeiling", Delay::FEEDBACK_CEILING..=Delay::MAX_FEEDBACK_CEILING;
    custom feedback_r: Option<f64> = None, "rightFeedback", "number 0 to 1 by feedback's law, the right line's own feedback; left out to follow feedback", |p, _, f| if let Some(feedback) = p.feedback_r { f.push("rightFeedback", Json::Number(feedback)) }, |json, _| json.and_then(|j| j.get("rightFeedback")).and_then(Json::as_f64).map(|feedback| feedback.clamp(0.0, 1.0));
    flag freeze = false, "freeze";
    number ping_pong = 0.0, "pingPong", 0.0..=1.0;
    custom ping_pong_r: Option<f64> = None, "rightPingPong", "number 0 to 1, the share of the right line's recycle that crosses to the left; left out to follow pingPong", |p, _, f| if let Some(share) = p.ping_pong_r { f.push("rightPingPong", Json::Number(share)) }, |json, _| json.and_then(|j| j.get("rightPingPong")).and_then(Json::as_f64).map(|share| share.clamp(0.0, 1.0));
    flag even = false, "evenPingPong";
    flag even_stereo = false, "evenStereo";
    flag even_sway = false, "evenSway";
    number lean = 0.0, "inputLean", -1.0..=1.0;
    nested filter1: Filter = Filter::LOW_CUT, "filter1";
    nested filter2: Filter = Filter::HIGH_CUT, "filter2";
    nested feedback_filter: Filter = Delay::FEEDBACK_CUT, "feedbackFilter";
    flag loop_soft = false, "loopSoft";
    number drive = 0.25, "drive", 0.0..=1.0;
    number mod_depth = 0.0, "modDepth", 0.0..=1.0;
    number mod_rate_hz = 0.5, "modRateHz", Delay::MIN_MOD_HZ..=Delay::MAX_MOD_HZ;
    number mod_flutter = 0.4, "modFlutter", 0.0..=1.0;
    number mod_spread = 0.25, "modSpread", -0.5..=0.5;
    number start_phase = 0.0, "modStartPhase", 0.0..=1.0;
    number diffusion = 0.0, "diffusion", 0.0..=1.0;
    number width = 1.1, "width", 0.0..=1.5;
    number pan = 0.0, "wetPan", -1.0..=1.0;
    number pan_l = -1.0, "leftPan", -1.0..=1.0;
    number pan_r = 1.0, "rightPan", -1.0..=1.0;
    flag pan_balance = false, "linePanBalance";
    number ducking = 0.0, "duckGate", -1.0..=1.0;
    number threshold_db = -30.0, "duckThresholdDb", -60.0..=0.0;
    number attack_ms = 5.0, "duckAttackMs", 1.0..=500.0;
    number release_ms = 200.0, "duckReleaseMs", 10.0..=2000.0;
    number follow_threshold_db = -60.0, "followThresholdDb", -90.0..=0.0;
    number follow_range_db = 30.0, "followRangeDb", 1.0..=90.0;
    number follow_attack_ms = 0.0, "followAttackMs", 0.0..=Delay::MAX_FOLLOW_ATTACK;
    number follow_release_ms = 20.0, "followReleaseMs", 1.0..=2000.0;
    number follow_feedback = 0.0, "followFeedback", -Delay::MAX_FOLLOW..=Delay::MAX_FOLLOW;
    number follow_feedback_r = 0.0, "followRightFeedback", -Delay::MAX_FOLLOW..=Delay::MAX_FOLLOW;
    number follow_cross = 0.0, "followCross", -Delay::MAX_FOLLOW..=Delay::MAX_FOLLOW;
    number follow_cross_r = 0.0, "followRightCross", -Delay::MAX_FOLLOW..=Delay::MAX_FOLLOW;
    number follow_wet = 0.0, "followWet", -Delay::MAX_FOLLOW..=Delay::MAX_FOLLOW;
    number follow_dry = 0.0, "followDry", -Delay::MAX_FOLLOW_DRY..=Delay::MAX_FOLLOW_DRY;
    number wet_trim_db = 0.0, "wetTrimDb", -24.0..=12.0;
    number dry = 0.0, "dry", 0.0..=1.0;
    number splice = 0.0, "spliceMix", 0.0..=1.0;
    number pitch = 12.0, "splicePitchSemitones", -Delay::MAX_PITCH..=Delay::MAX_PITCH;
    number detune = 0.0, "spliceDetuneCents", -Delay::MAX_DETUNE..=Delay::MAX_DETUNE;
    flag splice_sync = false, "spliceSync";
    number splice_ms = 500.0, "spliceLengthMs", Delay::MIN_SPLICE..=Delay::MAX_SPLICE;
    choice splice_division: Division = Division::Quarter, "spliceDivision";
    number smoothing_ms = 20.0, "spliceSmoothingMs", Delay::MIN_SMOOTHING..=Delay::MAX_SMOOTHING;
    flag splice_reverse = false, "spliceReverse";
    flag hold = false, "spliceHold";
    number splice_offset = 0.0, "spliceRightOffset", 0.0..=0.9;
    flag sweep = false, "spliceSweep";
    flag cuts_after = false, "spliceCutsAfter";
    flag aliasing = false, "spliceAliasing";
    number wet_drive = 0.0, "wetDrive", 0.0..=1.0;
    number input_drive = 0.0, "inputDrive", 0.0..=1.0;
});

impl Delay {
    pub const FEEDBACK_CUT: Filter = Filter { hz: 5000.0, ..Filter::DEFAULT };
    pub const FILTERS: usize = 3;
    pub const FEEDBACK_CEILING: f64 = 0.985;
    pub const MAX_FEEDBACK_CEILING: f64 = 1.2;
    pub const MIN_TIME: f64 = 1.0;
    pub const MAX_TIME: f64 = 4000.0;
    pub const MIN_SPLICE: f64 = 2.0;
    pub const MAX_SPLICE: f64 = 2000.0;
    pub const MIN_SMOOTHING: f64 = 1.0;
    pub const MAX_SMOOTHING: f64 = 2000.0;
    pub const MAX_PITCH: f64 = 36.0;
    pub const MAX_DETUNE: f64 = 4800.0;
    pub const MIN_SCALE: f64 = 0.25;
    pub const MAX_SCALE: f64 = 4.0;
    pub const MIN_MOD_HZ: f64 = 0.05;
    pub const MAX_MOD_HZ: f64 = 10.0;
    pub const MAX_FOLLOW: f64 = 4.0;
    pub const MAX_FOLLOW_DRY: f64 = 2.0;
    pub const MAX_FOLLOW_ATTACK: f64 = 100.0;

    pub fn filters(&self) -> [Filter; Delay::FILTERS] {
        [self.filter1, self.filter2, self.feedback_filter]
    }

    pub fn following(&self) -> bool {
        [self.follow_feedback, self.follow_feedback_r, self.follow_cross, self.follow_cross_r, self.follow_wet, self.follow_dry].iter().any(|amount| *amount != 0.0)
    }
}

impl Filtered for Delay {
    fn sites(&self) -> &'static [&'static [usize]] {
        &[&[0, 1], &[2]]
    }

    fn filter(&self, unit: usize) -> &Filter {
        match unit {
            0 => &self.filter1,
            1 => &self.filter2,
            _ => &self.feedback_filter,
        }
    }

    fn filter_mut(&mut self, unit: usize) -> &mut Filter {
        match unit {
            0 => &mut self.filter1,
            1 => &mut self.filter2,
            _ => &mut self.feedback_filter,
        }
    }
}

pub const FILTER1: FilterKnobs<Delay> = filter_params!("FILTER 1", filter1);
pub const FILTER2: FilterKnobs<Delay> = filter_params!("FILTER 2", filter2);
pub const FEEDBACK_FILTER: FilterKnobs<Delay> = filter_params!("FEEDBACK FILTER", feedback_filter);

table! {
    LISTED: Delay {
        LEFT_TIME = "TIME", "LEFT TIME", field!(time_l_ms), log(Delay::MIN_TIME, Delay::MAX_TIME);
        DRY = "TIME", "DRY", field!(dry), UNIT;
        RIGHT_TIME = "TIME", "RIGHT TIME", field!(time_r_ms), log(Delay::MIN_TIME, Delay::MAX_TIME);
        LEFT_SCALE = "TIME", "LEFT SCALE", field!(scale_l), log(Delay::MIN_SCALE, Delay::MAX_SCALE);
        RIGHT_SCALE = "TIME", "RIGHT SCALE", field!(scale_r), log(Delay::MIN_SCALE, Delay::MAX_SCALE);
        CENTER_TIME = "TIME", "CENTER TIME", field!(time_c_ms), log(Delay::MIN_TIME, Delay::MAX_TIME);
        CENTER_LEVEL = "TIME", "CENTER LEVEL", field!(level_c), UNIT;
        CENTER_PAN = "OUTPUT", "CENTER PAN", field!(pan_c), BIPOLAR;
        FORWARD = "TIME", "FORWARD", field!(forward), UNIT;
        REVERSE = "TIME", "REVERSE", field!(reverse), UNIT;
        FEEDBACK = "LOOP", "FEEDBACK", field!(feedback), UNIT;
        RIGHT_FEEDBACK = "LOOP", "RIGHT FEEDBACK", field!(|p: &Delay| p.feedback_r.unwrap_or(p.feedback), |p: &mut Delay, v| p.feedback_r = Some(v)), UNIT;
        FEEDBACK_CEILING = "LOOP", "FEEDBACK CEILING", field!(feedback_ceiling), linear(Delay::FEEDBACK_CEILING, Delay::MAX_FEEDBACK_CEILING);
        PING_PONG = "LOOP", "PING-PONG", field!(ping_pong), UNIT;
        RIGHT_PING_PONG = "LOOP", "RIGHT PING-PONG", field!(|p: &Delay| p.ping_pong_r.unwrap_or(p.ping_pong), |p: &mut Delay, v| p.ping_pong_r = Some(v)), UNIT;
        LEAN = "LOOP", "LEAN", field!(lean), BIPOLAR;
        DRIVE = "TONE", "DRIVE", field!(drive), UNIT;
        MOD_DEPTH = "TONE", "MOD DEPTH", field!(mod_depth), UNIT;
        MOD_RATE = "TONE", "MOD RATE", field!(mod_rate_hz), log(Delay::MIN_MOD_HZ, Delay::MAX_MOD_HZ);
        MOD_FLUTTER = "TONE", "FLUTTER", field!(mod_flutter), UNIT;
        MOD_SPREAD = "TONE", "MOD SPREAD", field!(mod_spread), linear(-0.5, 0.5);
        PHASE = "TONE", "PHASE", field!(start_phase), UNIT;
        DIFFUSE = "TONE", "DIFFUSE", field!(diffusion), UNIT;
        WIDTH = "OUTPUT", "WIDTH", field!(width), linear(0.0, 1.5);
        WET_TRIM = "OUTPUT", "WET TRIM", field!(wet_trim_db), linear(-24.0, 12.0);
        WET_DRIVE = "TONE", "WET DRIVE", field!(wet_drive), UNIT;
        INPUT_DRIVE = "TONE", "INPUT DRIVE", field!(input_drive), UNIT;
        WET_PAN = "OUTPUT", "WET PAN", field!(pan), BIPOLAR;
        LEFT_PAN = "OUTPUT", "LEFT PAN", field!(pan_l), BIPOLAR;
        RIGHT_PAN = "OUTPUT", "RIGHT PAN", field!(pan_r), BIPOLAR;
        DUCK_GATE = "DUCK", "DUCK / GATE", field!(ducking), BIPOLAR;
        THRESHOLD = "DUCK", "THRESHOLD", field!(threshold_db), linear(-60.0, 0.0);
        ATTACK = "DUCK", "ATTACK", field!(attack_ms), log(1.0, 500.0);
        RELEASE = "DUCK", "RELEASE", field!(release_ms), log(10.0, 2000.0);
        FOLLOW_THRESHOLD = "FOLLOW", "THRESHOLD", field!(follow_threshold_db), linear(-90.0, 0.0);
        FOLLOW_RANGE = "FOLLOW", "RANGE", field!(follow_range_db), linear(1.0, 90.0);
        FOLLOW_ATTACK = "FOLLOW", "ATTACK", field!(follow_attack_ms), linear(0.0, Delay::MAX_FOLLOW_ATTACK);
        FOLLOW_RELEASE = "FOLLOW", "RELEASE", field!(follow_release_ms), log(1.0, 2000.0);
        FOLLOW_FEEDBACK = "FOLLOW", "FEEDBACK", field!(follow_feedback), linear(-Delay::MAX_FOLLOW, Delay::MAX_FOLLOW) => BIPOLAR;
        FOLLOW_RIGHT_FEEDBACK = "FOLLOW", "RIGHT FEEDBACK", field!(follow_feedback_r), linear(-Delay::MAX_FOLLOW, Delay::MAX_FOLLOW) => BIPOLAR;
        FOLLOW_CROSS = "FOLLOW", "CROSS", field!(follow_cross), linear(-Delay::MAX_FOLLOW, Delay::MAX_FOLLOW) => BIPOLAR;
        FOLLOW_RIGHT_CROSS = "FOLLOW", "RIGHT CROSS", field!(follow_cross_r), linear(-Delay::MAX_FOLLOW, Delay::MAX_FOLLOW) => BIPOLAR;
        FOLLOW_WET = "FOLLOW", "WET", field!(follow_wet), linear(-Delay::MAX_FOLLOW, Delay::MAX_FOLLOW) => BIPOLAR;
        FOLLOW_DRY = "FOLLOW", "DRY", field!(follow_dry), linear(-Delay::MAX_FOLLOW_DRY, Delay::MAX_FOLLOW_DRY) => BIPOLAR;
        MIX = "SPLICE", "MIX", field!(splice), UNIT;
        PITCH = "SPLICE", "PITCH", field!(|p| p.pitch, |p, v| p.pitch = v.round()), linear(-Delay::MAX_PITCH, Delay::MAX_PITCH);
        DETUNE = "SPLICE", "DETUNE", field!(detune), linear(-Delay::MAX_DETUNE, Delay::MAX_DETUNE);
        LENGTH = "SPLICE", "LENGTH", field!(splice_ms), log(Delay::MIN_SPLICE, Delay::MAX_SPLICE);
        SMOOTHING = "SPLICE", "SMOOTHING", field!(smoothing_ms), log(Delay::MIN_SMOOTHING, Delay::MAX_SMOOTHING);
        RIGHT_OFFSET = "SPLICE", "RIGHT OFFSET", field!(splice_offset), linear(0.0, 0.9);
    }
}

static FLAT: [Param<Delay>; LISTED.len() + 3 * FilterKnobs::<Delay>::COUNT] = flat(&[LISTED, &FILTER1.all(), &FILTER2.all(), &FEEDBACK_FILTER.all()]);
pub static PARAMS: &[Param<Delay>] = &FLAT;
