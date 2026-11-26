use super::{Aim, Automation, Curve, LaneLfo, Layer, Project, RACK_ROWS, Row, TRACKS};
use std::borrow::Cow;
use std::ops::Range;

const COPY_SEED_SHIFT: u32 = 53;

impl Row {
    pub const MAX_TIMES: usize = sdk::schema::MAX_TIMES;

    pub fn times(&self) -> usize {
        let times = match self {
            Row::Midi(midi) => midi.times,
            Row::Core(layer) => layer.times,
            Row::Effect(slot) => slot.times,
            Row::Bus(_) => 1,
        };
        times.clamp(1, Row::MAX_TIMES)
    }

    pub fn set_times(&mut self, times: usize) {
        let times = times.clamp(1, Row::MAX_TIMES);
        match self {
            Row::Midi(midi) => midi.times = times,
            Row::Core(layer) => layer.times = times,
            Row::Effect(slot) => slot.times = times,
            Row::Bus(_) => {}
        }
    }

    pub fn repeats(&self, above: &Row) -> bool {
        let bare = |row: &Row| {
            let mut row = *row;
            row.set_times(1);
            if let Row::Core(layer) = &mut row {
                layer.seed = 0;
            }
            row
        };
        (self.is_midi() || self.is_core()) && bare(self) == bare(above)
    }

    fn copy(&self, place: u64, copy: usize, leaving: bool) -> Row {
        let mut row = *self;
        row.set_times(1);
        match &mut row {
            Row::Core(layer) => {
                let seed = if layer.seed == 0 { place } else { layer.seed };
                layer.seed = seed.wrapping_add((copy as u64) << COPY_SEED_SHIFT);
                layer.times = usize::from(!leaving);
            }
            Row::Midi(midi) => midi.times = usize::from(!leaving),
            Row::Effect(_) | Row::Bus(_) => {}
        }
        row
    }
}

impl Layer {
    pub fn leaving(&self) -> bool {
        self.times == 0
    }
}

pub fn taken(rack: &[Row]) -> usize {
    rack.iter().map(Row::times).sum()
}

pub fn room(rack: &[Row]) -> usize {
    RACK_ROWS.saturating_sub(taken(rack))
}

fn passes(rack: &[Row]) -> Vec<usize> {
    let mut passes: Vec<usize> = rack.iter().map(Row::times).collect();
    let mut over = taken(rack).saturating_sub(RACK_ROWS.max(rack.len()));
    for held in passes.iter_mut().rev() {
        let cut = over.min(*held - 1);
        *held -= cut;
        over -= cut;
    }
    passes
}

fn fit(rack: &mut [Row]) {
    for (row, held) in passes(rack).into_iter().enumerate() {
        rack[row].set_times(held);
    }
}

pub fn next_times(rack: &[Row], row: usize) -> (usize, bool) {
    let held = rack.get(row).filter(|entry| !entry.is_bus()).map_or(Row::MAX_TIMES, Row::times);
    let stopped = held < Row::MAX_TIMES && room(rack) == 0;
    (if held == Row::MAX_TIMES || stopped { 1 } else { held + 1 }, stopped)
}

fn spread(rack: &[Row], lingering: &[usize]) -> Vec<(usize, usize)> {
    let kept = passes(rack);
    let mut spare = RACK_ROWS.saturating_sub(kept.iter().sum());
    let mut leaving = |row: usize| {
        let held = lingering.get(row).copied().unwrap_or(0).min(spare);
        spare -= held;
        held
    };
    kept.into_iter().enumerate().map(|(row, kept)| (kept, leaving(row))).collect()
}

pub fn lingering(rack: &[Row], wanted: &[usize]) -> Vec<usize> {
    spread(rack, wanted).into_iter().map(|(_, leaving)| leaving).collect()
}

pub fn places(rack: &[Row]) -> Vec<usize> {
    places_with(rack, &[])
}

pub fn places_with(rack: &[Row], lingering: &[usize]) -> Vec<usize> {
    let mut places = vec![0];
    for (kept, leaving) in spread(rack, lingering) {
        places.push(places[places.len() - 1] + kept + leaving);
    }
    places
}

pub fn copies(places: &[usize], row: usize) -> Range<usize> {
    let place = |row: usize| match places.get(row) {
        Some(place) => *place,
        None => places.last().map_or(row, |last| last + row + 1 - places.len()),
    };
    place(row)..place(row + 1)
}

pub fn played(rack: &[Row], track: usize) -> Vec<Row> {
    played_with(rack, track, &[])
}

pub fn played_with(rack: &[Row], track: usize, lingering: &[usize]) -> Vec<Row> {
    if plain(rack, lingering) {
        return rack.to_vec();
    }
    let mut played = Vec::new();
    for (row, (entry, (kept, leaving))) in rack.iter().zip(spread(rack, lingering)).enumerate() {
        played.extend((0..kept + leaving).map(|copy| entry.copy((track * RACK_ROWS + row) as u64, copy, copy >= kept)));
    }
    let master = copies(&places_with(rack, lingering), 0).end.saturating_sub(1);
    for row in &mut played {
        if let Row::Bus(bus) = row {
            bus.master = master;
        }
    }
    played
}

fn plain(rack: &[Row], lingering: &[usize]) -> bool {
    rack.iter().all(|row| row.times() == 1) && lingering.iter().all(|held| *held == 0)
}

fn aimed(aim: &Aim, places: &[usize]) -> Vec<Aim> {
    let Some(row) = aim.row() else {
        return vec![aim.clone()];
    };
    let landed = |at: usize| {
        let mut aim = aim.clone();
        aim.remap(&|_| Some(at));
        aim
    };
    copies(places, row).map(landed).collect()
}

fn expand(rack: &mut Vec<Row>, automation: &mut Automation, track: usize, lingering: &[usize]) {
    if plain(rack, lingering) {
        return;
    }
    let places = places_with(rack, lingering);
    *rack = played_with(rack, track, lingering);
    automation.curves = automation.curves.iter().flat_map(|curve| aimed(&curve.aim, &places).into_iter().map(move |aim| Curve { aim, ..curve.clone() })).collect();
    automation.lfos = automation.lfos.iter().flat_map(|lfo| aimed(&lfo.aim, &places).into_iter().map(move |aim| LaneLfo { aim, ..lfo.clone() })).collect();
}

impl Project {
    pub fn expanded<'a>(&self, lingering: impl Fn(usize) -> &'a [usize]) -> Cow<'_, Project> {
        if self.tracks.iter().enumerate().all(|(index, track)| plain(&track.rack, lingering(index))) && self.returns().all(|ret| plain(&ret.rack, &[])) {
            return Cow::Borrowed(self);
        }
        let mut project = self.clone();
        for (index, track) in project.tracks.iter_mut().enumerate() {
            expand(&mut track.rack, &mut track.automation, index, lingering(index));
        }
        for ret in project.returns_mut() {
            expand(&mut ret.rack, &mut ret.automation, TRACKS, &[]);
        }
        Cow::Owned(project)
    }

    pub fn fit_times(&mut self) {
        for track in &mut self.tracks {
            fit(&mut track.rack);
        }
        for ret in self.returns_mut() {
            fit(&mut ret.rack);
        }
    }
}
