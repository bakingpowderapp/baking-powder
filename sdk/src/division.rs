#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Division {
    Bars16,
    Bars12,
    Bars8,
    Bars4,
    Bars3,
    Bars2,
    BarDotted,
    Bar,
    HalfDotted,
    TwoThirds,
    Half,
    QuarterDotted,
    Third,
    #[default]
    Quarter,
    EighthDotted,
    Sixth,
    Eighth,
    SixteenthDotted,
    Twelfth,
    Sixteenth,
    ThirtySecondDotted,
    TwentyFourth,
    ThirtySecond,
    SixtyFourthDotted,
    FortyEighth,
    SixtyFourth,
}

pub const LABELS: [&str; 26] = ["16/1", "8/1.", "8/1", "4/1", "3/1", "2/1", "1/1.", "1/1", "1/2.", "2/3", "1/2", "1/4.", "1/3", "1/4", "1/8.", "1/6", "1/8", "1/16.", "1/12", "1/16", "1/32.", "1/24", "1/32", "1/64.", "1/48", "1/64"];

impl Division {
    pub const ALL: [Division; 26] = [
        Division::Bars16,
        Division::Bars12,
        Division::Bars8,
        Division::Bars4,
        Division::Bars3,
        Division::Bars2,
        Division::BarDotted,
        Division::Bar,
        Division::HalfDotted,
        Division::TwoThirds,
        Division::Half,
        Division::QuarterDotted,
        Division::Third,
        Division::Quarter,
        Division::EighthDotted,
        Division::Sixth,
        Division::Eighth,
        Division::SixteenthDotted,
        Division::Twelfth,
        Division::Sixteenth,
        Division::ThirtySecondDotted,
        Division::TwentyFourth,
        Division::ThirtySecond,
        Division::SixtyFourthDotted,
        Division::FortyEighth,
        Division::SixtyFourth,
    ];

    pub fn index(self) -> usize {
        Division::ALL.iter().position(|division| *division == self).unwrap_or(0)
    }

    pub fn name(self) -> &'static str {
        LABELS[self.index()]
    }

    pub fn from_name(name: &str) -> Option<Division> {
        LABELS.iter().position(|label| *label == name).map(|index| Division::ALL[index])
    }

    pub const KEYS: &'static [&'static str] = &LABELS;

    pub fn beats(self) -> f64 {
        match self {
            Division::Bars16 => 64.0,
            Division::Bars12 => 48.0,
            Division::Bars8 => 32.0,
            Division::Bars4 => 16.0,
            Division::Bars3 => 12.0,
            Division::Bars2 => 8.0,
            Division::BarDotted => 6.0,
            Division::Bar => 4.0,
            Division::HalfDotted => 3.0,
            Division::TwoThirds => 8.0 / 3.0,
            Division::Half => 2.0,
            Division::QuarterDotted => 1.5,
            Division::Third => 4.0 / 3.0,
            Division::Quarter => 1.0,
            Division::EighthDotted => 0.75,
            Division::Sixth => 4.0 / 6.0,
            Division::Eighth => 0.5,
            Division::SixteenthDotted => 0.375,
            Division::Twelfth => 4.0 / 12.0,
            Division::Sixteenth => 0.25,
            Division::ThirtySecondDotted => 0.1875,
            Division::TwentyFourth => 4.0 / 24.0,
            Division::ThirtySecond => 0.125,
            Division::SixtyFourthDotted => 0.09375,
            Division::FortyEighth => 4.0 / 48.0,
            Division::SixtyFourth => 0.0625,
        }
    }

    pub fn hz(self, bpm: f64) -> f64 {
        bpm / 60.0 / self.beats()
    }

    pub fn ms(self, bpm: f64) -> f64 {
        self.beats() * 60_000.0 / bpm.max(1.0)
    }

    pub fn nearest(ms: f64, bpm: f64, among: &[Division]) -> Division {
        let target = ms.ln();
        among.iter().copied().min_by(|a, b| (a.ms(bpm).ln() - target).abs().total_cmp(&(b.ms(bpm).ln() - target).abs())).unwrap_or_default()
    }
}
