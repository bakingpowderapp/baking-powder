use super::*;

choice!(pub MotionMode { None = "None" / "none", Chorus = "Chorus" / "chorus", Flanger = "Flanger" / "flanger", Phaser = "Phaser" / "phaser" });

params!(MotionBand {
    flag on = true, "on";
    choice mode: MotionMode = MotionMode::None, "mode";
    flag solo = false, "solo";
    flag mute = false, "mute";
    number input_db = 0.0, "inputDb", -24.0..=24.0;
    number output_db = 0.0, "outputDb", -24.0..=12.0;
    number wet = 1.0, "wet", 0.0..=1.0;
    nested chorus: Chorus = Chorus::DEFAULT, "chorus";
    nested flanger: Flanger = Flanger::DEFAULT, "flanger";
    nested phaser: Phaser = Phaser::DEFAULT, "phaser";
});

impl MotionBand {
    pub fn modulating(&self) -> bool {
        self.mode != MotionMode::None
    }

    pub fn engaged(&self) -> bool {
        self.mute || self.solo || (self.on && (self.modulating() || self.input_db != 0.0 || self.output_db != 0.0))
    }
}

fn bands_manual() -> Json {
    let mut fields = Fields::new();
    MotionBand::manual(&MotionBand::DEFAULT, "", &band_description, &mut fields);
    Json::Array(vec![Json::from("4 bands, low to high; bandCount selects active bands"), fields.into_json()])
}

fn lfo_description(key: &str) -> Option<&'static str> {
    Some(match key {
        "sync" => "use the tempo division instead of rateHz",
        "division" => "synced LFO division",
        "rateHz" => "LFO rate",
        "spread" => "left/right LFO phase offset",
        "startPhase" => "LFO phase at beat 0",
        "width" => "wet width",
        "dry" => "dry kept as wet rises; 0 crossfades, 1 keeps full dry",
        _ => return None,
    })
}

fn filter_description(key: &str) -> Option<&'static str> {
    Some(match key.split_once('.')? {
        (_, "on") => "filter on",
        (_, "type") => "low-pass, high-pass or band-pass",
        (_, "slopeDb") => "slope in dB per octave",
        ("filter1", "cutoffHz") => "cutoff; filters the wet",
        (_, "cutoffHz") => "cutoff; filters the wet and feedback",
        (_, "resonant") => "resonant cut shaped by Q; needs 12 or 24",
        (_, "q") => "resonance, or band width; 0.71 is flat",
        _ => return None,
    })
}

fn band_description(key: &str) -> String {
    let (stage, inner) = key.split_once('.').unwrap_or(("", key));
    let shared = if stage.is_empty() { None } else { lfo_description(inner).or_else(|| filter_description(inner)) };
    String::from(shared.unwrap_or(match (stage, inner) {
        ("", "on") => "off passes the band's range through",
        ("", "mode") => "modulation in this band: none, chorus, flanger or phaser",
        ("", "solo") => "hear this band alone",
        ("", "mute") => "silence the band's range",
        ("", "inputDb") => "band input gain",
        ("", "outputDb") => "band output gain after the stage",
        ("", "wet") => "the band's share of the row's wet",
        ("chorus", "mode") => "chorus mode",
        ("chorus", "depth") => "delay modulation depth",
        ("chorus", "centerMs") => "modulation centre delay",
        ("chorus", "farCenterMs") => "second tap delay on the same LFO; 0 is off",
        ("chorus", "farPhase") => "far tap phase relative to the first",
        ("chorus", "feedback") => "wet feedback",
        ("chorus", "trimDb") => "gain after the chorus mix",
        ("chorus", "crossfade") => "equal-power dry/wet crossfade",
        ("chorus", "dual") => "second tap at 1.5 times centerMs",
        ("flanger", "mode") => "flanger mode",
        ("flanger", "peakPolarity") => "add the wet as a peak; off subtracts it as a notch",
        ("flanger", "shape") => "LFO shape",
        ("flanger", "depth") => "delay modulation depth",
        ("flanger", "centerMs") => "modulation centre delay",
        ("flanger", "feedback") => "feedback; negative inverts polarity",
        ("phaser", "routing") => "one phaser, or two in series or parallel",
        ("phaser", "stages") => "stage count",
        ("phaser", "rateRatio") => "second phaser rate multiplier",
        ("phaser", "stageOffset") => "second phaser stage offset",
        ("phaser", "shape") => "LFO shape",
        ("phaser", "depthOctaves") => "sweep depth in octaves",
        ("phaser", "centerHz") => "sweep centre frequency",
        ("phaser", "stageClamp") => "clamp each stage between sweepFloorHz and 18 kHz",
        ("phaser", "sweepFloorHz") => "stage frequency floor under stageClamp",
        ("phaser", "stageSpreadOctaves") => "stage spacing in octaves around the centre",
        ("phaser", "rightOffsetOctaves") => "right channel frequency offset in octaves",
        ("phaser", "vintage") => "feedback coloration",
        ("phaser", "feedback") => "feedback, either polarity",
        ("phaser", "feedbackLowCutHz") => "low cut in the feedback",
        ("phaser", "feedbackHighCutHz") => "high cut in the feedback; 20000 is off",
        ("phaser", "blend") => "swept signal against the input; 0.5 is equal parts",
        ("phaser", "driveTimes") => "feedback saturation under vintage",
        _ => "",
    }))
}

params!(Motion {
    custom band_count: usize = MOTION_BANDS, "bandCount", format!("whole number 1 to {MOTION_BANDS}, default {MOTION_BANDS}: bands in use, from the lowest"), |p, d, f| f.number("bandCount", p.band_count as f64, d.band_count as f64), |json, d| number(json, "bandCount", d.band_count as f64).clamp(1.0, MOTION_BANDS as f64) as usize;
    custom crossovers_hz: [f64; MOTION_BANDS - 1] = [120.0, 800.0, 5000.0], "crossoversHz", "3 numbers in Hz, 30 to 20000, each at least 1.2 times the one below; default 120, 800, 5000", |p, d, f| if p.crossovers_hz != d.crossovers_hz { f.push("crossoversHz", Json::Array(p.crossovers_hz.iter().map(|hz| Json::Number(*hz)).collect())) }, |json, d| {
        let mut hz = d.crossovers_hz;
        for (slot, item) in hz.iter_mut().zip(items(json, "crossoversHz")) {
            *slot = item.as_f64().unwrap_or(*slot).clamp(Motion::MIN_CROSSOVER_HZ, Motion::MAX_CROSSOVER_HZ);
        }
        hz
    };
    choice slope: FreakSlope = FreakSlope::Linkwitz, "crossoverSlope";
    number output_db = 0.0, "outputDb", -24.0..=12.0;
    custom bands: [MotionBand; MOTION_BANDS] = [MotionBand::DEFAULT; MOTION_BANDS], "bands", bands_manual(), |p, d, f| if p.bands != d.bands { f.push("bands", Json::Array(p.bands.iter().map(|band| nested(|fields| band.fields(&MotionBand::DEFAULT, fields))).collect())) }, |json, d| {
        let mut bands = d.bands;
        for (slot, item) in bands.iter_mut().zip(items(json, "bands")) {
            *slot = MotionBand::from_json(Some(item), MotionBand::DEFAULT);
        }
        bands
    };
});

impl Motion {
    pub const MIN_CROSSOVER_HZ: f64 = 30.0;
    pub const MAX_CROSSOVER_HZ: f64 = 20_000.0;
    pub const CROSSOVER_RATIO: f64 = 1.2;
    pub const FILTER_TRAYS: [&'static [usize]; MOTION_BANDS] = [&[0, 1], &[2, 3], &[4, 5], &[6, 7]];

    pub fn count(&self) -> usize {
        self.band_count.clamp(1, MOTION_BANDS)
    }

    pub fn engaged(&self) -> bool {
        self.output_db != 0.0 || self.bands[..self.count()].iter().any(MotionBand::engaged)
    }

    pub fn latency(&self, sample_rate: f64) -> usize {
        if self.slope == FreakSlope::Linear && self.count() > 1 { sdk::dsp::crossover::half(sample_rate) } else { 0 }
    }
}

impl Filtered for Motion {
    fn sites(&self) -> &'static [&'static [usize]] {
        &Motion::FILTER_TRAYS
    }

    fn filter(&self, unit: usize) -> &Filter {
        let chorus = &self.bands[unit / Chorus::FILTERS % MOTION_BANDS].chorus;
        if unit.is_multiple_of(Chorus::FILTERS) { &chorus.filter1 } else { &chorus.filter2 }
    }

    fn filter_mut(&mut self, unit: usize) -> &mut Filter {
        let chorus = &mut self.bands[unit / Chorus::FILTERS % MOTION_BANDS].chorus;
        if unit.is_multiple_of(Chorus::FILTERS) { &mut chorus.filter1 } else { &mut chorus.filter2 }
    }
}

bank!(pub ChorusKnobs: Motion { rate, depth, center, far_center, far_phase, spread, phase, feedback, width, trim });
bank!(pub FlangerKnobs: Motion { rate, depth, center, spread, phase, feedback, dry, width });
bank!(pub PhaserKnobs: Motion { rate_ratio, dry, stage_offset, rate, depth, center, sweep_floor, spread, stage_spread, right_offset, phase, feedback, drive, width, blend, low_cut, high_cut });
bank!(pub BandLevels: Motion { input, output, wet });

macro_rules! chorus_band {
    ($n:expr, $band:literal) => {
        ChorusKnobs {
            rate: Param::new($band, "RATE", field!(bands[$n].chorus.rate_hz), RATE_HZ),
            depth: Param::new($band, "DEPTH", field!(|p: &Motion| p.bands[$n].chorus.depth, |p: &mut Motion, v| p.bands[$n].chorus.depth = v), linear(0.0, Chorus::MAX_DEPTH)).swung(UNIT),
            center: Param::new($band, "CENTER", field!(bands[$n].chorus.center_ms), log(1.0, 40.0)),
            far_center: Param::new($band, "FAR CENTER", field!(bands[$n].chorus.far_center_ms), skewed(0.0, Chorus::MAX_FAR_CENTER_MS, 2.0)),
            far_phase: Param::new($band, "FAR PHASE", field!(bands[$n].chorus.far_phase), UNIT),
            spread: Param::new($band, "SPREAD", field!(bands[$n].chorus.spread), UNIT),
            phase: Param::new($band, "PHASE", field!(bands[$n].chorus.start_phase), UNIT),
            feedback: Param::new($band, "FEEDBACK", field!(bands[$n].chorus.feedback), UNIT),
            width: Param::new($band, "WIDTH", field!(bands[$n].chorus.width), linear(0.0, 1.5)),
            trim: Param::new($band, "TRIM", field!(bands[$n].chorus.trim_db), linear(-24.0, 24.0)),
        }
    };
}

macro_rules! flanger_band {
    ($n:expr, $band:literal) => {
        FlangerKnobs {
            rate: Param::new($band, "RATE", field!(bands[$n].flanger.rate_hz), RATE_HZ),
            depth: Param::new($band, "DEPTH", field!(bands[$n].flanger.depth), UNIT),
            center: Param::new($band, "CENTER", field!(bands[$n].flanger.center_ms), log(0.1, 15.0)),
            spread: Param::new($band, "SPREAD", field!(bands[$n].flanger.spread), UNIT),
            phase: Param::new($band, "PHASE", field!(bands[$n].flanger.start_phase), UNIT),
            feedback: Param::new($band, "FEEDBACK", field!(bands[$n].flanger.feedback), BIPOLAR),
            dry: Param::new($band, "DRY", field!(bands[$n].flanger.dry), UNIT),
            width: Param::new($band, "WIDTH", field!(bands[$n].flanger.width), linear(0.0, 1.5)),
        }
    };
}

macro_rules! phaser_band {
    ($n:expr, $band:literal) => {
        PhaserKnobs {
            rate_ratio: Param::new($band, "RATE RATIO", field!(bands[$n].phaser.rate_ratio), log(0.25, 4.0)),
            dry: Param::new($band, "DRY", field!(bands[$n].phaser.dry), UNIT),
            stage_offset: Param::new($band, "STAGE OFFSET", int!(bands[$n].phaser.stage_offset), linear(-4.0, 4.0)),
            rate: Param::new($band, "RATE", field!(|p: &Motion| p.bands[$n].phaser.rate_hz, |p: &mut Motion, v| p.bands[$n].phaser.rate_hz = v), log(Phaser::MIN_RATE_HZ, Phaser::MAX_RATE_HZ)).swung(RATE_HZ),
            depth: Param::new($band, "DEPTH", field!(bands[$n].phaser.depth_octaves), skewed(0.0, Phaser::MAX_DEPTH_OCTAVES, 2.0)),
            center: Param::new($band, "CENTER", field!(bands[$n].phaser.center_hz), log(30.0, 8000.0)),
            sweep_floor: Param::new($band, "SWEEP FLOOR", field!(bands[$n].phaser.sweep_floor_hz), log(Phaser::MIN_FLOOR_HZ, Phaser::MAX_FLOOR_HZ)),
            spread: Param::new($band, "SPREAD", field!(bands[$n].phaser.spread), UNIT),
            stage_spread: Param::new($band, "STAGE SPREAD", field!(bands[$n].phaser.stage_spread), linear(0.0, Phaser::MAX_STAGE_SPREAD)),
            right_offset: Param::new($band, "RIGHT OFFSET", field!(bands[$n].phaser.right_offset), linear(-Phaser::MAX_RIGHT_OFFSET, Phaser::MAX_RIGHT_OFFSET)),
            phase: Param::new($band, "PHASE", field!(bands[$n].phaser.start_phase), UNIT),
            feedback: Param::new($band, "FEEDBACK", field!(bands[$n].phaser.feedback), BIPOLAR),
            drive: Param::new($band, "DRIVE", field!(bands[$n].phaser.drive), linear(1.0, 4.0)),
            width: Param::new($band, "WIDTH", field!(bands[$n].phaser.width), linear(0.0, 1.5)),
            blend: Param::new($band, "BLEND", field!(bands[$n].phaser.blend), UNIT),
            low_cut: Param::new($band, "FEEDBACK LOW CUT", field!(bands[$n].phaser.feedback_low_cut_hz), log(Phaser::MIN_LOW_CUT_HZ, Phaser::MAX_LOW_CUT_HZ)),
            high_cut: Param::new($band, "FEEDBACK HIGH CUT", field!(bands[$n].phaser.feedback_high_cut_hz), log(Phaser::MIN_HIGH_CUT_HZ, Phaser::HIGH_CUT_OFF_HZ)),
        }
    };
}

macro_rules! chorus_filters {
    ($n:expr, $first:literal, $second:literal) => {
        [filter_params!($first, bands[$n].chorus.filter1), filter_params!($second, bands[$n].chorus.filter2)]
    };
}

macro_rules! band {
    ($n:expr, $band:literal) => {
        BandLevels {
            input: Param::new($band, "INPUT", field!(bands[$n].input_db), linear(-24.0, 24.0)),
            output: Param::new($band, "OUTPUT", field!(bands[$n].output_db), linear(-24.0, 12.0)),
            wet: Param::new($band, "WET", field!(bands[$n].wet), UNIT),
        }
    };
}

pub const CHORUS1: ChorusKnobs = chorus_band!(0, "CHORUS 1");
pub const CHORUS2: ChorusKnobs = chorus_band!(1, "CHORUS 2");
pub const CHORUS3: ChorusKnobs = chorus_band!(2, "CHORUS 3");
pub const CHORUS4: ChorusKnobs = chorus_band!(3, "CHORUS 4");
pub const FLANGER1: FlangerKnobs = flanger_band!(0, "FLANGER 1");
pub const FLANGER2: FlangerKnobs = flanger_band!(1, "FLANGER 2");
pub const FLANGER3: FlangerKnobs = flanger_band!(2, "FLANGER 3");
pub const FLANGER4: FlangerKnobs = flanger_band!(3, "FLANGER 4");
pub const PHASER1: PhaserKnobs = phaser_band!(0, "PHASER 1");
pub const PHASER2: PhaserKnobs = phaser_band!(1, "PHASER 2");
pub const PHASER3: PhaserKnobs = phaser_band!(2, "PHASER 3");
pub const PHASER4: PhaserKnobs = phaser_band!(3, "PHASER 4");
pub const FILTERS1: [FilterKnobs<Motion>; Chorus::FILTERS] = chorus_filters!(0, "CHORUS 1 FILTER 1", "CHORUS 1 FILTER 2");
pub const FILTERS2: [FilterKnobs<Motion>; Chorus::FILTERS] = chorus_filters!(1, "CHORUS 2 FILTER 1", "CHORUS 2 FILTER 2");
pub const FILTERS3: [FilterKnobs<Motion>; Chorus::FILTERS] = chorus_filters!(2, "CHORUS 3 FILTER 1", "CHORUS 3 FILTER 2");
pub const FILTERS4: [FilterKnobs<Motion>; Chorus::FILTERS] = chorus_filters!(3, "CHORUS 4 FILTER 1", "CHORUS 4 FILTER 2");
pub const BAND1: BandLevels = band!(0, "BAND 1");
pub const BAND2: BandLevels = band!(1, "BAND 2");
pub const BAND3: BandLevels = band!(2, "BAND 3");
pub const BAND4: BandLevels = band!(3, "BAND 4");

table! {
    LISTED: Motion {
        BAND_2_CROSSOVER = "BAND 2", "CROSSOVER", field!(crossovers_hz[0]), log(Motion::MIN_CROSSOVER_HZ, Motion::MAX_CROSSOVER_HZ);
        BAND_3_CROSSOVER = "BAND 3", "CROSSOVER", field!(crossovers_hz[1]), log(Motion::MIN_CROSSOVER_HZ, Motion::MAX_CROSSOVER_HZ);
        BAND_4_CROSSOVER = "BAND 4", "CROSSOVER", field!(crossovers_hz[2]), log(Motion::MIN_CROSSOVER_HZ, Motion::MAX_CROSSOVER_HZ);
        OUTPUT = "MASTER", "OUTPUT", field!(output_db), linear(-24.0, 12.0);
    }
}

static FLAT: [Param<Motion>; 172] = flat(&[&CHORUS1.all(), &CHORUS2.all(), &CHORUS3.all(), &CHORUS4.all(), &FLANGER1.all(), &FLANGER2.all(), &FLANGER3.all(), &FLANGER4.all(), &PHASER1.all(), &PHASER2.all(), &PHASER3.all(), &PHASER4.all(), &BAND1.all(), &BAND2.all(), &BAND3.all(), &BAND4.all(), LISTED, &FILTERS1[0].all(), &FILTERS1[1].all(), &FILTERS2[0].all(), &FILTERS2[1].all(), &FILTERS3[0].all(), &FILTERS3[1].all(), &FILTERS4[0].all(), &FILTERS4[1].all()]);
pub static PARAMS: &[Param<Motion>] = &FLAT;
