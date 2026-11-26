use super::*;

choice!(pub Vary { Trigger = "Trigger" / "trigger", Quarter = "1/4" / "1/4", Eighth = "1/8" / "1/8", Sixteenth = "1/16" / "1/16", Auto = "Auto" / "auto" });

choice!(pub RepeatMix { Mix = "Mix" / "mix", Insert = "Insert" / "insert", Gate = "Gate" / "gate" });

choice!(pub Grid { Bar = "1/1", Half = "1/2", Third = "1/3", Quarter = "1/4", Sixth = "1/6", Eighth = "1/8", Twelfth = "1/12", Sixteenth = "1/16", TwentyFourth = "1/24", ThirtySecond = "1/32", FortyEighth = "1/48", SixtyFourth = "1/64", NinetySixth = "1/96", OneTwentyEighth = "1/128", OneNinetySecond = "1/192", TwoFiftySixth = "1/256" });

const GRID_DIVISORS: [u32; 16] = [1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256];

impl Grid {
    pub fn beats(self) -> f64 {
        4.0 / f64::from(GRID_DIVISORS[self.index()])
    }
}

pub const INTERVALS: [Division; 8] = [Division::Bars4, Division::Bars2, Division::Bar, Division::Half, Division::Quarter, Division::Eighth, Division::Sixteenth, Division::ThirtySecond];

pub const INTERVAL_LABELS: [&str; 8] = ["4/1", "2/1", "1/1", "1/2", "1/4", "1/8", "1/16", "1/32"];

params!(Repeat {
    pick interval: Division = Division::Bar, "interval";
    whole offset: usize = 0, "offsetSixteenths", 0.0..=15.0;
    pick grid: Grid = Grid::Sixteenth, "grid";
    custom gate: usize = 4, "gateSixteenths", "whole number 1 to 64, default 4", |p, d, f| f.lead("gateSixteenths", Json::Number(p.gate as f64), p.gate == d.gate), |json, d| number(json, "gateSixteenths", d.gate as f64).clamp(1.0, 64.0) as usize;
    lead chance = 1.0, "chance", 0.0..=1.0;
    number variation = 0.0, "variation", 0.0..=1.0;
    choice vary: Vary = Vary::Trigger, "vary";
    nested filter1: Filter = Repeat::LOW_CUT, "filter1";
    nested filter2: Filter = Repeat::HIGH_CUT, "filter2";
    pick mix: RepeatMix = RepeatMix::Mix, "mixMode";
    number volume_db = 0.0, "outputDb", -24.0..=6.0;
    number decay = 0.0, "levelDecay", 0.0..=1.0;
});

impl Repeat {
    pub const LOW_CUT: Filter = Filter { pass: Pass::High, hz: 100.0, resonant: true, ..Filter::DEFAULT };
    pub const HIGH_CUT: Filter = Filter { hz: 8000.0, resonant: true, ..Filter::DEFAULT };
    pub const FILTERS: usize = 2;

    pub fn filters(&self) -> [Filter; Repeat::FILTERS] {
        [self.filter1, self.filter2]
    }
}

impl Filtered for Repeat {
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

pub const FILTER1: FilterKnobs<Repeat> = filter_params!("FILTER 1", filter1);
pub const FILTER2: FilterKnobs<Repeat> = filter_params!("FILTER 2", filter2);

table! {
    LISTED: Repeat {
        OFFSET = "TIME", "OFFSET", int!(offset), linear(0.0, 15.0);
        GATE = "TIME", "GATE", int!(gate), linear(1.0, 64.0);
        CHANCE = "CHANCE", "CHANCE", field!(chance), UNIT;
        VARIATION = "CHANCE", "VARIATION", field!(variation), UNIT;
        VOLUME = "OUT", "VOLUME", field!(volume_db), linear(-24.0, 6.0);
        DECAY = "OUT", "DECAY", field!(decay), UNIT;
    }
}

static FLAT: [Param<Repeat>; LISTED.len() + 2 * FilterKnobs::<Repeat>::COUNT] = flat(&[LISTED, &FILTER1.all(), &FILTER2.all()]);
pub static PARAMS: &[Param<Repeat>] = &FLAT;
