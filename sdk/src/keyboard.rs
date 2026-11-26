#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Space,
    Return,
    Escape,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Char(char),
}

impl Key {
    pub fn vertical(self) -> Option<i32> {
        match self {
            Key::Up => Some(1),
            Key::Down => Some(-1),
            _ => None,
        }
    }
}

pub const OCTAVE_MIN: i32 = -1;
pub const OCTAVE_MAX: i32 = 7;
pub const DEFAULT_OCTAVE: i32 = 3;

const NOTE_KEYS: [char; 18] = ['a', 'w', 's', 'e', 'd', 'f', 't', 'g', 'y', 'h', 'u', 'j', 'k', 'o', 'l', 'p', ';', '\''];

pub fn pitch(key: char, octave: i32) -> Option<u8> {
    let semitone = NOTE_KEYS.iter().position(|note| *note == key)? as i32;
    u8::try_from((octave + 1) * 12 + semitone).ok().filter(|pitch| *pitch <= 127)
}

pub fn octave_step(key: char) -> Option<i32> {
    match key {
        'z' => Some(-1),
        'x' => Some(1),
        _ => None,
    }
}
