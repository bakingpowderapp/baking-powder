use super::*;
use std::cell::Cell;

mod meters;
mod sync;

pub(super) use sync::plays;

const QUEUE: usize = 4096;

#[derive(Clone, PartialEq)]
struct Mirror {
    gain: f32,
    heard: bool,
    pours: bool,
    unmuted: bool,
    twinned: bool,
    group: Option<usize>,
    sends: [f32; SEND_COUNT],
    rack: Vec<Row>,
    places: Vec<usize>,
    written: Vec<Row>,
    lingering: Vec<usize>,
    solo: Option<Alone>,
    level: (f32, f32),
    gauges: [f32; RACK_ROWS],
    signals: [f32; RACK_ROWS],
    strips: [Strips; RACK_ROWS],
    plays: Vec<Play>,
    was: Option<usize>,
}

#[derive(Clone, PartialEq, Debug)]
struct SendMirror {
    gain: f32,
    rack: Vec<Row>,
    places: Vec<usize>,
    level: (f32, f32),
    gauges: [f32; RACK_ROWS],
    signals: [f32; RACK_ROWS],
    strips: [Strips; RACK_ROWS],
}

impl SendMirror {
    const DEFAULT: SendMirror = SendMirror { gain: 1.0, rack: Vec::new(), places: Vec::new(), level: (0.0, 0.0), gauges: [0.0; RACK_ROWS], signals: [0.0; RACK_ROWS], strips: NO_STRIPS };
}

#[derive(Clone, Copy, Default)]
struct Batch {
    begun: bool,
    messages: usize,
    retires: usize,
}

pub(super) struct Link {
    sender: SyncSender<Message>,
    held: RefCell<VecDeque<Message>>,
    batch: Cell<Option<Batch>>,
    batches: Cell<u64>,
    sample_rate: f64,
    max_frames: usize,
}

fn unpack(word: u64) -> (f64, u16, u16) {
    (f64::from(f32::from_bits((word >> 32) as u32)), (word >> 16) as u16, word as u16)
}

pub struct Audio {
    pub(super) link: Link,
    reclaim: Receiver<Displaced>,
    retired: Arc<AtomicUsize>,
    meters: Arc<Meters>,
    playhead: Arc<AtomicU64>,
    plays: u16,
    from: f64,
    tracks: Vec<Mirror>,
    sends: [SendMirror; SEND_COUNT],
    bpm: f64,
    key: u16,
    song: Song,
    leaders: ([u8; TRACKS], [u8; TRACKS]),
    curves: Curves,
    lfos: Lfos,
    return_curves: Curves,
    return_lfos: Lfos,
    master: SendMirror,
    solo: Option<usize>,
    metronome: bool,
    range: Option<(f64, f64)>,
    clip_hold: (f32, f32),
    load: f32,
    analyzer: Analyzer,
    spectra: [Spectrum; SCOPES],
    cuts: [[f32; sdk::spectrum::CURVE]; SCOPES],
    aimed: [Target; SCOPES],
    seer: Seer,
    recipe: Option<Recipe>,
    twin_seer: Seer,
    twin_recipe: Option<Recipe>,
    shared: bool,
    progress: Option<f32>,
    timing: Timing,
    cleared: bool,
    beats: Vec<(Arc<Wave>, Arc<Beats>)>,
}

impl Link {
    pub(super) fn send(&self, message: Message) {
        if let Some(mut batch) = self.batch.get() {
            let retires = message.retires();
            if batch.begun && (batch.messages == BATCH || batch.retires + retires > BUDGET) {
                self.post(Message::Split);
                batch = Batch::default();
            }
            if !batch.begun {
                self.post(Message::Begin);
            }
            self.batch.set(Some(Batch { begun: true, messages: batch.messages + 1, retires: batch.retires + retires }));
        }
        self.post(message);
    }

    pub(super) fn open(&self) {
        if self.batch.get().is_none() {
            self.batch.set(Some(Batch::default()));
        }
    }

    pub(super) fn commit(&self) {
        if self.batch.take().is_some_and(|batch| batch.begun) {
            self.batches.set(self.batches.get() + 1);
            self.post(Message::Commit);
        }
    }

    pub(super) fn sending(&self) -> bool {
        self.batch.get().is_some() || !self.held.borrow().is_empty()
    }

    pub(super) fn batches(&self) -> u64 {
        self.batches.get()
    }

    fn post(&self, message: Message) {
        let mut held = self.held.borrow_mut();
        if !held.is_empty() {
            held.push_back(message);
            return;
        }
        if let Err(TrySendError::Full(message)) = self.sender.try_send(message) {
            held.push_back(message);
        }
    }

    pub(super) fn flush(&self) {
        let mut held = self.held.borrow_mut();
        while let Some(message) = held.pop_front() {
            if let Err(TrySendError::Full(message)) = self.sender.try_send(message) {
                held.push_front(message);
                return;
            }
        }
    }

    pub(super) fn insert(&self, row: Row, twinned: bool) -> Insert {
        Insert::new(row, self.sample_rate, self.max_frames, twinned)
    }

    pub(super) fn rack(&self, rows: &[Row]) -> Vec<Insert> {
        let mut rack = Vec::with_capacity(RACK_ROWS);
        rack.extend(rows.iter().map(|row| self.insert(*row, false)));
        rack
    }

    pub(super) fn twins(&self, rows: &[Row], twinned: bool) -> Vec<Option<Box<Twin>>> {
        rows.iter()
            .map(|row| match row {
                Row::Effect(slot) if twinned => Some(Box::new(Twin::new(slot, self.sample_rate, self.max_frames))),
                _ => None,
            })
            .collect()
    }

}

impl Audio {
    pub fn open(sample_rate: f64, max_frames: usize, seer: Seer) -> (Audio, Renderer) {
        let (sender, receiver) = sync_channel(QUEUE);
        let (reclaim_sender, reclaim) = sync_channel(RECLAIM);
        let retired = Arc::new(AtomicUsize::new(0));
        let meters = Arc::new(Meters {
            tracks: (0..TRACKS).map(|_| Meter::new()).collect(),
            track_ears: (0..TRACKS).map(|_| Mutex::new(None)).collect(),
            sends: std::array::from_fn(|_| Meter::new()),
            send_ears: std::array::from_fn(|_| Mutex::new(None)),
            master: Meter::new(),
            track_gauges: (0..TRACKS).map(|_| gauges()).collect(),
            send_gauges: std::array::from_fn(|_| gauges()),
            master_gauges: gauges(),
            taps: std::array::from_fn(|_| Tap::new()),
            dropouts: AtomicU32::new(0),
            load: AtomicU32::new(0),
            lane: Lane::default().cells(),
            sequenced: (0..TRACKS).map(|_| std::array::from_fn(|_| Lane::UNCLOCKED.cells())).collect(),
            preview_progress: unpublished(),
            rehearsal_beat: unpublished(),
        });
        let playhead = Arc::new(AtomicU64::new(pack(0.0, 0, 0)));
        let audio = Audio {
            link: Link { sender, held: RefCell::new(VecDeque::new()), batch: Cell::new(None), batches: Cell::new(0), sample_rate, max_frames },
            reclaim,
            retired: Arc::clone(&retired),
            meters: Arc::clone(&meters),
            playhead: Arc::clone(&playhead),
            plays: 0,
            from: 0.0,
            tracks: Vec::new(),
            sends: [SendMirror::DEFAULT; SEND_COUNT],
            bpm: DEFAULT_BPM,
            key: 0,
            song: Vec::new(),
            leaders: sync::unled(),
            curves: Vec::new(),
            lfos: Vec::new(),
            return_curves: Vec::new(),
            return_lfos: Vec::new(),
            master: SendMirror::DEFAULT,
            solo: None,
            metronome: false,
            range: None,
            clip_hold: (0.0, 0.0),
            load: 0.0,
            analyzer: Analyzer::new(sample_rate),
            spectra: std::array::from_fn(|_| Spectrum::new()),
            cuts: [[0.0; sdk::spectrum::CURVE]; SCOPES],
            aimed: [Target::NONE; SCOPES],
            twin_seer: seer.sibling(),
            seer,
            recipe: None,
            twin_recipe: None,
            shared: true,
            progress: None,
            timing: Timing::default(),
            cleared: false,
            beats: Vec::new(),
        };
        let table = Arc::new(Table::new(max_frames, cap(sample_rate), Arc::clone(&meters)));
        table.warm();
        let renderer = Renderer::new(receiver, reclaim_sender, retired, meters, table, playhead, sample_rate, max_frames);
        (audio, renderer)
    }

    pub fn reclaim(&self) {
        while let Ok(displaced) = self.reclaim.try_recv() {
            self.retired.fetch_sub(1, Ordering::Relaxed);
            match displaced {
                Displaced::Song(song) => drop(song),
                Displaced::Curves(curves) => drop(curves),
                Displaced::Lfos(lfos) => drop(lfos),
                Displaced::Lagged(lagged) => drop(lagged),
                Displaced::Rack(rack) => drop(rack),
                Displaced::Buffer(buffer) => drop(buffer),
                Displaced::Insert(insert) => drop(insert),
                Displaced::Plays(plays) => drop(plays),
                Displaced::Preview(cue) => drop(cue),
                Displaced::Rehearsal(rehearsal) => drop(rehearsal),
                Displaced::Vision(vision) => drop(vision),
                Displaced::Plan(plan) => drop(plan),
                Displaced::Track(track) => drop(track),
                Displaced::Line(line) => drop(line),
                Displaced::TwinLines(lines) => drop(lines),
                Displaced::TwinUnit(unit) => drop(unit),
                Displaced::TwinUnits(units) => drop(units),
            }
        }
    }

    fn deliver(&self, rendered: Vec<Rendered>) {
        for Rendered { host, index, vision } in rendered {
            if self.shared && host.track().and_then(|track| self.tracks.get(track)).is_some_and(|mirror| mirror.twinned) {
                self.link.send(Message::Vision { host, index, vision: vision.clone(), twin: true });
            }
            self.link.send(Message::Vision { host, index, vision, twin: false });
        }
    }

    fn deliver_twins(&self, rendered: Vec<Rendered>) {
        for Rendered { host, index, vision } in rendered {
            self.link.send(Message::Vision { host, index, vision, twin: true });
        }
    }

    pub fn timing(&self) -> &Timing {
        &self.timing
    }

    pub fn position(&self) -> Position {
        let (beat, wraps, plays) = unpack(self.playhead.load(Ordering::Relaxed));
        Position { beat: if plays == self.plays { beat } else { self.from }, wraps }
    }

    pub fn lane(&self) -> Lane {
        Lane::load(&self.meters.lane)
    }

    pub fn noted(&self, track: usize, row: usize) -> Option<Lane> {
        let row = self.tracks.get(track).map_or(row, |mirror| copies(&mirror.places, row).start);
        self.meters.sequenced.get(track).and_then(|rows| rows.get(row)).and_then(|cells| Lane::load(cells).clocked())
    }

    pub fn play(&mut self, from: f64) {
        self.plays = self.plays.wrapping_add(1);
        self.from = from;
        self.link.send(Message::Play { from });
    }

    pub fn stop(&self) {
        self.link.send(Message::Stop);
    }

    pub fn preview(&self, cue: Option<Box<Cue>>) {
        self.link.send(Message::Preview(cue));
    }

    pub fn beats_of(&mut self, wave: &Arc<Wave>) -> Arc<Beats> {
        sync::beats_of(&mut self.beats, wave)
    }

    pub fn preview_gain(&self, gain: f32) {
        self.link.send(Message::PreviewGain { gain });
    }

    pub fn preview_progress(&self) -> Option<f64> {
        published(&self.meters.preview_progress)
    }

    pub fn rehearsal_beat(&self) -> Option<f64> {
        published(&self.meters.rehearsal_beat)
    }

    pub fn rehearse(&self, rehearsal: Option<Box<Rehearsal>>) {
        self.link.send(Message::Rehearse(rehearsal));
    }

    pub fn place(&self, beat: f64) {
        self.link.send(Message::Place { beat });
    }

    pub fn set_loop(&mut self, range: Option<(f64, f64)>) {
        if self.range != range {
            self.range = range;
            self.link.send(Message::Loop { range });
        }
    }

    pub fn note_on(&self, track: usize, midi: u8, velocity: u8) {
        self.link.send(Message::NoteOn { track, midi, velocity });
    }

    pub fn note_off(&self, track: usize, midi: u8) {
        self.link.send(Message::NoteOff { track, midi });
    }

}
