use super::*;

const MAIN: [Cell<Repeat>; 16] = [
    title(1, 2, "INTERVAL"),
    desc(pick(2, 2, 3, "TIME", "INTERVAL", choose!(&INTERVALS, interval), &INTERVAL_LABELS), "How often a new slice is captured"),
    desc(wide(value(Rise, 2, 5, "OFFSET", params::repeat::OFFSET, Sixteenths), 3), "Moves the capture point later, in sixteenths"),
    title(1, 8, "GRID"),
    desc(pick(2, 8, 3, "TIME", "GRID", choose!(Grid::ALL, grid), Grid::KEYS), "Length of the slice that repeats, a bar to 1/256"),
    desc(wide(value(Rise, 2, 11, "GATE", params::repeat::GATE, Sixteenths), 3), "How long the repeats run, in sixteenths"),
    title(3, 2, "CHANCE"),
    desc(wide(value(Rise, 4, 2, "CHANCE", params::repeat::CHANCE, Percent), 4), "Odds that a capture point repeats at all"),
    desc(wide(value(Rise, 4, 6, "VARIATION", params::repeat::VARIATION, Percent), 4), "How far the grid wanders from GRID"),
    title(3, 10, "VARY"),
    desc(pick(4, 10, 4, "CHANCE", "VARY", choose!(Vary::ALL, vary), Vary::LABELS), "When the grid wanders: at the trigger, on a beat, or every repeat"),
    title(5, 2, "MIX"),
    desc(pick(6, 2, 3, "OUT", "MIX", choose!(RepeatMix::ALL, mix), RepeatMix::LABELS), "Repeats over the dry, instead of it, or alone"),
    title(5, 5, "OUTPUT"),
    desc(wide(value(Rise, 6, 5, "VOLUME", params::repeat::VOLUME, Decibels), 3), "Level of the repeats"),
    desc(wide(value(Rise, 6, 8, "DECAY", params::repeat::DECAY, Percent), 3), "Each repeat quieter than the last"),
];

const FILTERS: [Cell<Repeat>; 1] = [desc(wide(open(6, 11, "FILTER", "FILTER", "OPEN", 0, 2), 3), "Two filters on the repeats; opens them on the screen")];
const FILTER1: [Cell<Repeat>; 6] = filter_cells!(0, "FILTER 1", "the repeats", params::repeat::FILTER1, filter1);
const FILTER2: [Cell<Repeat>; 6] = filter_cells!(1, "FILTER 2", "the repeats", params::repeat::FILTER2, filter2);

pub(crate) static CELLS: [Cell<Repeat>; 29] = flat(&[&MAIN, &FILTERS, &FILTER1, &FILTER2]);
