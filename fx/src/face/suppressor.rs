use super::*;

const SLOPE_DESC: &str = "Crossover slope in dB/octave, or linear phase with about 9 ms latency";
const CROSSOVER_DESC: &str = "The band's lower crossover";

fn beyond<const N: usize>(p: &Suppressor) -> bool {
    p.count() <= N
}

macro_rules! band_view {
    ($n:literal, $band:literal, $knobs:expr) => {
        viewed(
            [
                title(2, 0, $band),
                desc(unless(wide(value(Rise, 3, 0, "DEPTH", $knobs.depth, Tenths), 4), beyond::<$n>), "How much a resonance is cut"),
                desc(unless(wide(value(Rise, 3, 4, "DETAIL", $knobs.detail, Tenths), 4), beyond::<$n>), "Cut width; higher is narrower and deeper"),
                desc(unless(wide(value(Rise, 3, 8, "ATTACK", $knobs.attack, Tenths), 4), beyond::<$n>), "How quickly a cut sets in"),
                desc(unless(wide(value(Rise, 3, 12, "RELEASE", $knobs.release, Tenths), 4), beyond::<$n>), "How quickly a cut lets go"),
            ],
            $n,
        )
    };
}

const VIEW1: [Cell<Suppressor>; 5] = band_view!(0, "BAND 1", params::suppressor::BAND1);
const VIEW2: [Cell<Suppressor>; 5] = band_view!(1, "BAND 2", params::suppressor::BAND2);
const VIEW3: [Cell<Suppressor>; 5] = band_view!(2, "BAND 3", params::suppressor::BAND3);
const VIEW4: [Cell<Suppressor>; 5] = band_view!(3, "BAND 4", params::suppressor::BAND4);

const SHARED: [Cell<Suppressor>; 5] = [
    title(4, 0, "CROSSOVER"),
    desc(held(unless(wide(switch(5, 0, FreakSlope::LABELS, "BANDS", "SLOPE", choose!(FreakSlope::ALL, slope)), 4), beyond::<1>)), SLOPE_DESC),
    desc(paged(unless(wide(value(Rise, 5, 4, "FREQUENCY", params::suppressor::BAND_2_CROSSOVER, Hertz), 4), beyond::<1>), 1), CROSSOVER_DESC),
    desc(paged(unless(wide(value(Rise, 5, 4, "FREQUENCY", params::suppressor::BAND_3_CROSSOVER, Hertz), 4), beyond::<2>), 2), CROSSOVER_DESC),
    desc(paged(unless(wide(value(Rise, 5, 4, "FREQUENCY", params::suppressor::BAND_4_CROSSOVER, Hertz), 4), beyond::<3>), 3), CROSSOVER_DESC),
];

pub(crate) const CELLS: [Cell<Suppressor>; 25] = flat(&[&VIEW1, &VIEW2, &VIEW3, &VIEW4, &SHARED]);
