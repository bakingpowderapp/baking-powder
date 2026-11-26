use super::*;

choice!(pub Shape { HighPass = "HP" / "high-pass", HighPassPlus = "HP+" / "high-pass-steep", LowShelf = "LS" / "low-shelf", LowShelfQ = "LSQ" / "low-shelf-q", Bell = "Bel" / "bell", Notch = "Not" / "notch", HighShelf = "HS" / "high-shelf", HighShelfQ = "HSQ" / "high-shelf-q", LowPass = "LP" / "low-pass", LowPassPlus = "LP+" / "low-pass-steep" });

choice!(pub Channel { Stereo = "Stereo" / "stereo", Mid = "Mid" / "mid", Side = "Side" / "side" });

impl Shape {
    pub fn has_gain(self) -> bool {
        matches!(self, Shape::Bell | Shape::LowShelf | Shape::LowShelfQ | Shape::HighShelf | Shape::HighShelfQ)
    }

    pub fn has_q(self) -> bool {
        !matches!(self, Shape::LowShelf | Shape::HighShelf)
    }

    pub fn max_q(self) -> f64 {
        match self {
            Shape::Notch => 100.0,
            Shape::LowShelfQ | Shape::HighShelfQ => Shape::MAX_SHELF_Q,
            _ => 18.0,
        }
    }

    pub const MAX_SHELF_Q: f64 = 4.0;

    pub fn is_plus(self) -> bool {
        matches!(self, Shape::HighPassPlus | Shape::LowPassPlus)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct EqBand {
    pub on: bool,
    pub shape: Shape,
    pub channel: Channel,
    pub hz: f64,
    pub gain_db: f64,
    pub q: f64,
    pub dynamic: bool,
    pub threshold_db: f64,
    pub ratio: f64,
    pub knee_db: f64,
    pub attack_ms: f64,
    pub release_ms: f64,
    pub link: f64,
    pub balance: f64,
}

impl EqBand {
    pub const MAX_THRESHOLD_DB: f64 = 24.0;
    pub const MAX_KNEE_DB: f64 = 24.0;
    pub const MIN_ATTACK_MS: f64 = 0.1;
    pub const MAX_ATTACK_MS: f64 = 100.0;
    pub const MIN_RELEASE_MS: f64 = 1.0;
    pub const MAX_RELEASE_MS: f64 = 1000.0;

    const fn new(shape: Shape, hz: f64, gain_db: f64, q: f64) -> EqBand {
        EqBand { on: false, shape, channel: Channel::Stereo, hz, gain_db, q, dynamic: false, threshold_db: 6.0, ratio: 2.0, knee_db: 12.0, attack_ms: 1.0, release_ms: 10.0, link: 1.0, balance: 0.0 }
    }

    pub fn moving(&self) -> bool {
        self.dynamic && self.shape.has_gain()
    }

    fn to_json(self, default: &EqBand) -> Json {
        nested(|fields| {
            fields.lead("on", Json::Bool(self.on), self.on == default.on);
            if self.on {
                fields.push("shape", Json::String(String::from(self.shape.name())));
                fields.push("channel", Json::String(String::from(self.channel.name())));
                fields.push("frequencyHz", Json::Number(self.hz));
                fields.push("gainDb", Json::Number(self.gain_db));
                fields.push("q", Json::Number(self.q));
            } else {
                fields.name("shape", self.shape.name(), default.shape.name());
                fields.name("channel", self.channel.name(), default.channel.name());
                fields.number("frequencyHz", self.hz, default.hz);
                fields.number("gainDb", self.gain_db, default.gain_db);
                fields.number("q", self.q, default.q);
            }
            fields.flag("dynamic", self.dynamic, default.dynamic);
            if self.dynamic {
                fields.push("thresholdDb", Json::Number(self.threshold_db));
                fields.push("ratio", Json::Number(self.ratio));
                fields.push("kneeDb", Json::Number(self.knee_db));
                fields.push("attackMs", Json::Number(self.attack_ms));
                fields.push("releaseMs", Json::Number(self.release_ms));
                fields.push("link", Json::Number(self.link));
                fields.push("balance", Json::Number(self.balance));
            } else {
                fields.number("thresholdDb", self.threshold_db, default.threshold_db);
                fields.number("ratio", self.ratio, default.ratio);
                fields.number("kneeDb", self.knee_db, default.knee_db);
                fields.number("attackMs", self.attack_ms, default.attack_ms);
                fields.number("releaseMs", self.release_ms, default.release_ms);
                fields.number("link", self.link, default.link);
                fields.number("balance", self.balance, default.balance);
            }
        })
    }

    fn from_json(json: Option<&Json>, default: EqBand) -> EqBand {
        let shape = name(json, "shape").and_then(Shape::from_name).unwrap_or(default.shape);
        EqBand {
            on: flag(json, "on", default.on),
            shape,
            channel: name(json, "channel").and_then(Channel::from_name).unwrap_or(default.channel),
            hz: number(json, "frequencyHz", default.hz).clamp(20.0, 20_000.0),
            gain_db: number(json, "gainDb", default.gain_db).clamp(-18.0, 18.0),
            q: number(json, "q", default.q).clamp(0.1, shape.max_q()),
            dynamic: flag(json, "dynamic", default.dynamic),
            threshold_db: number(json, "thresholdDb", default.threshold_db).clamp(0.0, EqBand::MAX_THRESHOLD_DB),
            ratio: number(json, "ratio", default.ratio).clamp(1.0, 20.0),
            knee_db: number(json, "kneeDb", default.knee_db).clamp(0.0, EqBand::MAX_KNEE_DB),
            attack_ms: number(json, "attackMs", default.attack_ms).clamp(EqBand::MIN_ATTACK_MS, EqBand::MAX_ATTACK_MS),
            release_ms: number(json, "releaseMs", default.release_ms).clamp(EqBand::MIN_RELEASE_MS, EqBand::MAX_RELEASE_MS),
            link: number(json, "link", default.link).clamp(0.0, 1.0),
            balance: number(json, "balance", default.balance).clamp(-1.0, 1.0),
        }
    }
}

const DEFAULT_BANDS: [EqBand; EQ_BANDS] = [
    EqBand::new(Shape::HighPass, 30.0, 0.0, 0.71),
    EqBand::new(Shape::Bell, 100.0, 0.0, 0.71),
    EqBand::new(Shape::Bell, 250.0, 0.0, 0.71),
    EqBand::new(Shape::Bell, 3100.0, -10.0, 18.0),
    EqBand::new(Shape::Bell, 4500.0, -10.0, 18.0),
    EqBand::new(Shape::Bell, 6500.0, -10.0, 18.0),
    EqBand::new(Shape::Bell, 8500.0, -10.0, 18.0),
    EqBand::new(Shape::LowPass, 18_000.0, 0.0, 0.71),
];

fn bands_manual() -> Json {
    let mut fields = Fields::new();
    fields.manual("on", either(false), "enable band");
    fields.manual("shape", one_of(Shape::KEYS, Shape::Bell.name()), "filter shape; bands 1/8 default to high-pass/low-pass");
    fields.manual("channel", one_of(Channel::KEYS, Channel::Stereo.name()), "stereo, mid or side processing");
    fields.push("frequencyHz", Json::from("number 20 to 20000: center/corner frequency; default 30, 100, 250, 3100, 4500, 6500, 8500 and 18000 by band"));
    fields.push("gainDb", Json::from("number -18 to 18, default 0 (-10 on bands 4 to 7): bells/shelves only; maximum when dynamic"));
    fields.push("q", Json::from("number 0.1 to 18 (100 for notch, 4 for shelf-q), default 0.71 (18 on bands 4 to 7): low-shelf and high-shelf ignore it"));
    fields.manual("dynamic", either(false), "apply gain as the band exceeds its surroundings, up to gainDb; bells/shelves only");
    fields.push("thresholdDb", Json::from("number 0 to 24, default 6: threshold above surrounding energy; white noise reads 0 dB"));
    fields.push("ratio", Json::from("number 1 to 20, default 2: ratio above thresholdDb"));
    fields.push("kneeDb", Json::from("number 0 to 24, default 12: dynamic knee width"));
    fields.push("attackMs", Json::from("number 0.1 to 100, default 1: attack at 1 kHz, scaling with frequency"));
    fields.push("releaseMs", Json::from("number 1 to 1000, default 10: release at 1 kHz, scaling with frequency"));
    fields.push("link", Json::from("number 0 to 1, default 1: stereo detector link; 0 independent"));
    fields.push("balance", Json::from("number -1 to 1, default 0: dynamic gain balance; 1 leaves left unchanged, -1 right"));
    Json::Array(vec![Json::from("8 bands; {} keeps defaults"), fields.into_json()])
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PassBand {
    pub hz: f64,
    pub q: f64,
    pub level: f64,
}

impl PassBand {
    pub const DEFAULTS: [PassBand; PASS_BANDS] = [PassBand::new(330.0, 1.0), PassBand::new(660.0, 0.0), PassBand::new(1320.0, 0.0)];
    pub const MIN_Q: f64 = 0.5;
    pub const MAX_Q: f64 = 50.0;

    const fn new(hz: f64, level: f64) -> PassBand {
        PassBand { hz, q: 8.0, level }
    }

    fn to_json(self, default: &PassBand) -> Json {
        nested(|fields| {
            fields.number("frequencyHz", self.hz, default.hz);
            fields.number("q", self.q, default.q);
            fields.number("level", self.level, default.level);
        })
    }

    fn from_json(json: Option<&Json>, default: PassBand) -> PassBand {
        PassBand {
            hz: number(json, "frequencyHz", default.hz).clamp(CURVE_LO_HZ, CURVE_HI_HZ),
            q: number(json, "q", default.q).clamp(PassBand::MIN_Q, PassBand::MAX_Q),
            level: number(json, "level", default.level).clamp(0.0, 1.0),
        }
    }
}

fn pass_manual() -> Json {
    let mut fields = Fields::new();
    fields.push("frequencyHz", Json::from("number 20 to 20000: band frequency; default 330, 660 and 1320"));
    fields.push("q", Json::from("number 0.5 to 50, default 8: band Q"));
    fields.push("level", Json::from("number 0 to 1: band level; default 1, 0 and 0"));
    Json::Array(vec![Json::from("3 band-pass bands"), fields.into_json()])
}

params!(BandPass {
    switch on = false, "on";
    custom bands: [PassBand; PASS_BANDS] = PassBand::DEFAULTS, "bands", pass_manual(), |p, d, f| if p.bands != d.bands { f.push("bands", Json::Array(p.bands.iter().zip(&d.bands).map(|(band, default)| band.to_json(default)).collect())) }, |json, d| {
        let mut bands = d.bands;
        for (slot, item) in bands.iter_mut().zip(items(json, "bands")) {
            *slot = PassBand::from_json(Some(item), *slot);
        }
        bands
    };
    number trim_db = 0.0, "trimDb", -24.0..=24.0;
});

params!(Eq {
    custom bands: [EqBand; EQ_BANDS] = DEFAULT_BANDS, "bands", bands_manual(), |p, d, f| if p.bands != d.bands { f.push("bands", Json::Array(p.bands.iter().zip(&d.bands).map(|(band, default)| band.to_json(default)).collect())) }, |json, d| {
        let mut bands = d.bands;
        for (slot, item) in bands.iter_mut().zip(items(json, "bands")) {
            *slot = EqBand::from_json(Some(item), *slot);
        }
        bands
    };
    number scale = 1.0, "gainScale", 0.5..=2.0;
    number trim_db = 0.0, "trimDb", -18.0..=18.0;
    nested pass: BandPass = BandPass::DEFAULT, "bandPass";
    state solo: Option<usize> = None;
});

bank!(pub EqBandKnobs: Eq { frequency, gain, q, threshold, ratio, knee, attack, release, link, balance });
bank!(pub PassBandKnobs: Eq { frequency, q, level });

macro_rules! eq_band {
    ($n:expr, $band:literal) => {
        EqBandKnobs {
            frequency: Param::new($band, "FREQUENCY", field!(bands[$n].hz), log(CURVE_LO_HZ, CURVE_HI_HZ)),
            gain: Param::new($band, "GAIN", field!(bands[$n].gain_db), linear(-18.0, 18.0)),
            q: Param::new($band, "Q", field!(|p: &Eq| p.bands[$n].q, |p: &mut Eq, v| p.bands[$n].q = v.min(p.bands[$n].shape.max_q())), log(0.1, 18.0)),
            threshold: Param::new($band, "THRESHOLD", field!(bands[$n].threshold_db), linear(0.0, EqBand::MAX_THRESHOLD_DB)),
            ratio: Param::new($band, "RATIO", field!(bands[$n].ratio), Scale::Ratio),
            knee: Param::new($band, "KNEE", field!(bands[$n].knee_db), linear(0.0, EqBand::MAX_KNEE_DB)),
            attack: Param::new($band, "ATTACK", field!(bands[$n].attack_ms), log(EqBand::MIN_ATTACK_MS, EqBand::MAX_ATTACK_MS)),
            release: Param::new($band, "RELEASE", field!(bands[$n].release_ms), log(EqBand::MIN_RELEASE_MS, EqBand::MAX_RELEASE_MS)),
            link: Param::new($band, "LINK", field!(bands[$n].link), UNIT),
            balance: Param::new($band, "BALANCE", field!(bands[$n].balance), BIPOLAR),
        }
    };
}

pub const BAND1: EqBandKnobs = eq_band!(0, "BAND 1");
pub const BAND2: EqBandKnobs = eq_band!(1, "BAND 2");
pub const BAND3: EqBandKnobs = eq_band!(2, "BAND 3");
pub const BAND4: EqBandKnobs = eq_band!(3, "BAND 4");
pub const BAND5: EqBandKnobs = eq_band!(4, "BAND 5");
pub const BAND6: EqBandKnobs = eq_band!(5, "BAND 6");
pub const BAND7: EqBandKnobs = eq_band!(6, "BAND 7");
pub const BAND8: EqBandKnobs = eq_band!(7, "BAND 8");

macro_rules! pass_band {
    ($n:expr, $band:literal) => {
        PassBandKnobs { frequency: Param::new($band, "FREQUENCY", field!(pass.bands[$n].hz), log(CURVE_LO_HZ, CURVE_HI_HZ)), q: Param::new($band, "Q", field!(pass.bands[$n].q), log(PassBand::MIN_Q, PassBand::MAX_Q)), level: Param::new($band, "LEVEL", field!(pass.bands[$n].level), UNIT) }
    };
}

pub const PASS1: PassBandKnobs = pass_band!(0, "PASS 1");
pub const PASS2: PassBandKnobs = pass_band!(1, "PASS 2");
pub const PASS3: PassBandKnobs = pass_band!(2, "PASS 3");

table! {
    LISTED: Eq {
        SCALE = "GLOBAL", "SCALE", field!(scale), log(0.5, 2.0);
        TRIM = "GLOBAL", "TRIM", field!(trim_db), linear(-18.0, 18.0);
        PASS_TRIM = "PASS", "TRIM", field!(pass.trim_db), linear(-24.0, 24.0);
    }
}

const FLAT: [Param<Eq>; EQ_BANDS * EqBandKnobs::COUNT + PASS_BANDS * PassBandKnobs::COUNT + 3] = flat(&[&BAND1.all(), &BAND2.all(), &BAND3.all(), &BAND4.all(), &BAND5.all(), &BAND6.all(), &BAND7.all(), &BAND8.all(), &PASS1.all(), &PASS2.all(), &PASS3.all(), LISTED]);
pub const PARAMS: &[Param<Eq>] = &FLAT;
