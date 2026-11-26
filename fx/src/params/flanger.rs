use super::*;

choice!(pub FlangerMode { Standard = "Standard" / "standard", ThruZero = "ThruZero" / "thru-zero", Vintage = "Vintage" / "vintage", Tape = "Tape" / "tape" });

choice!(pub FlangerShape { Sine = "Sine" / "sine", Triangle = "Triangle" / "triangle", Smoothed = "Smoothed" / "smoothed", Log = "Log" / "log" });

params!(Flanger {
    pick mode: FlangerMode = FlangerMode::Standard, "mode";
    flag peak = false, "peakPolarity";
    choice shape: FlangerShape = FlangerShape::Smoothed, "shape";
    switch sync = false, "sync";
    pick division: Division = Division::Quarter, "division";
    lead rate_hz = 0.4, "rateHz", RATE_MIN_HZ..=RATE_MAX_HZ;
    lead depth = 0.5, "depth", 0.0..=1.0;
    number center_ms = 3.0, "centerMs", 0.1..=15.0;
    number spread = 0.7, "spread", 0.0..=1.0;
    number start_phase = 0.0, "startPhase", 0.0..=1.0;
    lead feedback = 0.45, "feedback", -1.0..=1.0;
    number width = 1.0, "width", 0.0..=1.5;
    number dry = 0.0, "dry", 0.0..=1.0;
});
