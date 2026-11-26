use super::*;

choice!(pub Routing { Single = "Single" / "single", Series = "Series" / "series", Parallel = "Parallel" / "parallel" });

choice!(pub PhaserShape { Sine = "Sine" / "sine", Triangle = "Tri" / "triangle", Arc = "Arc" / "arc", Round = "Round" / "round", Square = "Square" / "square", Step4 = "Step 4" / "step-4", Step8 = "Step 8" / "step-8", SampleHold = "S&H" / "sample-hold" });

pub const STAGES: [usize; 6] = [2, 4, 6, 8, 12, 16];

pub const STAGE_LABELS: [&str; 6] = ["2", "4", "6", "8", "12", "16"];

fn stages(value: f64) -> usize {
    STAGES.iter().copied().min_by_key(|stage| (*stage as f64 - value).abs() as i64).unwrap_or(4)
}

params!(Phaser {
    choice routing: Routing = Routing::Single, "routing";
    custom stages: usize = 4, "stages", "one of 2, 4, 6, 8, 12, 16; default 4", |p, d, f| f.lead("stages", Json::Number(p.stages as f64), p.stages == d.stages), |json, d| stages(number(json, "stages", d.stages as f64));
    number rate_ratio = 1.618_033_988_7, "rateRatio", 0.25..=4.0;
    custom stage_offset: i32 = 0, "stageOffset", "whole number -4 to 4, default 0", |p, d, f| f.number("stageOffset", f64::from(p.stage_offset), f64::from(d.stage_offset)), |json, d| number(json, "stageOffset", f64::from(d.stage_offset)).clamp(-4.0, 4.0).round() as i32;
    choice shape: PhaserShape = PhaserShape::Sine, "shape";
    switch sync = false, "sync";
    pick division: Division = Division::Quarter, "division";
    lead rate_hz = 0.5, "rateHz", Phaser::MIN_RATE_HZ..=Phaser::MAX_RATE_HZ;
    lead depth_octaves = 2.0, "depthOctaves", 0.0..=Phaser::MAX_DEPTH_OCTAVES;
    lead center_hz = 800.0, "centerHz", 30.0..=8000.0;
    flag stage_clamp = false, "stageClamp";
    number sweep_floor_hz = Phaser::MAX_FLOOR_HZ, "sweepFloorHz", Phaser::MIN_FLOOR_HZ..=Phaser::MAX_FLOOR_HZ;
    number spread = 0.5, "spread", 0.0..=1.0;
    number stage_spread = 0.0, "stageSpreadOctaves", 0.0..=Phaser::MAX_STAGE_SPREAD;
    number right_offset = 0.0, "rightOffsetOctaves", -Phaser::MAX_RIGHT_OFFSET..=Phaser::MAX_RIGHT_OFFSET;
    number start_phase = 0.0, "startPhase", 0.0..=1.0;
    flag vintage = false, "vintage";
    lead feedback = 0.3, "feedback", -1.0..=1.0;
    number feedback_low_cut_hz = 120.0, "feedbackLowCutHz", Phaser::MIN_LOW_CUT_HZ..=Phaser::MAX_LOW_CUT_HZ;
    number feedback_high_cut_hz = 8000.0, "feedbackHighCutHz", Phaser::MIN_HIGH_CUT_HZ..=Phaser::HIGH_CUT_OFF_HZ;
    number drive = 1.5, "driveTimes", 1.0..=4.0;
    number width = 1.0, "width", 0.0..=1.5;
    number dry = 0.0, "dry", 0.0..=1.0;
    number blend = 0.5, "blend", 0.0..=1.0;
});

impl Phaser {
    pub const MIN_RATE_HZ: f64 = 0.01;
    pub const MAX_RATE_HZ: f64 = 400.0;
    pub const MAX_DEPTH_OCTAVES: f64 = 24.0;
    pub const MIN_FLOOR_HZ: f64 = 1.0;
    pub const MAX_FLOOR_HZ: f64 = 20.0;
    pub const MAX_STAGE_SPREAD: f64 = 4.0;
    pub const MAX_RIGHT_OFFSET: f64 = 3.0;
    pub const MIN_LOW_CUT_HZ: f64 = 20.0;
    pub const MAX_LOW_CUT_HZ: f64 = 1000.0;
    pub const MIN_HIGH_CUT_HZ: f64 = 1000.0;
    pub const HIGH_CUT_OFF_HZ: f64 = 20_000.0;
}
