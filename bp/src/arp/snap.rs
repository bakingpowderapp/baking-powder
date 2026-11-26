use super::{Articulation, Sink};
use sdk::notes::SEMITONES_PER_OCTAVE;
use sdk::schema::choice;

const FREE: u8 = u8::MAX;
const TOP: u8 = 127;
const PITCHES: usize = TOP as usize + 1;
const ROOT_SHIFT: u32 = 12;
const SIZES: [usize; SEMITONES_PER_OCTAVE as usize] = [0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 6];

choice!(pub Ties { Down = "DOWN" / "down", Up = "UP" / "up", Alternate = "ALT" / "alternate", Smart = "SMART" / "smart" });

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snap {
    pub key: u16,
    pub ties: Ties,
}

impl Snap {
    fn has(self, pitch: i32) -> bool {
        self.key >> pitch.rem_euclid(SEMITONES_PER_OCTAVE) & 1 == 1
    }

    fn near(self, midi: u8, up: bool) -> u8 {
        let first = if up { 1 } else { -1 };
        let inside = |pitch: &i32| (0..=i32::from(TOP)).contains(pitch) && self.has(*pitch);
        (0..SEMITONES_PER_OCTAVE).flat_map(|reach| [first * reach, -first * reach]).map(|step| i32::from(midi) + step).find(inside).map_or(midi, |pitch| pitch as u8)
    }

    fn torn(self, midi: u8) -> bool {
        self.near(midi, true) != self.near(midi, false)
    }

    fn climbs(self, midi: u8, bass: Option<u8>) -> bool {
        let above = |low: i32| (i32::from(midi) - low).rem_euclid(SEMITONES_PER_OCTAVE);
        let root = i32::from(self.key >> ROOT_SHIFT);
        let (from, base) = match bass.filter(|bass| above(i32::from(*bass)) != 0) {
            Some(bass) => (i32::from(bass), i32::from(self.near(bass, self.climbs(bass, None)))),
            None => (root, root),
        };
        let aim = (1..=SEMITONES_PER_OCTAVE).map(|rise| base + rise).filter(|pitch| self.has(*pitch)).nth(SIZES[above(from) as usize].wrapping_sub(1));
        aim.is_none_or(|aim| (aim - i32::from(midi)).rem_euclid(SEMITONES_PER_OCTAVE) < SEMITONES_PER_OCTAVE / 2)
    }

    fn settled(self, midi: u8) -> u8 {
        let up = match self.ties {
            Ties::Up => true,
            Ties::Down => false,
            Ties::Alternate | Ties::Smart => self.climbs(midi, None),
        };
        self.near(midi, up)
    }
}

fn slot(midi: u8) -> usize {
    usize::from(midi.min(TOP))
}

pub(super) struct Snapper {
    snap: Option<Snap>,
    sent: [u8; PITCHES],
    holders: [u8; PITCHES],
    held: usize,
    rose: bool,
    bass: Option<u8>,
    arriving: Option<u8>,
}

impl Snapper {
    pub(super) const fn new() -> Snapper {
        Snapper { snap: None, sent: [FREE; PITCHES], holders: [0; PITCHES], held: 0, rose: false, bass: None, arriving: None }
    }

    pub(super) fn set(&mut self, snap: Option<Snap>) {
        self.snap = snap;
    }

    pub(super) fn restart(&mut self) {
        self.rose = false;
    }

    pub(super) fn cleared(&self) -> Snapper {
        Snapper { snap: self.snap, ..Snapper::new() }
    }

    pub(super) fn lean(&mut self, bass: Option<u8>) {
        self.bass = bass;
    }

    pub(super) fn expect(&mut self, arriving: Option<u8>) {
        self.arriving = arriving;
    }

    fn landed(&mut self, snap: Snap, midi: u8) -> u8 {
        let up = match snap.ties {
            Ties::Up => true,
            Ties::Down => false,
            Ties::Smart => snap.climbs(midi, self.bass.into_iter().chain(self.arriving).min()),
            Ties::Alternate if snap.torn(midi) => {
                self.rose = !self.rose;
                self.rose
            }
            Ties::Alternate => true,
        };
        snap.near(midi, up)
    }

    fn hold(&mut self, midi: u8) -> (u8, bool) {
        if self.snap.is_none() && self.held == 0 {
            return (midi, false);
        }
        let key = slot(midi);
        if self.sent[key] != FREE {
            return (self.sent[key], false);
        }
        let out = match self.snap {
            Some(snap) => self.landed(snap, midi).min(TOP),
            None => midi.min(TOP),
        };
        let again = self.holders[slot(out)] > 0;
        self.sent[key] = out;
        self.holders[slot(out)] += 1;
        self.held += 1;
        (out, again)
    }

    fn lift(&mut self, midi: u8) -> Option<u8> {
        let key = slot(midi);
        let out = std::mem::replace(&mut self.sent[key], FREE);
        if out == FREE {
            return (self.holders[key] == 0).then_some(midi);
        }
        self.held -= 1;
        self.holders[slot(out)] -= 1;
        (self.holders[slot(out)] == 0).then_some(out)
    }

    fn struck(&mut self, sink: &mut impl Sink, midi: u8) -> u8 {
        let (midi, again) = self.hold(midi);
        if again {
            sink.note_off(midi);
        }
        midi
    }

    fn rebased(&self, articulation: Articulation, midi: u8, out: u8) -> Articulation {
        let Some(snap) = self.snap.filter(|_| articulation.shift != 0) else {
            return articulation;
        };
        let key = snap.settled((i32::from(midi) - i32::from(articulation.shift)).clamp(0, i32::from(TOP)) as u8);
        Articulation { shift: (i32::from(out) - i32::from(key)) as i8, ..articulation }
    }

    pub(super) fn note_on(&mut self, sink: &mut impl Sink, midi: u8, velocity: u8) {
        let midi = self.struck(sink, midi);
        sink.note_on(midi, velocity);
    }

    pub(super) fn strike(&mut self, sink: &mut impl Sink, midi: u8, velocity: u8, articulation: Articulation) {
        let out = self.struck(sink, midi);
        sink.strike(out, velocity, self.rebased(articulation, midi, out));
    }

    pub(super) fn note_off(&mut self, sink: &mut impl Sink, midi: u8) {
        if let Some(midi) = self.lift(midi) {
            sink.note_off(midi);
        }
    }

    pub(super) fn legato(&mut self, sink: &mut impl Sink, from: u8, to: u8, velocity: u8, articulation: Articulation) {
        let lifted = self.lift(from);
        let (out, again) = self.hold(to);
        let articulation = self.rebased(articulation, to, out);
        match lifted {
            Some(from) if !again => sink.legato(from, out, velocity, articulation),
            _ => {
                for midi in lifted.into_iter().chain(again.then_some(out)) {
                    sink.note_off(midi);
                }
                sink.strike(out, velocity, articulation);
            }
        }
    }

    pub(super) fn release(&mut self, sink: &mut impl Sink) {
        if self.held == 0 {
            return;
        }
        for midi in 0..=TOP {
            if self.holders[slot(midi)] > 0 {
                sink.note_off(midi);
            }
        }
        *self = Snapper { snap: self.snap, rose: self.rose, ..Snapper::new() };
    }
}
