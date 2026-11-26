use super::*;

params!(SuppressorBand {
    flag on = true, "on";
    flag solo = false, "solo";
    flag mute = false, "mute";
    number depth = 5.0, "depth", 0.0..=10.0;
    number detail = 5.0, "detail", 0.0..=10.0;
    number attack = 5.0, "attack", 0.0..=10.0;
    number release = 3.0, "release", 0.0..=10.0;
});

fn bands_manual() -> Json {
    let mut fields = Fields::new();
    SuppressorBand::manual(&SuppressorBand::DEFAULT, "", &band_description, &mut fields);
    Json::Array(vec![Json::from("4 bands, low to high; bandCount selects active bands"), fields.into_json()])
}

fn band_description(key: &str) -> String {
    String::from(match key {
        "on" => "off leaves the band's range uncut",
        "solo" => "hear this band alone; needs on",
        "mute" => "silence the band's range; needs on",
        "depth" => "how much a resonance is cut",
        "detail" => "cut width; higher is narrower and deeper",
        "attack" => "how quickly a cut sets in",
        "release" => "how quickly a cut lets go",
        _ => "",
    })
}

params!(Suppressor {
    custom band_count: usize = 1, "bandCount", format!("whole number 1 to {SUPPRESSOR_BANDS}, default 1: bands in use, from the lowest"), |p, d, f| f.number("bandCount", p.band_count as f64, d.band_count as f64), |json, d| number(json, "bandCount", d.band_count as f64).clamp(1.0, SUPPRESSOR_BANDS as f64) as usize;
    custom crossovers_hz: [f64; SUPPRESSOR_BANDS - 1] = [120.0, 800.0, 5000.0], "crossoversHz", "3 numbers in Hz, 30 to 20000, each at least 1.2 times the one below; default 120, 800, 5000", |p, d, f| if p.crossovers_hz != d.crossovers_hz { f.push("crossoversHz", Json::Array(p.crossovers_hz.iter().map(|hz| Json::Number(*hz)).collect())) }, |json, d| crossovers(json, d.crossovers_hz, Suppressor::MIN_CROSSOVER_HZ, Suppressor::MAX_CROSSOVER_HZ);
    choice slope: FreakSlope = FreakSlope::Linkwitz, "crossoverSlope";
    custom bands: [SuppressorBand; SUPPRESSOR_BANDS] = [SuppressorBand::DEFAULT; SUPPRESSOR_BANDS], "bands", bands_manual(), |p, d, f| if p.bands != d.bands { f.push("bands", Json::Array(p.bands.iter().map(|band| nested(|fields| band.fields(&SuppressorBand::DEFAULT, fields))).collect())) }, |json, d| {
        let mut bands = d.bands;
        for (slot, item) in bands.iter_mut().zip(items(json, "bands")) {
            *slot = SuppressorBand::from_json(Some(item), SuppressorBand::DEFAULT);
        }
        bands
    };
});

impl Suppressor {
    pub const LOOKAHEAD_MS: f64 = 5.0;
    pub const MIN_CROSSOVER_HZ: f64 = 30.0;
    pub const MAX_CROSSOVER_HZ: f64 = 20_000.0;
    pub const CROSSOVER_RATIO: f64 = 1.2;

    pub fn count(&self) -> usize {
        self.band_count.clamp(1, SUPPRESSOR_BANDS)
    }

    pub fn lookahead(&self, sample_rate: f64) -> usize {
        (Suppressor::LOOKAHEAD_MS * 0.001 * sample_rate).round() as usize
    }

    pub fn latency(&self, sample_rate: f64) -> usize {
        self.lookahead(sample_rate) + if self.slope == FreakSlope::Linear && self.count() > 1 { sdk::dsp::crossover::half(sample_rate) } else { 0 }
    }
}

bank!(pub SuppressorKnobs: Suppressor { depth, detail, attack, release });

macro_rules! band {
    ($n:expr, $band:literal) => {
        SuppressorKnobs {
            depth: Param::new($band, "DEPTH", field!(bands[$n].depth), linear(0.0, 10.0)),
            detail: Param::new($band, "DETAIL", field!(bands[$n].detail), linear(0.0, 10.0)),
            attack: Param::new($band, "ATTACK", field!(bands[$n].attack), linear(0.0, 10.0)),
            release: Param::new($band, "RELEASE", field!(bands[$n].release), linear(0.0, 10.0)),
        }
    };
}

pub const BAND1: SuppressorKnobs = band!(0, "BAND 1");
pub const BAND2: SuppressorKnobs = band!(1, "BAND 2");
pub const BAND3: SuppressorKnobs = band!(2, "BAND 3");
pub const BAND4: SuppressorKnobs = band!(3, "BAND 4");

table! {
    LISTED: Suppressor {
        BAND_2_CROSSOVER = "BAND 2", "CROSSOVER", field!(crossovers_hz[0]), log(Suppressor::MIN_CROSSOVER_HZ, Suppressor::MAX_CROSSOVER_HZ);
        BAND_3_CROSSOVER = "BAND 3", "CROSSOVER", field!(crossovers_hz[1]), log(Suppressor::MIN_CROSSOVER_HZ, Suppressor::MAX_CROSSOVER_HZ);
        BAND_4_CROSSOVER = "BAND 4", "CROSSOVER", field!(crossovers_hz[2]), log(Suppressor::MIN_CROSSOVER_HZ, Suppressor::MAX_CROSSOVER_HZ);
    }
}

static FLAT: [Param<Suppressor>; 19] = flat(&[&BAND1.all(), &BAND2.all(), &BAND3.all(), &BAND4.all(), LISTED]);
pub static PARAMS: &[Param<Suppressor>] = &FLAT;
