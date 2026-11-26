use super::*;

fn set_shape(p: &mut Eq, band: usize, v: f64) {
    let shape = choice(Shape::ALL, v);
    p.bands[band].shape = shape;
    p.bands[band].q = p.bands[band].q.min(shape.max_q());
}

fn set_on(p: &mut Eq, band: usize, v: f64) {
    p.bands[band].on = v >= 0.5;
    if !p.bands[band].on && p.solo == Some(band) {
        p.solo = None;
    }
}

fn set_solo(p: &mut Eq, band: usize, v: f64) {
    if v >= 0.5 {
        p.solo = Some(band);
    } else if p.solo == Some(band) {
        p.solo = None;
    }
}

fn asleep(band: &EqBand) -> bool {
    !band.on || !band.moving()
}

pub(crate) fn soloed(eq: &Eq) -> Option<&EqBand> {
    eq.solo.and_then(|index| eq.bands.get(index)).filter(|band| band.on)
}

macro_rules! eq_band {
    ($n:expr, $label:literal, $band:literal, $params:expr) => {
        [
            desc(toggle($n, 0, $label, $band, "ON", field!(|p| if p.bands[$n].on { 1.0 } else { 0.0 }, |p, v| set_on(p, $n, v))), "Enable band"),
            paged(desc(inert(pick($n, 1, 1, $band, "SHAPE", field!(|p| p.bands[$n].shape.index() as f64, |p, v| set_shape(p, $n, v)), Shape::LABELS), |p| !p.bands[$n].on), "Filter shape; Plus quadruples slope"), 0),
            desc(inert(value(Rise, $n, 2, "FRQ", $params.frequency, Hertz), |p| !p.bands[$n].on), "Band frequency"),
            desc(inert(refuses(value(Centre, $n, 3, "GAI", $params.gain, Decibels), |p| !p.bands[$n].shape.has_gain()), |p| !p.bands[$n].on || !p.bands[$n].shape.has_gain()), "Band gain; maximum dynamic gain with DYN"),
            desc(inert(refuses(value(Rise, $n, 4, "Q", $params.q, TwoPlaces), |p| !p.bands[$n].shape.has_q()), |p| !p.bands[$n].on || !p.bands[$n].shape.has_q()), "Band Q"),
            paged(desc(inert(pick($n, 5, 2, $band, "CHANNEL", choose!(Channel::ALL, bands[$n].channel), Channel::LABELS), |p| !p.bands[$n].on), "Stereo, mid or side processing"), 0),
            desc(unless(toggle($n, 7, "SOL", $band, "SOLO", field!(|p| if p.solo == Some($n) { 1.0 } else { 0.0 }, |p, v| set_solo(p, $n, v))), |p| !p.bands[$n].on), "Solo band input"),
            paged(desc(inert(refuses(toggle($n, 1, "DYN", $band, "DYNAMIC", flag!(bands[$n].dynamic)), |p| !p.bands[$n].shape.has_gain()), |p| !p.bands[$n].on || !p.bands[$n].shape.has_gain()), "Apply gain as the band exceeds its surroundings"), 1),
            paged(desc(inert(value(Rise, $n, 5, "THR", $params.threshold, DecibelsUnsigned), |p| asleep(&p.bands[$n])), "Threshold above surrounding energy; white noise reads 0 dB"), 1),
            paged(desc(inert(value(Rise, $n, 8, "RTO", $params.ratio, Ratio), |p| asleep(&p.bands[$n])), "Ratio above THR; gain capped at GAI"), 1),
            paged(desc(inert(value(Rise, $n, 9, "KNE", $params.knee, DecibelsUnsigned), |p| asleep(&p.bands[$n])), "Dynamic knee width"), 1),
            paged(desc(inert(value(Rise, $n, 10, "ATK", $params.attack, Millis), |p| asleep(&p.bands[$n])), "Attack at 1 kHz; scales with frequency"), 1),
            paged(desc(inert(value(Rise, $n, 11, "REL", $params.release, Millis), |p| asleep(&p.bands[$n])), "Release at 1 kHz; scales with frequency"), 1),
            paged(desc(inert(value(Rise, $n, 12, "LNK", $params.link, Percent), |p| asleep(&p.bands[$n])), "Stereo detector link; 0% independent"), 1),
            paged(desc(inert(value(Pan, $n, 13, "BAL", $params.balance, Balance), |p| asleep(&p.bands[$n])), "Dynamic gain balance between channels"), 1),
        ]
    };
}

pub(super) const BAND_CELLS: usize = 15;
const BAND_VIEWS: usize = 2;
const PASS_VIEW: usize = 2;

const fn banded(mut cells: [Cell<Eq>; BAND_CELLS]) -> [Cell<Eq>; BAND_CELLS] {
    let mut i = 0;
    while i < BAND_CELLS {
        if cells[i].page.is_none() {
            cells[i].page = Some(0);
            cells[i].pages = BAND_VIEWS;
        }
        i += 1;
    }
    cells
}

const EQ1: [Cell<Eq>; BAND_CELLS] = banded(eq_band!(0, "1", "BAND 1", params::eq::BAND1));

const EQ2: [Cell<Eq>; BAND_CELLS] = banded(eq_band!(1, "2", "BAND 2", params::eq::BAND2));

const EQ3: [Cell<Eq>; BAND_CELLS] = banded(eq_band!(2, "3", "BAND 3", params::eq::BAND3));

const EQ4: [Cell<Eq>; BAND_CELLS] = banded(eq_band!(3, "4", "BAND 4", params::eq::BAND4));

const EQ5: [Cell<Eq>; BAND_CELLS] = banded(eq_band!(4, "5", "BAND 5", params::eq::BAND5));

const EQ6: [Cell<Eq>; BAND_CELLS] = banded(eq_band!(5, "6", "BAND 6", params::eq::BAND6));

const EQ7: [Cell<Eq>; BAND_CELLS] = banded(eq_band!(6, "7", "BAND 7", params::eq::BAND7));

const EQ8: [Cell<Eq>; BAND_CELLS] = banded(eq_band!(7, "8", "BAND 8", params::eq::BAND8));

const EQ_GLOBAL: [Cell<Eq>; 2] = [
    paged(desc(value(Rise, 0, 11, "SCL", params::eq::SCALE, Percent), "Scale all band gains"), 0),
    paged(desc(value(Centre, 0, 12, "TRM", params::eq::TRIM, Decibels), "EQ output gain"), 0),
];

const EQ_VIEWS: [Cell<Eq>; 3] = [
    desc(page(0, 15, "EQ", "VIEW", "SHAPES", 0), "Shapes, channels, scale and trim"),
    desc(page(1, 15, "DYN", "VIEW", "DYNAMICS", 1), "Dynamic band controls"),
    desc(page(2, 15, "BP", "VIEW", "BAND-PASS", PASS_VIEW), "Three band-pass bands ahead of the EQ bands"),
];

fn pass_off(p: &Eq) -> bool {
    !p.pass.on
}

macro_rules! pass_band {
    ($n:expr, $number:literal, $params:expr) => {
        [
            desc(inert(wide(value(Rise, $n + 4, 2, concat!("FREQUENCY ", $number), $params.frequency, Hertz), 4), pass_off), "Band-pass frequency"),
            desc(inert(wide(value(Rise, $n + 4, 6, concat!("Q ", $number), $params.q, TwoPlaces), 4), pass_off), "Band-pass Q"),
            desc(inert(wide(value(Rise, $n + 4, 10, concat!("LEVEL ", $number), $params.level, Percent), 4), pass_off), "Band-pass level; 0 is off"),
        ]
    };
}

const EQ_PASS: [Cell<Eq>; 4] = [
    title(1, 2, "BAND-PASS"),
    desc(wide(toggle(2, 2, "ON", "PASS", "ON", flag!(pass.on)), 4), "Pass only three bands on to the EQ bands"),
    desc(inert(wide(value(Centre, 2, 6, "TRIM", params::eq::PASS_TRIM, Decibels), 4), pass_off), "Band-pass output gain"),
    title(3, 2, "BANDS"),
];

const EQ_PASS1: [Cell<Eq>; 3] = pass_band!(0, "1", params::eq::PASS1);

const EQ_PASS2: [Cell<Eq>; 3] = pass_band!(1, "2", params::eq::PASS2);

const EQ_PASS3: [Cell<Eq>; 3] = pass_band!(2, "3", params::eq::PASS3);

const EQ_PASSES: [Cell<Eq>; PASS_BANDS * 3 + 4] = viewed(flat(&[&EQ_PASS, &EQ_PASS1, &EQ_PASS2, &EQ_PASS3]), PASS_VIEW);

pub(crate) static CELLS: [Cell<Eq>; EQ_BANDS * BAND_CELLS + PASS_BANDS * 3 + 9] = flat(&[&EQ1, &EQ2, &EQ3, &EQ4, &EQ5, &EQ6, &EQ7, &EQ8, &EQ_GLOBAL, &EQ_VIEWS, &EQ_PASSES]);
