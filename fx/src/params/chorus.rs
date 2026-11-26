use super::*;

choice!(pub ChorusMode { Standard = "Standard" / "standard", Vintage = "Vintage" / "vintage", Ensemble = "Ensemble" / "ensemble", Spatial = "Spatial" / "spatial", Classic = "Classic" / "classic" });

params!(Chorus {
    pick mode: ChorusMode = ChorusMode::Standard, "mode";
    switch sync = false, "sync";
    pick division: Division = Division::Quarter, "division";
    lead rate_hz = 0.5, "rateHz", RATE_MIN_HZ..=RATE_MAX_HZ;
    lead depth = 0.5, "depth", 0.0..=Chorus::MAX_DEPTH;
    number center_ms = 11.0, "centerMs", 1.0..=40.0;
    number far_center_ms = 0.0, "farCenterMs", 0.0..=Chorus::MAX_FAR_CENTER_MS;
    number far_phase = 0.5, "farPhase", 0.0..=1.0;
    number spread = 1.0, "spread", 0.0..=1.0;
    number start_phase = 0.0, "startPhase", 0.0..=1.0;
    number feedback = 0.0, "feedback", 0.0..=1.0;
    nested filter1: Filter = Chorus::LOW_CUT, "filter1";
    nested filter2: Filter = Chorus::HIGH_CUT, "filter2";
    number width = 1.0, "width", 0.0..=1.5;
    number trim_db = 0.0, "trimDb", -24.0..=24.0;
    flag crossfade = false, "crossfade";
    flag dual = false, "dual";
});

impl Chorus {
    pub const MAX_DEPTH: f64 = 5.0;
    pub const MAX_FAR_CENTER_MS: f64 = 75.0;
    pub const LOW_CUT: Filter = Filter { pass: Pass::High, hz: 80.0, resonant: true, ..Filter::DEFAULT };
    pub const HIGH_CUT: Filter = Filter { on: true, slope: Slope::Six, hz: 16_000.0, ..Filter::DEFAULT };
    pub const FILTERS: usize = 2;

    pub fn filters(&self) -> [Filter; Chorus::FILTERS] {
        [self.filter1, self.filter2]
    }
}
