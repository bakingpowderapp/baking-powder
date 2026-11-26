pub const OCTAVE_REACH: i8 = 3;
pub const SEMITONE_REACH: i8 = 12;
pub const CENTS_REACH: i8 = 50;
pub const FORCE_REACH: i8 = 1;
pub const SEMITONES_PER_OCTAVE: i32 = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Join {
    Play,
    Tie,
    Slide,
}

impl Join {
    pub fn next(self) -> Join {
        match self {
            Join::Play => Join::Tie,
            Join::Tie => Join::Slide,
            Join::Slide => Join::Play,
        }
    }

    pub fn spell(self) -> &'static str {
        match self {
            Join::Play => "PLAY",
            Join::Tie => "TIE",
            Join::Slide => "SLIDE",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Articulation {
    pub cents: f32,
    pub modulation: Option<f32>,
    pub shift: i8,
    pub join: Join,
}

impl Articulation {
    pub const PLAIN: Articulation = Articulation { cents: 0.0, modulation: None, shift: 0, join: Join::Play };
    pub const MAX_CENTS: f32 = 2.0 * CENTS_REACH as f32;
    pub const MAX_SHIFT: i8 = SEMITONE_REACH + OCTAVE_REACH * SEMITONES_PER_OCTAVE as i8 + 2 * FORCE_REACH;

    pub fn stepped(cents: f64, modulation: f64, shift: i32, join: Join) -> Articulation {
        Articulation { cents: cents as f32, modulation: Some(modulation as f32), shift: shift as i8, join }
    }
}

pub trait Sink {
    fn note_on(&mut self, midi: u8, velocity: u8);
    fn note_off(&mut self, midi: u8);
    fn strike(&mut self, midi: u8, velocity: u8, articulation: Articulation);
    fn legato(&mut self, from: u8, to: u8, velocity: u8, articulation: Articulation);
    fn at(&mut self, _: i64) {}
}
