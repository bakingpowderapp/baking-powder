use super::*;

choice!(pub Curve { Soft = "Soft" / "soft", Tube = "Tube" / "tube", Tape = "Tape" / "tape", Diode = "Diode" / "diode", OpAmp = "Op-Amp" / "op-amp", Fold = "Fold" / "fold", Crush = "Crush" / "crush", HardClip = "Hard Clip" / "hard-clip", HalfRect = "Half Rect" / "half-rect", FullRect = "Full Rect" / "full-rect", Decimate = "Decimate" / "decimate", Limit = "Limit" / "limit", Phase = "Phase" / "phase" });

choice!(pub BandStereo { Stereo = "Stereo" / "stereo", MidSide = "M-S" / "mid-side", Mid = "Mid" / "mid", Side = "Side" / "side" });

choice!(pub RippleShape { Sine = "SINE" / "sine", Triangle = "TRI" / "triangle" });

choice!(pub DetectMode { Rms = "RMS" / "rms", Peak = "PEAK" / "peak" });

choice!(pub DetectStereo { Max = "MAX" / "max", Mean = "MEAN" / "mean" });

choice!(pub DetectSlope { Split = "24" / "24", Gentle = "12" / "12" });

pub const FLIP_LABELS: [&str; 4] = ["None", "Whole", "Copy", "Curve"];

choice!(pub FreakSlope { TwoPole = "12" / "12", Linkwitz = "24" / "24", Butterworth = "30" / "30", Linear = "LIN" / "linear" });

choice!(pub SubSlope { Six = "6" / "6", Twelve = "12" / "12", TwentyFour = "24" / "24", Thirty = "30" / "30", FortyEight = "48" / "48" });

choice!(pub Order { Off = "OFF" / "off", Post = "POST" / "post", Pre = "PRE" / "pre" });

pub const CURVE_OR_NONE: [&str; Curve::LABELS.len() + 1] = or_none(Curve::LABELS);

const fn or_none<const N: usize>(labels: &[&'static str]) -> [&'static str; N] {
    let mut out = ["None"; N];
    let mut i = 0;
    while i < labels.len() {
        out[i + 1] = labels[i];
        i += 1;
    }
    out
}

params!(Squeeze {
    number threshold_db = -24.0, "thresholdDb", -60.0..=0.0;
    number ratio = 4.0, "ratio", 1.0..=20.0;
    number gain_db = 0.0, "gainDb", -24.0..=24.0;
    number attack_ms = 5.0, "attackMs", Squeeze::MIN_ATTACK_MS..=Squeeze::MAX_ATTACK_MS;
    number release_ms = 80.0, "releaseMs", Squeeze::MIN_RELEASE_MS..=Squeeze::MAX_RELEASE_MS;
    number up = 0.5, "upDepth", 0.0..=1.0;
    number down = 0.5, "downDepth", 0.0..=1.0;
    number soft_clip = 0.0, "softClip", 0.0..=Squeeze::MAX_CLIP;
    number width = 1.0, "width", 0.0..=Squeeze::MAX_WIDTH;
    number expand_threshold_db = Squeeze::EXPAND_OFF_DB, "expandThresholdDb", Squeeze::EXPAND_OFF_DB..=0.0;
    number expand_ratio = 2.0, "expandRatio", 1.0..=20.0;
    number expand_knee_db = 0.0, "expandKneeDb", 0.0..=Squeeze::MAX_EXPAND_KNEE_DB;
    number expand_attack_ms = 1.0, "expandAttackMs", Squeeze::MIN_ATTACK_MS..=Squeeze::MAX_ATTACK_MS;
    number expand_release_ms = 50.0, "expandReleaseMs", Squeeze::MIN_RELEASE_MS..=Squeeze::MAX_RELEASE_MS;
    choice detect_slope: DetectSlope = DetectSlope::Split, "detectSlope";
    number detect_ms = 10.0, "detectMs", Squeeze::MIN_DETECT_MS..=Squeeze::MAX_DETECT_MS;
    choice detect_mode: DetectMode = DetectMode::Rms, "detectMode";
    choice detect_stereo: DetectStereo = DetectStereo::Max, "detectStereo";
    number detect_hold_ms = 10.0, "detectHoldMs", 0.0..=Squeeze::MAX_HOLD_MS;
    number detect_release_ms = 20.0, "detectReleaseMs", 1.0..=Squeeze::MAX_DETECT_RELEASE_MS;
    number gate_floor_db = -60.0, "gateFloorDb", Squeeze::GATE_OFF_DB..=-24.0;
    number lift_cap_db = 24.0, "liftCapDb", 0.0..=Squeeze::MAX_LIFT_DB;
    number mix = 1.0, "mix", 0.0..=1.0;
});

impl Squeeze {
    pub const MIN_ATTACK_MS: f64 = 0.1;
    pub const MAX_ATTACK_MS: f64 = 100.0;
    pub const MIN_RELEASE_MS: f64 = 10.0;
    pub const MAX_RELEASE_MS: f64 = 1000.0;
    pub const MAX_CLIP: f64 = 4.0;
    pub const MAX_WIDTH: f64 = 2.0;
    pub const EXPAND_OFF_DB: f64 = -120.0;
    pub const MAX_EXPAND_KNEE_DB: f64 = 24.0;
    pub const MIN_DETECT_MS: f64 = 0.1;
    pub const MAX_DETECT_MS: f64 = 50.0;
    pub const MAX_HOLD_MS: f64 = 100.0;
    pub const MAX_DETECT_RELEASE_MS: f64 = 500.0;
    pub const GATE_OFF_DB: f64 = -120.0;
    pub const MAX_LIFT_DB: f64 = 48.0;

    pub fn expanding(&self) -> bool {
        self.expand_threshold_db > Squeeze::EXPAND_OFF_DB
    }
}

params!(FreakBand {
    flag on = true, "on";
    choice order: Order = Order::Off, "order";
    flag solo = false, "solo";
    flag mute = false, "mute";
    number input_db = 0.0, "inputDb", -24.0..=24.0;
    custom curve: Option<Curve> = None, "curve", format!("one of {}; omit for no curve", Curve::KEYS.join(", ")), |p, _, f| if let Some(curve) = p.curve { f.push("curve", Json::String(String::from(curve.name()))) }, |json, _| name(json, "curve").and_then(Curve::from_name);
    choice stereo: BandStereo = BandStereo::Stereo, "stereo";
    number drive_db = 0.0, "driveDb", FreakBand::MIN_DRIVE_DB..=FreakBand::MAX_DRIVE_DB;
    number bias = 0.0, "bias", -Freak::MAX_BIAS..=Freak::MAX_BIAS;
    flag bias_pre = false, "biasPre";
    number dynamics = 0.0, "dynamics", -1.0..=1.0;
    number threshold_db = -30.0, "thresholdDb", -60.0..=0.0;
    number lift = 0.0, "lift", 0.0..=FreakBand::MAX_SLOPE;
    number sharpen = 0.0, "sharpen", 0.0..=FreakBand::MAX_SLOPE;
    number range_db = 24.0, "rangeDb", 0.0..=FreakBand::MAX_RANGE_DB;
    number attack_ms = 5.0, "attackMs", Freak::MIN_ATTACK_MS..=Freak::MAX_ATTACK_MS;
    number release_ms = 80.0, "releaseMs", Freak::MIN_RELEASE_MS..=Freak::MAX_RELEASE_MS;
    flag linked = true, "linked";
    number wet = 1.0, "wet", 0.0..=1.0;
    flag invert = false, "invert";
    flag invert_copy = false, "invertCopy";
    number output_db = 0.0, "outputDb", -24.0..=12.0;
    whole times: usize = 1, "times", 1.0..=Freak::MAX_TIMES as f64;
    number knee = 0.0, "knee", 0.0..=1.0;
    number bass_db = 0.0, "bassDb", -FreakBand::MAX_TONE_DB..=FreakBand::MAX_TONE_DB;
    number mid_db = 0.0, "midDb", -FreakBand::MAX_TONE_DB..=FreakBand::MAX_TONE_DB;
    number treble_db = 0.0, "trebleDb", -FreakBand::MAX_TONE_DB..=FreakBand::MAX_TONE_DB;
    number presence_db = 0.0, "presenceDb", -FreakBand::MAX_TONE_DB..=FreakBand::MAX_TONE_DB;
    number decimate_hz = 12_000.0, "decimateHz", 100.0..=48_000.0;
    number fold_ripples = 0.0, "foldRipples", 0.0..=1.0;
    whole fold_ripple_count: usize = 4, "foldRippleCount", 1.0..=Freak::MAX_RIPPLES as f64;
    choice fold_ripple_shape: RippleShape = RippleShape::Sine, "foldRippleShape";
    number phase_tone_hz = 440.0, "phaseToneHz", Freak::MIN_PHASE_TONE_HZ..=Freak::MAX_PHASE_TONE_HZ;
    number phase_normalize = 0.0, "phaseNormalize", 0.0..=1.0;
    number phase_spread = 0.0, "phaseSpread", 0.0..=Freak::MAX_PHASE_SPREAD;
    nested squeeze: Squeeze = Squeeze::DEFAULT, "squeeze";
});

impl FreakBand {
    pub const MIN_DRIVE_DB: f64 = -24.0;
    pub const MAX_DRIVE_DB: f64 = 60.0;
    pub const MAX_SLOPE: f64 = 2.0;
    pub const MAX_RANGE_DB: f64 = 48.0;
    pub const MAX_TONE_DB: f64 = 24.0;

    pub fn toned(&self) -> bool {
        [self.bass_db, self.mid_db, self.treble_db, self.presence_db].iter().any(|db| *db != 0.0)
    }

    pub fn shaping(&self) -> bool {
        self.curve.is_some()
    }

    pub fn squeezing(&self) -> bool {
        self.order != Order::Off
    }

    pub fn engaged(&self) -> bool {
        self.mute || self.solo || (self.on && (self.shaping() || self.squeezing() || self.input_db != 0.0 || self.output_db != 0.0 || self.invert || self.invert_copy))
    }
}

fn bands_manual() -> Json {
    let mut fields = Fields::new();
    FreakBand::manual(&FreakBand::DEFAULT, "", &band_description, &mut fields);
    Json::Array(vec![Json::from("4 bands, low to high; bandCount selects active bands"), fields.into_json()])
}

fn band_description(key: &str) -> String {
    String::from(match key {
        "on" => "off passes the band's range through",
        "order" => "squeeze off, after the curve or before it",
        "solo" => "hear this band alone",
        "mute" => "silence the band's range",
        "inputDb" => "band input gain",
        "stereo" => "stereo component to distort",
        "driveDb" => "curve drive",
        "bias" => "curve bias; follows the bias LFO",
        "biasPre" => "apply bias before drive, in full-scale input units",
        "dynamics" => "envelope-driven drive, up to +/-12 dB",
        "thresholdDb" => "threshold for lift and sharpen",
        "lift" => "pre-drive gain per dB below thresholdDb, capped by rangeDb",
        "sharpen" => "pre-drive expansion per dB from thresholdDb, capped by rangeDb",
        "rangeDb" => "maximum lift/sharpen gain change",
        "attackMs" => "curve envelope attack",
        "releaseMs" => "curve envelope release",
        "linked" => "link the curve envelope across channels",
        "wet" => "curve/input blend",
        "invert" => "invert band polarity",
        "invertCopy" => "invert the dry copy; with invert, only shaped output inverts",
        "outputDb" => "band output gain after both stages",
        "times" => "runs the band's chain this many times over",
        "knee" => "Diode curve softness",
        "bassDb" => "119 Hz low shelf on shaped output",
        "midDb" => "600 Hz wide bell on shaped output",
        "trebleDb" => "2.5 kHz shelf on shaped output",
        "presenceDb" => "7 kHz shelf on shaped output",
        "decimateHz" => "Decimate sample-and-hold rate",
        "foldRipples" => "Fold ripple depth",
        "foldRippleCount" => "Fold ripple frequency multiplier",
        "foldRippleShape" => "Fold ripple shape",
        "phaseToneHz" => "Phase curve modulator low-pass",
        "phaseNormalize" => "Phase curve depth made level independent",
        "phaseSpread" => "Phase offset between the sides, in turns",
        "squeeze.thresholdDb" => "compression threshold",
        "squeeze.ratio" => "compression ratio",
        "squeeze.gainDb" => "makeup gain",
        "squeeze.attackMs" => "gain fall time",
        "squeeze.releaseMs" => "gain recovery time",
        "squeeze.upDepth" => "upward compression amount",
        "squeeze.downDepth" => "downward compression amount",
        "squeeze.softClip" => "post-gain soft-clip level of full scale; 0 is off",
        "squeeze.width" => "post-gain side level; 0 is mono",
        "squeeze.expandThresholdDb" => "expander threshold after compression; -120 is off",
        "squeeze.expandRatio" => "expander ratio",
        "squeeze.expandKneeDb" => "expander knee width",
        "squeeze.expandAttackMs" => "expander opening time",
        "squeeze.expandReleaseMs" => "expander closing time",
        "squeeze.detectSlope" => "detector slope; 12 includes neighbours",
        "squeeze.detectMs" => "RMS detector window",
        "squeeze.detectMode" => "rms over detectMs, or peak held and released",
        "squeeze.detectStereo" => "louder channel, or mean channel power",
        "squeeze.detectHoldMs" => "peak hold window",
        "squeeze.detectReleaseMs" => "peak detector release",
        "squeeze.gateFloorDb" => "lift fades out over 12 dB below this; -120 is off",
        "squeeze.liftCapDb" => "maximum upward gain",
        "squeeze.mix" => "squeezed/input blend",
        _ => "",
    })
}

params!(Freak {
    custom band_count: usize = FREAK_BANDS, "bandCount", format!("whole number 1 to {FREAK_BANDS}, default {FREAK_BANDS}: bands in use, from the lowest"), |p, d, f| f.number("bandCount", p.band_count as f64, d.band_count as f64), |json, d| number(json, "bandCount", d.band_count as f64).clamp(1.0, FREAK_BANDS as f64) as usize;
    custom crossovers_hz: [f64; FREAK_BANDS - 1] = [120.0, 800.0, 5000.0], "crossoversHz", "3 numbers in Hz, 30 to 20000, each at least 1.2 times the one below; default 120, 800, 5000", |p, d, f| if p.crossovers_hz != d.crossovers_hz { f.push("crossoversHz", Json::Array(p.crossovers_hz.iter().map(|hz| Json::Number(*hz)).collect())) }, |json, d| crossovers(json, d.crossovers_hz, Freak::MIN_CROSSOVER_HZ, Freak::MAX_CROSSOVER_HZ);
    choice slope: FreakSlope = FreakSlope::Linkwitz, "crossoverSlope";
    flag oversample = true, "oversample";
    flag flat_top = false, "flatTop";
    number sub_cut_hz = 0.0, "subCutHz", 0.0..=Freak::MAX_SUB_CUT_HZ;
    choice sub_cut_slope: SubSlope = SubSlope::Thirty, "subCutSlope";
    number drive_db = 0.0, "driveDb", 0.0..=Freak::MAX_DRIVE_DB;
    number output_db = 0.0, "outputDb", -24.0..=12.0;
    number punish = 0.0, "punish", 0.0..=1.0;
    number dry = 0.0, "dry", 0.0..=1.0;
    flag crossfade = false, "crossfade";
    flag dry_plain = false, "dryPlain";
    flag ceiling = false, "ceiling";
    number ceiling_db = 0.0, "ceilingDb", -24.0..=0.0;
    number ceiling_knee_db = 0.0, "ceilingKneeDb", 0.0..=Freak::MAX_KNEE_DB;
    number ceiling_trim_db = 0.0, "ceilingTrimDb", -Freak::MAX_TRIM_DB..=Freak::MAX_TRIM_DB;
    number ceiling_release_ms = 0.0, "ceilingReleaseMs", 0.0..=Freak::MAX_CEILING_RELEASE_MS;
    number ceiling_attack_ms = 0.0, "ceilingAttackMs", 0.0..=Freak::MAX_CEILING_ATTACK_MS;
    flag ceiling_on_mix = false, "ceilingOnMix";
    nested filter1: Filter = Freak::LOW_CUT, "filter1";
    nested filter2: Filter = Freak::HIGH_CUT, "filter2";
    number bias_lfo_rate_hz = 1.0, "biasLfoRateHz", RATE_MIN_HZ..=RATE_MAX_HZ;
    number bias_lfo_depth = 0.0, "biasLfoDepth", 0.0..=Freak::MAX_BIAS;
    number bias_lfo_spread = 0.5, "biasLfoSpread", 0.0..=1.0;
    number bias_lfo_phase = 0.0, "biasLfoStartPhase", 0.0..=1.0;
    custom bands: [FreakBand; FREAK_BANDS] = [FreakBand::DEFAULT; FREAK_BANDS], "bands", bands_manual(), |p, d, f| if p.bands != d.bands { f.push("bands", Json::Array(p.bands.iter().map(|band| nested(|fields| band.fields(&FreakBand::DEFAULT, fields))).collect())) }, |json, d| {
        let mut bands = d.bands;
        for (slot, item) in bands.iter_mut().zip(items(json, "bands")) {
            *slot = FreakBand::from_json(Some(item), FreakBand::DEFAULT);
        }
        bands
    };
});

impl Freak {
    pub const MIN_CROSSOVER_HZ: f64 = 30.0;
    pub const MAX_CROSSOVER_HZ: f64 = 20_000.0;
    pub const CROSSOVER_RATIO: f64 = 1.2;
    pub const MAX_SUB_CUT_HZ: f64 = 500.0;
    pub const MIN_SUB_CUT_HZ: f64 = 5.0;
    pub const MAX_BIAS: f64 = 4.0;
    pub const MAX_RIPPLES: usize = 16;
    pub const MAX_TIMES: usize = 4;
    pub const MIN_ATTACK_MS: f64 = 0.1;
    pub const MAX_ATTACK_MS: f64 = 50.0;
    pub const MIN_RELEASE_MS: f64 = 1.0;
    pub const MAX_RELEASE_MS: f64 = 500.0;
    pub const LOW_CUT: Filter = Filter { pass: Pass::High, hz: 80.0, resonant: true, ..Filter::DEFAULT };
    pub const HIGH_CUT: Filter = Filter { hz: 12_000.0, resonant: true, ..Filter::DEFAULT };
    pub const FILTERS: usize = 2;
    pub const MASTER_VIEW_TRAYS: [&'static [usize]; FREAK_BANDS] = [&[0, 1], &[2, 3], &[4, 5], &[6, 7]];
    pub const MIN_PHASE_TONE_HZ: f64 = 20.0;
    pub const MAX_PHASE_TONE_HZ: f64 = 10_000.0;
    pub const MAX_PHASE_SPREAD: f64 = 0.25;
    pub const MAX_KNEE_DB: f64 = 12.0;
    pub const MAX_TRIM_DB: f64 = 24.0;
    pub const MAX_CEILING_RELEASE_MS: f64 = 2000.0;
    pub const MAX_CEILING_ATTACK_MS: f64 = 10.0;
    pub const MAX_DRIVE_DB: f64 = 36.0;

    pub fn count(&self) -> usize {
        self.band_count.clamp(1, FREAK_BANDS)
    }

    pub fn filters(&self) -> [Filter; Freak::FILTERS] {
        [self.filter1, self.filter2]
    }

    pub fn engaged(&self) -> bool {
        self.sub_cut_hz > 0.0 || self.output_db != 0.0 || self.ceiling || self.filter1.on || self.filter2.on || self.bands[..self.count()].iter().any(FreakBand::engaged)
    }

    pub fn latency(&self, sample_rate: f64) -> usize {
        let split = if self.slope == FreakSlope::Linear && self.count() > 1 { sdk::dsp::crossover::half(sample_rate) } else { 0 };
        split + if self.oversample { HALFBAND_LATENCY * self.most_times() } else { 0 }
    }

    pub fn most_times(&self) -> usize {
        self.bands[..self.count()].iter().map(|band| band.times.clamp(1, Freak::MAX_TIMES)).max().unwrap_or(1)
    }
}

impl Filtered for Freak {
    fn sites(&self) -> &'static [&'static [usize]] {
        &Freak::MASTER_VIEW_TRAYS
    }

    fn filter(&self, unit: usize) -> &Filter {
        if unit.is_multiple_of(Freak::FILTERS) { &self.filter1 } else { &self.filter2 }
    }

    fn filter_mut(&mut self, unit: usize) -> &mut Filter {
        if unit.is_multiple_of(Freak::FILTERS) { &mut self.filter1 } else { &mut self.filter2 }
    }
}

bank!(pub ShapeKnobs: Freak { input, drive, bias, output, dynamics, threshold, lift, sharpen, range, attack, release, wet, bass, mid, treble, presence, knee, decimate, ripples, ripple_count, phase_tone, normalize, phase_spread });
bank!(pub SqueezeKnobs: Freak { threshold, ratio, gain, attack, release, up, down, soft_clip, width, expand, expand_ratio, expand_knee, expand_attack, expand_release, detect, hold, detect_release, gate_floor, cap, mix });

macro_rules! shape_band {
    ($n:expr, $band:literal) => {
        ShapeKnobs {
            input: Param::new($band, "INPUT", field!(bands[$n].input_db), linear(-24.0, 24.0)),
            drive: Param::new($band, "DRIVE", field!(bands[$n].drive_db), linear(FreakBand::MIN_DRIVE_DB, FreakBand::MAX_DRIVE_DB)),
            bias: Param::new($band, "BIAS", field!(|p: &Freak| p.bands[$n].bias, |p: &mut Freak, v| p.bands[$n].bias = v), linear(-Freak::MAX_BIAS, Freak::MAX_BIAS)).swung(BIPOLAR),
            output: Param::new($band, "OUTPUT", field!(bands[$n].output_db), linear(-24.0, 12.0)),
            dynamics: Param::new($band, "DYNAMICS", field!(bands[$n].dynamics), BIPOLAR),
            threshold: Param::new($band, "THRESHOLD", field!(bands[$n].threshold_db), linear(-60.0, 0.0)),
            lift: Param::new($band, "LIFT", field!(bands[$n].lift), linear(0.0, FreakBand::MAX_SLOPE)),
            sharpen: Param::new($band, "SHARPEN", field!(bands[$n].sharpen), linear(0.0, FreakBand::MAX_SLOPE)),
            range: Param::new($band, "RANGE", field!(bands[$n].range_db), linear(0.0, FreakBand::MAX_RANGE_DB)),
            attack: Param::new($band, "ATTACK", field!(bands[$n].attack_ms), log(Freak::MIN_ATTACK_MS, Freak::MAX_ATTACK_MS)),
            release: Param::new($band, "RELEASE", field!(bands[$n].release_ms), log(Freak::MIN_RELEASE_MS, Freak::MAX_RELEASE_MS)),
            wet: Param::new($band, "WET", field!(bands[$n].wet), UNIT),
            bass: Param::new($band, "BASS", field!(bands[$n].bass_db), linear(-FreakBand::MAX_TONE_DB, FreakBand::MAX_TONE_DB)),
            mid: Param::new($band, "MID", field!(bands[$n].mid_db), linear(-FreakBand::MAX_TONE_DB, FreakBand::MAX_TONE_DB)),
            treble: Param::new($band, "TREBLE", field!(bands[$n].treble_db), linear(-FreakBand::MAX_TONE_DB, FreakBand::MAX_TONE_DB)),
            presence: Param::new($band, "PRESENCE", field!(bands[$n].presence_db), linear(-FreakBand::MAX_TONE_DB, FreakBand::MAX_TONE_DB)),
            knee: Param::new($band, "KNEE", field!(bands[$n].knee), UNIT),
            decimate: Param::new($band, "DECIMATE", field!(bands[$n].decimate_hz), log(100.0, 48_000.0)),
            ripples: Param::new($band, "RIPPLES", field!(bands[$n].fold_ripples), UNIT),
            ripple_count: Param::new($band, "RIPPLE COUNT", int!(bands[$n].fold_ripple_count), linear(1.0, Freak::MAX_RIPPLES as f64)),
            phase_tone: Param::new($band, "PHASE TONE", field!(bands[$n].phase_tone_hz), log(Freak::MIN_PHASE_TONE_HZ, Freak::MAX_PHASE_TONE_HZ)),
            normalize: Param::new($band, "NORMALIZE", field!(bands[$n].phase_normalize), UNIT),
            phase_spread: Param::new($band, "PHASE SPREAD", field!(bands[$n].phase_spread), linear(0.0, Freak::MAX_PHASE_SPREAD)),
        }
    };
}

macro_rules! squeeze_band {
    ($n:expr, $band:literal) => {
        SqueezeKnobs {
            threshold: Param::new($band, "THRESHOLD", field!(bands[$n].squeeze.threshold_db), linear(-60.0, 0.0)),
            ratio: Param::new($band, "RATIO", field!(bands[$n].squeeze.ratio), Scale::Ratio),
            gain: Param::new($band, "GAIN", field!(bands[$n].squeeze.gain_db), linear(-24.0, 24.0)),
            attack: Param::new($band, "ATTACK", field!(bands[$n].squeeze.attack_ms), log(Squeeze::MIN_ATTACK_MS, Squeeze::MAX_ATTACK_MS)),
            release: Param::new($band, "RELEASE", field!(bands[$n].squeeze.release_ms), log(Squeeze::MIN_RELEASE_MS, Squeeze::MAX_RELEASE_MS)),
            up: Param::new($band, "UP", field!(bands[$n].squeeze.up), UNIT),
            down: Param::new($band, "DOWN", field!(bands[$n].squeeze.down), UNIT),
            soft_clip: Param::new($band, "SOFT CLIP", field!(bands[$n].squeeze.soft_clip), linear(0.0, Squeeze::MAX_CLIP)),
            width: Param::new($band, "WIDTH", field!(bands[$n].squeeze.width), linear(0.0, Squeeze::MAX_WIDTH)),
            expand: Param::new($band, "EXPAND", field!(bands[$n].squeeze.expand_threshold_db), linear(Squeeze::EXPAND_OFF_DB, 0.0)),
            expand_ratio: Param::new($band, "EXPAND RATIO", field!(bands[$n].squeeze.expand_ratio), Scale::Ratio),
            expand_knee: Param::new($band, "EXPAND KNEE", field!(bands[$n].squeeze.expand_knee_db), linear(0.0, Squeeze::MAX_EXPAND_KNEE_DB)),
            expand_attack: Param::new($band, "EXPAND ATTACK", field!(bands[$n].squeeze.expand_attack_ms), log(Squeeze::MIN_ATTACK_MS, Squeeze::MAX_ATTACK_MS)),
            expand_release: Param::new($band, "EXPAND RELEASE", field!(bands[$n].squeeze.expand_release_ms), log(Squeeze::MIN_RELEASE_MS, Squeeze::MAX_RELEASE_MS)),
            detect: Param::new($band, "DETECT", field!(bands[$n].squeeze.detect_ms), log(Squeeze::MIN_DETECT_MS, Squeeze::MAX_DETECT_MS)),
            hold: Param::new($band, "HOLD", field!(bands[$n].squeeze.detect_hold_ms), linear(0.0, Squeeze::MAX_HOLD_MS)),
            detect_release: Param::new($band, "DETECT RELEASE", field!(bands[$n].squeeze.detect_release_ms), log(1.0, Squeeze::MAX_DETECT_RELEASE_MS)),
            gate_floor: Param::new($band, "GATE FLOOR", field!(bands[$n].squeeze.gate_floor_db), linear(Squeeze::GATE_OFF_DB, -24.0)),
            cap: Param::new($band, "CAP", field!(bands[$n].squeeze.lift_cap_db), linear(0.0, Squeeze::MAX_LIFT_DB)),
            mix: Param::new($band, "MIX", field!(bands[$n].squeeze.mix), UNIT),
        }
    };
}

pub const SHAPE1: ShapeKnobs = shape_band!(0, "BAND 1");
pub const SHAPE2: ShapeKnobs = shape_band!(1, "BAND 2");
pub const SHAPE3: ShapeKnobs = shape_band!(2, "BAND 3");
pub const SHAPE4: ShapeKnobs = shape_band!(3, "BAND 4");
pub const SQUEEZE1: SqueezeKnobs = squeeze_band!(0, "SQUEEZE 1");
pub const SQUEEZE2: SqueezeKnobs = squeeze_band!(1, "SQUEEZE 2");
pub const SQUEEZE3: SqueezeKnobs = squeeze_band!(2, "SQUEEZE 3");
pub const SQUEEZE4: SqueezeKnobs = squeeze_band!(3, "SQUEEZE 4");
pub const FILTER1: FilterKnobs<Freak> = filter_params!("FILTER 1", filter1);
pub const FILTER2: FilterKnobs<Freak> = filter_params!("FILTER 2", filter2);

table! {
    LISTED: Freak {
        BAND_2_CROSSOVER = "BAND 2", "CROSSOVER", field!(crossovers_hz[0]), log(Freak::MIN_CROSSOVER_HZ, Freak::MAX_CROSSOVER_HZ);
        BAND_3_CROSSOVER = "BAND 3", "CROSSOVER", field!(crossovers_hz[1]), log(Freak::MIN_CROSSOVER_HZ, Freak::MAX_CROSSOVER_HZ);
        BAND_4_CROSSOVER = "BAND 4", "CROSSOVER", field!(crossovers_hz[2]), log(Freak::MIN_CROSSOVER_HZ, Freak::MAX_CROSSOVER_HZ);
        SUB_CUT = "MASTER", "SUB CUT", field!(sub_cut_hz), floored(Freak::MIN_SUB_CUT_HZ, Freak::MAX_SUB_CUT_HZ);
        DRIVE = "MASTER", "DRIVE", field!(drive_db), linear(0.0, Freak::MAX_DRIVE_DB);
        OUTPUT = "MASTER", "OUTPUT", field!(output_db), linear(-24.0, 12.0);
        PUNISH = "MASTER", "PUNISH", field!(punish), UNIT;
        DRY = "MASTER", "DRY", field!(dry), UNIT;
        CEILING = "OUTPUT", "CEILING", field!(ceiling_db), linear(-24.0, 0.0);
        KNEE = "OUTPUT", "KNEE", field!(ceiling_knee_db), linear(0.0, Freak::MAX_KNEE_DB);
        CEILING_TRIM = "OUTPUT", "TRIM", field!(ceiling_trim_db), linear(-Freak::MAX_TRIM_DB, Freak::MAX_TRIM_DB);
        CEILING_RELEASE = "OUTPUT", "RELEASE", field!(ceiling_release_ms), linear(0.0, Freak::MAX_CEILING_RELEASE_MS);
        CEILING_ATTACK = "OUTPUT", "ATTACK", field!(ceiling_attack_ms), linear(0.0, Freak::MAX_CEILING_ATTACK_MS);
        BIAS_LFO_RATE = "BIAS LFO", "RATE", field!(bias_lfo_rate_hz), RATE_HZ;
        BIAS_LFO_DEPTH = "BIAS LFO", "DEPTH", field!(bias_lfo_depth), linear(0.0, Freak::MAX_BIAS) => UNIT;
        BIAS_LFO_SPREAD = "BIAS LFO", "SPREAD", field!(bias_lfo_spread), UNIT;
        BIAS_LFO_PHASE = "BIAS LFO", "PHASE", field!(bias_lfo_phase), UNIT;
    }
}

static FLAT: [Param<Freak>; 193] = flat(&[&SHAPE1.all(), &SHAPE2.all(), &SHAPE3.all(), &SHAPE4.all(), &SQUEEZE1.all(), &SQUEEZE2.all(), &SQUEEZE3.all(), &SQUEEZE4.all(), LISTED, &FILTER1.all(), &FILTER2.all()]);
pub static PARAMS: &[Param<Freak>] = &FLAT;
