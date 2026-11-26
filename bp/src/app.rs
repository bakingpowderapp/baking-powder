use crate::audio::{self, Audio, CoreSolo, Renderer, Seer};
use crate::browser::{self, Browser, Found, Ghost, Lifted, Under};
use coresynth::tv::{self, Shown, Stage, Tv, Which};
use sdk::division::Division;
use sdk::schema::{Fields, choice, flag, index, items, number};
use sdk::draw::{DrawList, Fill, Lens, Rect, Theme, Tone};
use sdk::face::{self, Act, Face, Faced, Format, Sketch};
use crate::flatten;
use sdk::font;
use fx::{Kind, Slot};
use fx::Picture;
use crate::grid::{self, ClipHit, ClipLook, Lane};
use crate::history::{self, History, Step};
use sdk::json::{self, Json};
use sdk::keyboard::{self, Key};
use crate::arp;
use crate::mcp;
use crate::midi;
use crate::palette::{Palette, SCALE as PALETTE_SCALE, Up};
use crate::png::Png;
use crate::project::{Aim, Automation, BEATS_PER_BAR, Bus, Chord, Clip, Cores, Curve, DECKS, DEFAULT_BPM, DELAY_MS, Denial, Document, Fade, Fades, File, Host, LONGEST_TICKS, Landed, LaneLfo, Layer, LfoField, NO_ROOM, Node, Note, Passage, Piece, Plan, Preset, Project, RACK_ROWS, Row, SEND_COUNT, SEND_NAMES, Slope, SongNote, Sounding, Span, TICKS_PER_BAR, TICKS_PER_BEAT, Track, VELOCITY, beats, bend_through, continues, gain_to_position, joined, level_among, moved_index, moved_lfos, next_times, pitch_range, position_to_gain, removed_index, room, split, stacked, taken, ticks, tidy, travel_at};
use crate::project::codec::wave_names;
use crate::rack::{self, Action, Offer, Pick, Rack};
use crate::record::{self, Take};
use crate::lanes::{self, Lanes};
use crate::roll::{self, Roll};
use crate::ruler::{self, Loop, Release, Ruler};
use crate::wav::{Refusal, Wave};
use std::sync::Arc;
use sdk::spectrum::{self, Scope, Target};
use crate::theory::{Fit, Root, ScaleName, flat_spelled, scale_pitch_classes};

macro_rules! remembered {
    (@$side:ident $s:tt, $other:ident, state, $field:ident: $ty:ty = $default:expr) => {};
    (@$side:ident $s:tt, $other:ident, hand, $field:ident: $ty:ty = $default:expr) => {};
    (@read $s:tt, $json:ident, flag, $field:ident: $ty:ty = $default:expr, $key:literal) => { $s.$field = flag(Some($json), $key, $default) };
    (@read $s:tt, $json:ident, whole, $field:ident: $ty:ty = $default:expr, $key:literal, $range:expr $(, |$read:ident| $settled:expr)?) => {
        $s.$field = (number(Some($json), $key, $default as f64) as i64).clamp(i64::from(*$range.start()), i64::from(*$range.end())) as $ty;
        $(let $read = $s.$field;
        $s.$field = $settled;)?
    };
    (@read $s:tt, $json:ident, number, $field:ident: $ty:ty = $default:expr, $key:literal, $range:expr) => { $s.$field = number(Some($json), $key, $default).clamp(*$range.start(), *$range.end()) };
    (@read $s:tt, $json:ident, choice, $field:ident: $ty:ty = $default:expr, $key:literal $(, $among:expr)?) => { $s.$field = $json.get($key).and_then(Json::as_str).and_then(<$ty>::from_name)$(.filter(|choice| $among.contains(choice)))?.unwrap_or($default) };
    (@write $s:tt, $fields:ident, flag, $field:ident: $ty:ty = $default:expr, $key:literal) => { $fields.flag($key, $s.$field, $default) };
    (@write $s:tt, $fields:ident, whole, $field:ident: $ty:ty = $default:expr, $key:literal, $range:expr $(, |$read:ident| $settled:expr)?) => { $fields.number($key, $s.$field as f64, $default as f64) };
    (@write $s:tt, $fields:ident, number, $field:ident: $ty:ty = $default:expr, $key:literal, $range:expr) => { $fields.number($key, $s.$field, $default) };
    (@write $s:tt, $fields:ident, choice, $field:ident: $ty:ty = $default:expr, $key:literal $(, $among:expr)?) => { $fields.name($key, $s.$field.name(), $default.name()) };
    ($name:ident { $($kind:ident $field:ident: $ty:ty = $default:expr $(, $key:literal $(, $arg:expr $(, |$read:ident| $settled:expr)?)?)?;)+ }) => {
        pub(super) struct $name {
            $(pub(super) $field: $ty,)+
        }

        impl $name {
            pub(super) fn new() -> $name {
                $name { $($field: $default,)+ }
            }

            pub(super) fn resume(&mut self, json: &Json) {
                $(remembered!(@read self, json, $kind, $field: $ty = $default $(, $key $(, $arg $(, |$read| $settled)?)?)?);)+
            }

            pub(super) fn session(&self, fields: &mut Fields) {
                $(remembered!(@write self, fields, $kind, $field: $ty = $default $(, $key $(, $arg $(, |$read| $settled)?)?)?);)+
            }
        }
    };
}

mod automation;
mod bands;
mod clips;
mod draw;
mod draw_controls;
mod draw_generator;
mod draw_tracks;
mod drags;
mod ducks;
mod edit;
mod files;
mod ghosts;
mod generator;
mod input;
mod keys;
mod layout;
mod pool;
mod presets;
mod readouts;
mod region;
mod rows;
mod serve;
mod session;
mod settings;
mod sorry;
mod tracks;
mod transport;
mod view;
mod wave;

use bands::*;
use layout::*;

const SENDS_PER_BANK: usize = 4;
const SEND_BANKS: usize = SEND_COUNT / SENDS_PER_BANK;
const UNREADABLE: &str = "CAN'T READ";
const TOO_LONG: &str = "TOO LONG";
const AUDIO: &str = "AUDIO";
const MIDI: &str = "MIDI";
const TRACK: &str = "TRACK";
const ROW: &str = "ROW";
const DEFAULT: &str = "DEFAULT";
const LOAD: &str = "LOAD";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    File,
    Export,
    Recent,
    Snap,
    Key,
    Scale,
    Fit,
    Add(usize),
    Follow(usize),
}

impl Menu {
    fn opener(self) -> Option<files::FileAction> {
        match self {
            Menu::Export => Some(files::FileAction::ExportProject),
            Menu::Recent => Some(files::FileAction::OpenRecent),
            Menu::File | Menu::Snap | Menu::Key | Menu::Scale | Menu::Fit | Menu::Add(_) | Menu::Follow(_) => None,
        }
    }

    fn root(self) -> Menu {
        match self {
            Menu::Follow(track) => Menu::Add(track),
            _ if self.opener().is_some() => Menu::File,
            _ => self,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Open { path: Option<String> },
    SaveAs { json: String, save: u64 },
    Write { path: String, json: String, save: u64 },
    OpenLink { url: String },
    Remember { json: String },
    Default { json: Option<String> },
    Fresh,
    Export { name: String, message: String },
    Exported { path: String, bytes: Vec<u8> },
    Midi { name: String, bytes: Vec<u8> },
    Wave { name: String, bytes: Vec<u8> },
    Preset { name: String, json: String },
    Buffer { frames: u32 },
    Copy { text: String },
    Folder { at: Option<String> },
    List { path: String },
    Scan { path: String },
    Rename { path: String, name: String },
    Delete { path: String },
    Reveal { path: String },
    Keep { path: String, text: String },
    Read { path: String },
    Stow { file: String, bytes: Vec<u8> },
    Drag { path: String, ghost: Ghost },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opened {
    Project,
    Preset,
    Refused,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dropped {
    Kept,
    Read,
    Document,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    Move { x: i32, y: i32 },
    Down { x: i32, y: i32, clicks: i32 },
    RightDown { x: i32, y: i32 },
    MiddleDown { x: i32, y: i32 },
    Up { x: i32, y: i32 },
    Scroll { x: i32, y: i32, dx: i32, dy: i32 },
    Leave,
    Modifiers { shift: bool, command: bool },
    Key { key: Key, shift: bool, command: bool, repeat: bool },
    KeyUp { key: Key },
    Blur,
    Focus,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    Main,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Button {
    Menu(Menu),
    Play,
    Stop,
    Record,
    Metronome,
    Keyboard,
    Chord,
    Hold,
    Automation,
    Lock,
    Higher,
    Lower,
    Big,
    Group,
    Bounce,
    Flatten,
    Close,
    CopyMcp,
    Save,
    Stock,
    Show(Aside),
    MasterName,
    Send(usize),
    Cycle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hover {
    Nothing,
    Button(Button),
    Item(usize),
    Setting(&'static str),
    Version(usize),
    Edit(usize),
    Track(usize, tracks::TrackButton),
    Append(usize),
    Tile(usize),
    Fold(usize),
    Column,
    Volume(usize),
    Master,
    TrackSend(usize, usize),
    Clip(usize, usize, ClipHit),
    Lane(usize),
    Feedback,
    Tempo,
    Clock,
    Palette(usize),
    Face,
    Load,
    Confirm,
    Cancel,
    Lfo(usize),
    Curve(usize),
    Clear(usize),
    SaveOption(usize),
    StockOption(usize),
    Rack,
    Hertz(i32),
    Wave,
    WaveHead,
    WaveBody(i32),
    WaveFade(Slope, wave::FadePart),
    WaveLane(wave::SpanHit),
    WaveZoom,
    WaveLoop,
    WaveReverse,
    WaveSave,
    WaveAll,
    Browser,
    Lanes,
    Roll,
    Ruler,
    Zoom,
}

#[derive(Clone, Copy)]
enum Mark {
    Play,
    Stop,
    Record,
    Cycle,
    Metronome,
    Keyboard,
    Heart,
    Up,
    Down,
    Right,
    Chord,
    Hold,
    Automation,
    Lock,
    Group,
    Robot,
}

impl Mark {
    fn size(self) -> (i32, i32) {
        match self {
            Mark::Play | Mark::Stop | Mark::Record => (11, 11),
            Mark::Cycle => (10, 8),
            Mark::Metronome => (13, 12),
            Mark::Keyboard => (16, 10),
            Mark::Heart => (9, 8),
            Mark::Up | Mark::Down | Mark::Right => (9, 9),
            Mark::Chord => (11, 7),
            Mark::Hold => (13, 8),
            Mark::Automation | Mark::Group => (13, 9),
            Mark::Lock => (11, 12),
            Mark::Robot => (13, 11),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Strip {
    Track(usize),
    Curve(usize),
}

choice!(Auto { Off = "off", Track = "track", All = "all" });

impl Auto {
    fn next(self) -> Auto {
        Auto::ALL[(self.index() + 1) % Auto::ALL.len()]
    }
}

enum Drag {
    Contrast(settings::StepDrag),
    ScreenZoom(settings::StepDrag),
    Preview(settings::StepDrag),
    Buffer(settings::BufferDrag),
    Tempo(transport::TempoDrag),
    Zoom(drags::ZoomDrag),
    ZoomAt(drags::ZoomAt),
    Track(tracks::TrackDrag),
    Clip(clips::ClipDrag),
    Chord(clips::ChordDrag),
    Ghost(ghosts::GhostDrag),
    Marquee(clips::Marquee),
    Region(region::RegionDrag),
    TrackMarquee(tracks::TrackMarquee),
    Volume(tracks::Gang),
    Delay(tracks::Gang),
    Master(tracks::MasterDrag),
    Send(tracks::SendDrag),
    Auto { host: Host, armed: automation::Armed, slot: usize, drag: automation::AutoDrag },
    Head,
    WaveZoom(wave::WaveZoomDrag),
    WaveZoomAt(wave::WaveZoomAt),
    Select(wave::SelectDrag),
    Span(wave::SpanDrag),
    Fade(wave::FadeDrag),
}

struct Notice {
    band: &'static str,
    name: String,
    reason: &'static str,
}

fn stem_of(path: &str) -> String {
    std::path::Path::new(path).file_stem().and_then(|stem| stem.to_str()).map_or_else(|| String::from(path), String::from)
}

choice!(Aside { Blank = "blank", Spectrum = "spectrum", Browser = "browser", Wave = "wave", Pattern = "pattern", Picture = "picture" });

pub struct App {
    width: i32,
    height: i32,
    project: Project,
    fresh: Project,
    history: History,
    gesture: Option<(u64, f32)>,
    defaults: Vec<Row>,
    palettes: [Palette; DECKS],
    face: Face,
    lfo_face: Face,
    rack: Rack,
    roll: Roll,
    ruler: Ruler,
    roll_note: Option<(usize, u8)>,
    open: Option<usize>,
    save: Option<presets::SavePanel>,
    stock: Option<presets::StockPanel>,
    notice: Option<Notice>,
    ask: Option<files::Ask>,
    picture: Picture,
    tv: Tv,
    lane_sketch: Sketch,
    sketch_lane: Option<(Host, automation::Armed)>,
    sorry: sorry::Sorry,
    command: bool,
    requests: Vec<Request>,
    disk: files::Disk,
    screen: Screen,
    hover: Hover,
    bar_under: Option<i32>,
    menu: Option<Menu>,
    fits: Vec<Fit>,
    settings: settings::Settings,
    wave_face: Face,
    wave_view: Option<wave::WaveView>,
    rename: Option<keys::Rename>,
    drag: Option<Drag>,
    monitored: Option<Host>,
    generators: Vec<generator::Generator>,
    soloed: Option<usize>,
    timeline: layout::Timeline,
    selection: tracks::Selection,
    live: keys::Live,
    view: view::View,
    audio: Option<Audio>,
    playing: bool,
    play_from: i32,
    playhead: f64,
    shift: bool,
    browser: Browser,
    factory: Option<String>,
    lanes: Lanes,
    armed: Option<automation::Armed>,
    armed_returns: [Option<automation::Armed>; SEND_COUNT + 1],
    lfo_strip: Option<usize>,
    ducks: ducks::Ducks,
    cut_flash: Option<(i32, f32)>,
    clipboard: edit::Clipboard,
    pool: pool::Pool,
    agent: serve::Agent,
    list: DrawList,
}

fn counted(count: i32, word: &str) -> String {
    if count == 1 { format!("1 {word}") } else { format!("{count} {word}S") }
}

fn needing(short: usize, word: &str) -> String {
    format!("NEEDS {short} MORE {word}{}", if short == 1 { "" } else { "S" })
}

fn lacking(name: &str, short: usize, word: &str) -> String {
    if short == 0 { String::from(name) } else { format!("{name} \u{b7} {}", needing(short, word)) }
}

fn turned_away(name: &str, denial: Denial) -> String {
    match denial {
        Denial::NoRoom(short) => lacking(name, short, ROW),
        Denial::EffectsOnly => String::from(name),
    }
}

fn bars(ticks: i32) -> i32 {
    (ticks + TICKS_PER_BAR - 1) / TICKS_PER_BAR
}

impl Default for App {
    fn default() -> App {
        App::new()
    }
}
