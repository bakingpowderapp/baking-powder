use super::*;
use super::files::{ADD_ITEMS, DELAY, EXPORT_ITEMS, FILE_ITEMS, FOLLOW, FileAction};
use super::tracks::{DropSpot, TRACK_BUTTONS, TrackDrag};
use crate::project::{LEAST_BPM, MOST_BPM};

pub(super) const BORDER: i32 = 8;
pub(super) const RAIL_H: i32 = 45;
pub(super) const CONTROL_BORDER: i32 = 2;
pub(super) const HEADER_PAD: i32 = 4;
pub(super) const CONTROL_H: i32 = RAIL_H - BORDER - 2 * HEADER_PAD;
pub(super) const TEXT_SCALE: i32 = 2;
pub(super) const TEXT_PAD: i32 = 10;
pub(super) const TILE: i32 = 120;
pub(super) const DRAG_ARM_PX: i32 = 4;
pub(super) const FOLD_MARK_W: i32 = 9;
pub(super) const LFO_CELLS_H: i32 = 4 * face::PITCH + face::FRAME;
pub(super) const VOL_HANDLE_H: i32 = 6;
pub(super) const ASK_NAME_SCALE: i32 = 4;
pub(super) const ASK_LEADING: i32 = 6;

const STAGE_H: i32 = face::H - 2 * face::FRAME + BORDER;
const CORE_W: i32 = face::W - 2 * face::FRAME + BORDER;
const ITEM_H: i32 = 32;
const MENU_W: i32 = 200;
const RECENT_CHARS: usize = 24;
const CURVE_TILE: i32 = 206;
const SLIP_PX: i32 = TILE / 4;
const CELL_H: i32 = 28;
const APPEND_H: i32 = 2 * BORDER + 2 * CELL_H - 3 * CONTROL_BORDER;
const TITLE_H: i32 = font::GLYPH_H * TEXT_SCALE + 2 * HEADER_PAD;
const VOL_W: i32 = 28;
const SQUARES_W: i32 = face::CELL + 2 * (BORDER - face::FRAME);
const MASTER_W: i32 = SENDS_PER_BANK as i32 * (CELL_H + BORDER) + BORDER;
const ASK_W: i32 = 400;
const WAVE_LANE_ROWS: i32 = 2;
const WAVE_ZOOM_SPAN: i32 = 2;
const ASK_H: i32 = 2 * BORDER + 2 * TEXT_PAD + font::GLYPH_H * (TEXT_SCALE + ASK_NAME_SCALE + face::LABEL_SCALE) + 2 * ASK_LEADING;
const EXPORT_BAR_H: i32 = RAIL_H - BORDER;
const EXPORT_H: i32 = 2 * BORDER + 2 * TEXT_PAD + font::GLYPH_H * (TEXT_SCALE + ASK_NAME_SCALE) + 2 * ASK_LEADING + EXPORT_BAR_H;
pub(super) const CANCEL: &str = "CANCEL";
pub(super) const WARMING: &str = "WARMING UP";

pub(super) fn file_cell() -> Rect {
    Rect::new(0, 0, TILE, RAIL_H + BORDER)
}

pub(super) fn file_inner() -> Rect {
    tile_inner(file_cell())
}

pub(super) fn block_cell() -> Rect {
    Rect::new(TILE - BORDER, 0, VOL_W + 2 * BORDER, RAIL_H + BORDER)
}

remembered!(Timeline {
    whole track_scroll: i32 = 0, "track_scroll", 0..=i32::MAX;
    whole grid_scroll: i32 = 0, "grid_scroll", 0..=i32::MAX;
    number beat_w: f64 = grid::BEAT_W, "beat_w", grid::BEAT_W_MIN..=grid::BEAT_W_MAX;
    choice snap: Division = grid::SNAP_DEFAULT, "snap", grid::SNAPS;
});

impl Timeline {
    pub(super) fn replace(&mut self) {
        self.track_scroll = 0;
    }

    pub(super) fn grid_step(&self) -> i32 {
        grid::step_of(self.snap)
    }

    pub(super) fn beat_under(&self, x: i32) -> f64 {
        f64::from(x - column_w() + self.grid_scroll) / self.beat_w
    }

    pub(super) fn clip_w(&self, length: i32) -> i32 {
        (beats(length) * self.beat_w).round() as i32 - 2 * grid::inset(self.beat_w)
    }

    pub(super) fn zoom_travel(&self) -> f32 {
        ((self.beat_w / grid::BEAT_W_MIN).log2() / (grid::BEAT_W_MAX / grid::BEAT_W_MIN).log2()) as f32
    }
}

pub(super) enum Wears {
    Mark(Mark),
    Word(String),
    Fold(String, Mark),
    Readout(String),
    Meter(String, String, f32, bool),
}

pub(super) struct Head {
    pub(super) hover: Hover,
    wide: &'static str,
    narrow: &'static str,
    pub(super) shown: fn(&App) -> bool,
    pub(super) on: fn(&App) -> bool,
    pub(super) wears: fn(&App) -> Wears,
}

const fn head(hover: Hover, wide: &'static str, narrow: &'static str, on: fn(&App) -> bool, wears: fn(&App) -> Wears) -> Head {
    Head { hover, wide, narrow, shown: |_| true, on, wears }
}

const fn mark(button: Button, on: fn(&App) -> bool, wears: fn(&App) -> Wears) -> Head {
    head(Hover::Button(button), "", "", on, wears)
}

pub(super) const HEADER: [Head; 21] = [
    mark(Button::Play, |app| app.playing, |_| Wears::Mark(Mark::Play)),
    mark(Button::Stop, |_| false, |_| Wears::Mark(Mark::Stop)),
    mark(Button::Record, |app| app.live.take.is_some(), |_| Wears::Mark(Mark::Record)),
    mark(Button::Metronome, |app| app.project.metronome, |_| Wears::Mark(Mark::Metronome)),
    mark(Button::Keyboard, |app| app.live.keyboard, |_| Wears::Mark(Mark::Keyboard)),
    head(Hover::Clock, "00:00", "0:00", |_| false, |app| Wears::Readout(app.clock())),
    head(Hover::Tempo, "TEMPO", "300", |_| false, |app| {
        let bpm = app.project.bpm;
        let readout = if app.shift || bpm.fract() != 0.0 { format!("{bpm:.2}") } else { (bpm as i32).to_string() };
        let name = if app.worded() { String::from("TEMPO") } else { (bpm.round() as i32).to_string() };
        Wears::Meter(name, readout, ((bpm - LEAST_BPM) / (MOST_BPM - LEAST_BPM)) as f32, matches!(app.drag, Some(Drag::Tempo(_))))
    }),
    head(Hover::Button(Button::Menu(Menu::Snap)), "1/16.", "1/16.", |app| app.menu_open(Menu::Snap), |app| Wears::Word(String::from(app.timeline.snap.name()))),
    head(Hover::Button(Button::Menu(Menu::Key)), "Db", "Db", |app| app.menu_open(Menu::Key), |app| Wears::Word(flat_spelled(app.project.root.name()))),
    head(Hover::Button(Button::Menu(Menu::Scale)), "MIXOLYDIAN", "MIX", |app| app.menu_open(Menu::Scale), |app| {
        let scale = ScaleName::LABELS[app.project.scale.index()];
        Wears::Word(String::from(if app.worded() { scale } else { &scale[..SCALE_SHORT] }))
    }),
    mark(Button::Higher, |_| false, |_| Wears::Mark(Mark::Up)),
    mark(Button::Lower, |_| false, |_| Wears::Mark(Mark::Down)),
    head(Hover::Button(Button::Chord), "CHORD", "", |app| app.view.palette_shown, |_| Wears::Fold(String::from("CHORD"), Mark::Chord)),
    head(Hover::Button(Button::Hold), "HOLD", "", |app| app.live.hold, |_| Wears::Fold(String::from("HOLD"), Mark::Hold)),
    head(Hover::Button(Button::Automation), "AUTO", "", |app| app.view.auto != Auto::Off, |app| Wears::Fold(String::from(if app.view.auto == Auto::All { "ALL" } else { "AUTO" }), Mark::Automation)),
    mark(Button::Lock, |app| app.project.lock, |_| Wears::Mark(Mark::Lock)),
    Head { shown: |app| app.settings.mcp, ..mark(Button::CopyMcp, |app| app.settings.copied > 0.0, |_| Wears::Mark(Mark::Robot)) },
    head(Hover::Button(Button::Big), "BIG", "BIG", |app| app.view.big, |_| Wears::Word(String::from("BIG"))),
    Head { shown: |app| app.group_label().is_some(), ..head(Hover::Button(Button::Group), "UNGROUP", "", |_| false, |app| Wears::Fold(String::from(app.group_label().unwrap_or_default()), Mark::Group)) },
    Head { shown: |app| app.bounce_span().is_some(), ..head(Hover::Button(Button::Bounce), "BOUNCE", "BOUNCE", |_| false, |_| Wears::Word(String::from("BOUNCE"))) },
    Head { shown: |app| !app.flatten_targets().is_empty(), ..head(Hover::Button(Button::Flatten), "FLATTEN", "FLATTEN", |_| false, |_| Wears::Word(String::from("FLATTEN"))) },
];

const FOLD_W: i32 = 1280;
pub(super) const FEEDBACK: &str = "FEEDBACK";
pub(super) const REV: &str = "REV";
pub(super) const SCALE_SHORT: usize = 3;

impl Strip {
    pub(super) fn height(self) -> i32 {
        match self {
            Strip::Track(_) => TILE,
            Strip::Curve(_) => CURVE_TILE,
        }
    }
}

fn header_inner_w(label: &str) -> i32 {
    if label.is_empty() { RAIL_H - BORDER } else { font::width(label, TEXT_SCALE) + 2 * TEXT_PAD }
}

fn header_label(index: usize, folded: bool) -> &'static str {
    if folded { HEADER[index].narrow } else { HEADER[index].wide }
}

fn feedback_w(folded: bool) -> i32 {
    header_inner_w(if folded { "" } else { FEEDBACK }) + 2 * BORDER
}

fn rev_w(folded: bool) -> i32 {
    if folded { header_inner_w(REV) + 2 * BORDER } else { feedback_w(false) }
}

pub(super) const ASK_BUTTONS: [Hover; 2] = [Hover::Cancel, Hover::Confirm];

pub(super) fn close_cell(width: i32) -> Rect {
    let size = RAIL_H + BORDER;
    Rect::new(width - size, 0, size, size)
}

pub(super) fn close_inner(width: i32) -> Rect {
    tile_inner(close_cell(width))
}

pub(super) fn label_y(rect: Rect) -> i32 {
    rect.y + (rect.h - font::GLYPH_H * TEXT_SCALE) / 2
}

pub(super) fn wash_and_ink(on: bool, hover: bool) -> (Option<Tone>, Tone) {
    match (on, hover) {
        (true, _) => (Some(Tone::Ink100), Tone::Ground),
        (false, true) => (Some(Tone::Ink25), Tone::Ink100),
        (false, false) => (None, Tone::Ink100),
    }
}

pub(super) fn tile_inner(tile: Rect) -> Rect {
    Rect::new(tile.x + BORDER, tile.y + BORDER, tile.w - 2 * BORDER, tile.h - 2 * BORDER)
}

pub(super) fn tile_title(tile: Rect) -> Rect {
    let inner = tile_inner(tile);
    Rect::new(inner.x, inner.y, inner.w, TITLE_H)
}

pub(super) fn fold_mark(tile: Rect) -> Rect {
    let inner = tile_inner(tile);
    Rect::new(inner.x, inner.y, 2 * HEADER_PAD + FOLD_MARK_W, TITLE_H)
}

pub(super) fn name_x(tile: Rect, group: bool) -> i32 {
    tile_inner(tile).x + HEADER_PAD + if group { FOLD_MARK_W + HEADER_PAD } else { 0 }
}

pub(super) fn tile_volume(tile: Rect) -> Rect {
    Rect::new(tile.x + tile.w - BORDER, tile.y, VOL_W + 2 * BORDER, tile.h)
}

pub(super) fn volume_inner(tile: Rect) -> Rect {
    tile_inner(tile_volume(tile))
}

pub(super) fn column_w() -> i32 {
    let column = tile_volume(Rect::new(0, 0, TILE, TILE));
    column.x + column.w
}

pub(super) fn rack_w() -> i32 {
    column_w() - 2 * BORDER + 2 * face::FRAME
}

pub(super) fn lane_rect(tile: Rect, width: i32) -> Rect {
    Rect::new(column_w(), tile.y + BORDER, width - column_w(), tile.h - 2 * BORDER)
}

pub(super) fn lfo_origin(tile: Rect) -> (i32, i32) {
    let inner = tile_inner(Rect::new(tile.x, tile.y, column_w(), tile.h));
    (inner.x, tile.y + tile.h - BORDER - LFO_CELLS_H)
}

pub(super) fn decibels(gain: f32) -> String {
    if gain <= 0.0 {
        return String::from("-inf");
    }
    let db = 20.0 * gain.log10();
    if db.abs() < 0.05 { String::from("0.0dB") } else { format!("{db:+.1}dB") }
}

pub(super) fn milliseconds(ms: f32) -> String {
    let tenths = (ms * 10.0).round() / 10.0;
    if tenths == 0.0 { String::from("0ms") } else { format!("{tenths:+}ms") }
}

pub(super) fn fader_gain(origin: f32, origin_y: i32, y: i32, inner: Rect) -> f32 {
    let travel = (inner.h - VOL_HANDLE_H) as f32;
    position_to_gain((origin + (origin_y - y) as f32 / travel).clamp(0.0, 1.0))
}

pub(super) fn handle_y(inner: Rect, gain: f32) -> i32 {
    inner.y + ((inner.h - VOL_HANDLE_H) as f32 * (1.0 - gain_to_position(gain))).round() as i32
}

pub(super) fn level_h(inner: Rect, level: f32) -> i32 {
    ((inner.h - VOL_HANDLE_H) as f32 * gain_to_position(level).min(1.0)).round() as i32 + VOL_HANDLE_H
}

pub(super) fn tile_button(tile: Rect, slot: usize) -> Rect {
    let inner = tile_inner(tile);
    let cells = TRACK_BUTTONS.len() as i32;
    let total = inner.w + (cells + 1) * CONTROL_BORDER;
    let width = |cell: i32| total / cells + i32::from(cell < total % cells);
    let x = inner.x - CONTROL_BORDER + (0..slot as i32).map(|cell| width(cell) - CONTROL_BORDER).sum::<i32>();
    Rect::new(x, inner.y + inner.h + CONTROL_BORDER - CELL_H, width(slot as i32), CELL_H)
}

pub(super) fn tile_clear(tile: Rect) -> Rect {
    let inner = tile_inner(Rect::new(tile.x, tile.y, column_w(), tile.h));
    Rect::new(inner.x + inner.w + CONTROL_BORDER - CELL_H, inner.y - CONTROL_BORDER, CELL_H, CELL_H)
}

pub(super) fn tile_send(tile: Rect, slot: usize) -> Rect {
    let button = tile_button(tile, slot);
    Rect::new(button.x, button.y - CELL_H + CONTROL_BORDER, button.w, CELL_H)
}

pub(super) fn append_button(tile: Rect, slot: usize) -> Rect {
    let inner = tile_inner(tile);
    Rect::new(inner.x - CONTROL_BORDER, inner.y - CONTROL_BORDER + slot as i32 * (CELL_H - CONTROL_BORDER), inner.w + 2 * CONTROL_BORDER, CELL_H)
}

pub(super) fn file_row(action: FileAction) -> usize {
    FILE_ITEMS.iter().position(|(item, _)| *item == action).unwrap_or(0)
}

impl App {
    fn folded(&self) -> bool {
        let wide: i32 = HEADER.iter().filter(|head| (head.shown)(self)).map(|head| header_inner_w(head.wide) + BORDER).sum();
        self.width < FOLD_W || TILE + VOL_W + wide + BORDER > self.width
    }

    pub(super) fn worded(&self) -> bool {
        !self.folded()
    }

    pub(super) fn menu_open(&self, menu: Menu) -> bool {
        self.menu.map(Menu::root) == Some(menu)
    }

    pub(super) fn header_cell(&self, index: usize) -> Rect {
        let folded = self.folded();
        let x = TILE + VOL_W + (0..index).filter(|&at| (HEADER[at].shown)(self)).map(|at| header_inner_w(header_label(at, folded)) + BORDER).sum::<i32>();
        Rect::new(x, 0, header_inner_w(header_label(index, folded)) + 2 * BORDER, RAIL_H + BORDER)
    }

    pub(super) fn last_header_cell(&self) -> Rect {
        HEADER.iter().rev().find(|head| (head.shown)(self)).map_or_else(|| self.cell_of(Hover::Button(Button::Big)), |head| self.cell_of(head.hover))
    }

    pub(super) fn cell_of(&self, hover: Hover) -> Rect {
        self.header_cell(HEADER.iter().position(|head| head.hover == hover).unwrap_or(0))
    }

    pub(super) fn feedback_cell(&self) -> Rect {
        let w = feedback_w(self.folded());
        Rect::new(self.width - w, 0, w, RAIL_H + BORDER)
    }

    pub(super) fn feedback_shown(&self) -> bool {
        let last = self.last_header_cell();
        last.x + last.w <= self.feedback_cell().x + BORDER
    }

    pub(super) fn rev_cell(&self) -> Rect {
        let (feedback, w) = (self.feedback_cell(), rev_w(self.folded()));
        let right = if self.feedback_shown() { feedback.x + BORDER } else { self.width };
        Rect::new(right - w, 0, w, feedback.h)
    }

    pub(super) fn menu_labels(&self, menu: Menu) -> Vec<String> {
        match menu {
            Menu::File => FILE_ITEMS.iter().map(|(action, label)| if *action == FileAction::ExportAudio { self.export_label(label) } else { String::from(*label) }).collect(),
            Menu::Export => EXPORT_ITEMS.iter().map(|(_, label)| String::from(*label)).collect(),
            Menu::Recent => self.disk.recent.iter().map(|path| font::shortened(&stem_of(path), RECENT_CHARS)).collect(),
            Menu::Snap => grid::SNAPS.iter().map(|snap| String::from(snap.name())).collect(),
            Menu::Key => Root::ALL.iter().map(|root| flat_spelled(root.name())).collect(),
            Menu::Scale => ScaleName::LABELS.iter().map(|label| String::from(*label)).collect(),
            Menu::Fit => self.fits.iter().map(|fit| fit.label()).collect(),
            Menu::Add(track) => ADD_ITEMS.iter().map(|(_, label)| String::from(*label)).chain((!self.project.follow_sources(track).is_empty()).then(|| String::from(FOLLOW))).chain(std::iter::once(String::from(DELAY))).collect(),
            Menu::Follow(track) => self.project.follow_sources(track).iter().map(|&source| font::shortened(&self.project.tracks[source].name, RECENT_CHARS)).collect(),
        }
    }

    pub(super) fn menu_rect(&self, menu: Menu) -> Rect {
        let labels = self.menu_labels(menu);
        let widest = labels.iter().map(|label| font::width(label, TEXT_SCALE)).max().unwrap_or(0) + 2 * TEXT_PAD;
        let h = 2 * CONTROL_BORDER + labels.len() as i32 * ITEM_H;
        if let Some(opener) = menu.opener() {
            let row = self.menu_item(Menu::File, file_row(opener));
            return Rect::new(row.x + row.w, row.y - CONTROL_BORDER, widest.max(MENU_W), h);
        }
        if let Menu::Follow(track) = menu {
            let row = self.menu_item(Menu::Add(track), ADD_ITEMS.len());
            return Rect::new(row.x + row.w, (row.y - CONTROL_BORDER).min(self.height - h), widest, h);
        }
        if let Menu::Add(track) = menu {
            let button = tile_button(self.track_tile(self.track_slot(track)), TRACK_BUTTONS.len() - 1);
            return Rect::new(button.x + button.w - CONTROL_BORDER, button.y.min(self.height - h), widest, h);
        }
        if menu == Menu::Fit {
            let (roll, cell) = (self.roll_rect(), self.roll.fit_cell());
            return Rect::new(roll.x + cell.x, roll.y + cell.y + cell.h - face::FRAME, widest.max(cell.w), h);
        }
        let (cell, min_w) = if menu == Menu::File { (file_cell(), MENU_W) } else { (self.cell_of(Hover::Button(Button::Menu(menu))), 0) };
        let w = (cell.w - 2 * BORDER + 2 * CONTROL_BORDER).max(widest).max(min_w);
        Rect::new(cell.x + BORDER - CONTROL_BORDER, RAIL_H + BORDER - CONTROL_BORDER, w, h)
    }

    pub(super) fn menu_item(&self, menu: Menu, index: usize) -> Rect {
        let rect = self.menu_rect(menu);
        Rect::new(rect.x + CONTROL_BORDER, rect.y + CONTROL_BORDER + index as i32 * ITEM_H, rect.w - 2 * CONTROL_BORDER, ITEM_H)
    }

    pub(super) fn delay_bar(&self, menu: Menu, index: usize) -> Option<usize> {
        let Menu::Add(track) = menu else {
            return None;
        };
        (index == ADD_ITEMS.len() + usize::from(!self.project.follow_sources(track).is_empty())).then_some(track)
    }

    pub(super) fn palette_rect(&self, deck: usize) -> Rect {
        let stage = tile_inner(self.stage_rect());
        if !self.view.palette_big {
            return stage;
        }
        let h = (stage.h - BORDER) / 2;
        Rect::new(stage.x, if deck == 0 { stage.y + stage.h - h } else { stage.y }, stage.w, h)
    }

    pub(super) fn shown_decks(&self) -> std::ops::Range<usize> {
        let shown = if self.view.palette_big { DECKS } else { 1 };
        0..shown
    }

    pub(super) fn palette_at(&self, x: i32, y: i32) -> Option<usize> {
        self.shown_decks().find(|&deck| self.palette_rect(deck).contains(x, y))
    }

    pub(super) fn active_palette(&self) -> Option<usize> {
        self.palettes.iter().position(Palette::active)
    }

    pub(super) fn core_cell(&self) -> Rect {
        let rack = self.rack_cell();
        Rect::new(rack.x + rack.w - BORDER, rack.y, CORE_W + BORDER, STAGE_H + BORDER)
    }

    pub(super) fn master_cell(&self) -> Rect {
        Rect::new(self.width - MASTER_W, self.seam_bottom(), MASTER_W, STAGE_H + BORDER)
    }

    pub(super) fn aside_cell(&self) -> Rect {
        let (squares, master) = (self.squares_cell(), self.master_cell());
        let x = squares.x + squares.w - BORDER;
        Rect::new(x, squares.y, master.x + BORDER - x, squares.h)
    }

    pub(super) fn hertz_cell(&self) -> Rect {
        let cell = self.aside_cell();
        Rect::new(cell.x + cell.w - MASTER_W, cell.y, MASTER_W, CELL_H + 2 * BORDER)
    }

    pub(super) fn master_name_cell(&self) -> Rect {
        let cell = self.master_cell();
        Rect::new(cell.x, cell.y, cell.w, CELL_H + 2 * BORDER)
    }

    pub(super) fn cycle_cell(&self) -> Rect {
        let cell = self.master_cell();
        Rect::new(cell.x, cell.y + cell.h - (CELL_H + 2 * BORDER), cell.w, CELL_H + 2 * BORDER)
    }

    pub(super) fn send_cell(&self, slot: usize) -> Rect {
        let cycle = self.cycle_cell();
        let pitch = CELL_H + BORDER;
        Rect::new(cycle.x + slot as i32 * pitch, cycle.y - pitch, pitch + BORDER, pitch + BORDER)
    }

    pub(super) fn master_volume(&self) -> Rect {
        let name = self.master_name_cell();
        let top = name.y + name.h - BORDER;
        Rect::new(name.x, top, name.w, self.send_cell(0).y + BORDER - top)
    }

    pub(super) fn face_rect(&self) -> Rect {
        let inner = tile_inner(self.core_cell());
        Rect::new(inner.x - face::FRAME, inner.y - face::FRAME, face::W, face::H)
    }

    pub(super) fn face_point(&self, x: i32, y: i32) -> (i32, i32) {
        let rect = self.face_rect();
        (x - rect.x, y - rect.y)
    }

    pub(super) fn squares_cell(&self) -> Rect {
        let core = self.core_cell();
        Rect::new(core.x + core.w - BORDER, core.y, SQUARES_W, core.h)
    }

    pub(super) fn square(&self, slot: i32) -> Rect {
        let inner = tile_inner(self.squares_cell());
        Rect::new(inner.x - face::FRAME, inner.y - face::FRAME + slot * face::PITCH, face::CELL, face::CELL)
    }

    pub(super) fn browser_square(&self) -> Rect {
        self.square(0)
    }

    pub(super) fn stock_square(&self) -> Rect {
        self.square(1)
    }

    fn below_stock(&self, slot: i32) -> i32 {
        slot + i32::from(self.stock_offered())
    }

    pub(super) fn save_cell(&self) -> Rect {
        self.square(self.below_stock(1))
    }

    pub(super) fn load_cell(&self) -> Rect {
        self.square(self.below_stock(2))
    }

    pub(super) fn spectrum_square(&self) -> Rect {
        self.square(self.below_stock(3))
    }

    pub(super) fn own_square(&self) -> Rect {
        self.square(self.below_stock(4))
    }

    pub(super) fn wave_square(&self) -> Rect {
        self.square(self.below_stock(if self.own_button().is_some() { 5 } else { 4 }))
    }

    pub(super) fn over_big(&self, x: i32, y: i32) -> bool {
        self.picture_big() && tile_inner(self.aside_cell()).contains(x, y)
    }

    pub(super) fn picture_inner(&self) -> Rect {
        if !self.picture_big() {
            return face::interior(face::readout_rect(self.readout_rows()));
        }
        let (inner, face) = (tile_inner(self.aside_cell()), self.face_rect());
        Rect::new(inner.x - face.x, inner.y - face.y, inner.w, inner.h)
    }

    pub(super) fn wave_body(&self) -> Rect {
        let inner = tile_inner(self.aside_cell());
        Rect::new(inner.x, inner.y, inner.w, self.wave_lane().y - inner.y)
    }

    pub(super) fn wave_lane(&self) -> Rect {
        let row = self.wave_row();
        Rect::new(row.x, row.y - WAVE_LANE_ROWS * face::PITCH, row.w, WAVE_LANE_ROWS * face::PITCH + face::FRAME)
    }

    pub(super) fn wave_zoom_cell(&self) -> Rect {
        let band = self.wave_band();
        Rect::new(band.x + FILE_BAND.len() as i32 * face::PITCH, band.y, WAVE_ZOOM_SPAN * face::PITCH + face::FRAME, face::CELL)
    }

    pub(super) fn wave_loop_cell(&self) -> Rect {
        let zoom = self.wave_zoom_cell();
        Rect::new(zoom.x + zoom.w - face::FRAME, zoom.y, face::CELL, face::CELL)
    }

    pub(super) fn wave_reverse_cell(&self) -> Rect {
        let looped = self.wave_loop_cell();
        Rect::new(looped.x + looped.w - face::FRAME, looped.y, face::CELL, face::CELL)
    }

    pub(super) fn wave_save_cell(&self) -> Rect {
        let reversed = self.wave_reverse_cell();
        Rect::new(reversed.x + reversed.w - face::FRAME, reversed.y, face::CELL, face::CELL)
    }

    pub(super) fn wave_all_cell(&self) -> Rect {
        let saved = self.wave_save_cell();
        Rect::new(saved.x + saved.w - face::FRAME, saved.y, face::CELL, face::CELL)
    }

    pub(super) fn wave_row(&self) -> Rect {
        let band = self.wave_band();
        Rect::new(band.x, band.y - face::PITCH, band.w, face::CELL)
    }

    pub(super) fn wave_band(&self) -> Rect {
        let inner = tile_inner(self.aside_cell());
        Rect::new(inner.x - face::FRAME, inner.y + inner.h + face::FRAME - face::CELL, inner.w + 2 * face::FRAME, face::CELL)
    }

    pub(super) fn wave_point(&self, x: i32, y: i32) -> (i32, i32) {
        let band = self.wave_band();
        (x - band.x, y - band.y)
    }

    pub(super) fn browser_cell(&self) -> Rect {
        if self.browser_big() { Rect::new(0, RAIL_H, self.width, self.height - RAIL_H) } else { self.aside_cell() }
    }

    pub(super) fn browser_rect(&self) -> Rect {
        let inner = tile_inner(self.browser_cell());
        Rect::new(inner.x - face::FRAME, inner.y - face::FRAME, inner.w + 2 * face::FRAME, inner.h + 2 * face::FRAME)
    }

    pub(super) fn lay_browser(&mut self) -> Rect {
        let rect = self.browser_rect();
        self.browser.layout(rect.w, rect.h);
        rect
    }

    pub(super) fn browser_point(&self, x: i32, y: i32) -> (i32, i32) {
        let rect = self.browser_rect();
        (x - rect.x, y - rect.y)
    }

    pub(super) fn lanes_rect(&self) -> Rect {
        let inner = tile_inner(self.aside_cell());
        Rect::new(inner.x - face::FRAME, inner.y - face::FRAME, inner.w + 2 * face::FRAME, lanes::H)
    }

    pub(super) fn lanes_point(&self, x: i32, y: i32) -> (i32, i32) {
        let rect = self.lanes_rect();
        (x - rect.x, y - rect.y)
    }

    pub(super) fn ask_rect(&self) -> Rect {
        let h = ASK_H + RAIL_H;
        Rect::new((self.width - ASK_W) / 2, (self.height - h) / 2, ASK_W, h)
    }

    pub(super) fn ask_message(&self) -> Rect {
        let rect = self.ask_rect();
        Rect::new(rect.x, rect.y, rect.w, ASK_H)
    }

    pub(super) fn ask_button(&self, index: usize) -> Rect {
        let rect = self.ask_rect();
        let first = (ASK_W + BORDER) / 2;
        let (x, w) = if index == 0 { (rect.x, first) } else { (rect.x + first - BORDER, ASK_W + BORDER - first) };
        Rect::new(x, rect.y + ASK_H - BORDER, w, RAIL_H + BORDER)
    }

    pub(super) fn export_rect(&self) -> Rect {
        let h = EXPORT_H + RAIL_H;
        Rect::new((self.width - ASK_W) / 2, (self.height - h) / 2, ASK_W, h)
    }

    pub(super) fn export_message(&self) -> Rect {
        let rect = self.export_rect();
        Rect::new(rect.x, rect.y, rect.w, EXPORT_H)
    }

    pub(super) fn export_bar(&self) -> Rect {
        let inner = tile_inner(self.export_message());
        let y = inner.y + TEXT_PAD + font::GLYPH_H * (TEXT_SCALE + ASK_NAME_SCALE) + 2 * ASK_LEADING;
        Rect::new(inner.x + TEXT_PAD, y, inner.w - 2 * TEXT_PAD, EXPORT_BAR_H)
    }

    pub(super) fn export_button(&self) -> Rect {
        let rect = self.export_rect();
        Rect::new(rect.x, rect.y + EXPORT_H - BORDER, rect.w, RAIL_H + BORDER)
    }

    pub(super) fn save_panel(&self, labels: &[&str]) -> Rect {
        face::panel_rect(self.save_cell(), labels, 0, self.stage_rect())
    }

    pub(super) fn stock_panel(&self, labels: &[&str]) -> Rect {
        face::panel_rect(self.stock_square(), labels, 0, self.stage_rect())
    }

    pub(super) fn rack_cell(&self) -> Rect {
        Rect::new(0, self.seam_bottom(), column_w(), STAGE_H + BORDER)
    }

    pub(super) fn rack_rect(&self) -> Rect {
        let inner = tile_inner(self.rack_cell());
        Rect::new(inner.x - face::FRAME, inner.y - face::FRAME, rack_w(), rack::H)
    }

    pub(super) fn rack_point(&self, x: i32, y: i32) -> (i32, i32) {
        let rect = self.rack_rect();
        (x - rect.x, y - rect.y)
    }

    pub(super) fn palette_point(&self, deck: usize, x: i32, y: i32) -> (i32, i32) {
        let rect = self.palette_rect(deck);
        (((x - rect.x) as f32 / PALETTE_SCALE).floor() as i32, ((y - rect.y) as f32 / PALETTE_SCALE).floor() as i32)
    }

    pub(super) fn edited_clip(&self) -> Option<(usize, usize)> {
        let &(track, start) = self.selection.clips.last()?;
        let owner = self.project.tracks.get(track)?;
        let index = owner.clips.iter().position(|clip| clip.start == start)?;
        (!owner.audio).then_some((track, index))
    }

    pub(super) fn roll_shown(&self) -> bool {
        !self.view.big && !self.view.palette_shown && self.edited_clip().is_some()
    }

    pub(super) fn roll_big(&self) -> bool {
        self.roll_shown() && self.roll.big()
    }

    pub(super) fn stage_rect(&self) -> Rect {
        if self.roll_big() { Rect::new(0, RAIL_H, self.width, self.arrangement_bottom() + BORDER - RAIL_H) } else { Rect::new(0, self.seam_bottom(), self.width, self.stage_h() + BORDER) }
    }

    pub(super) fn roll_rect(&self) -> Rect {
        let inner = tile_inner(self.stage_rect());
        Rect::new(inner.x - face::FRAME, inner.y - face::FRAME, inner.w + 2 * face::FRAME, inner.h + 2 * face::FRAME)
    }

    pub(super) fn roll_point(&self, x: i32, y: i32) -> (i32, i32) {
        let rect = self.roll_rect();
        (x - rect.x, y - rect.y)
    }

    pub(super) fn stage_h(&self) -> i32 {
        if self.view.palette_shown && self.view.palette_big { (2 * STAGE_H).min(self.height - RAIL_H - ruler::H - 3 * BORDER).max(STAGE_H) } else { STAGE_H }
    }

    pub(super) fn seam_bottom(&self) -> i32 {
        if self.view.big { self.height } else { self.height - self.stage_h() - BORDER }
    }

    pub(super) fn arrangement_bottom(&self) -> i32 {
        if self.view.big || self.roll_big() { self.height - ruler::H - 2 * BORDER } else { self.seam_bottom() - ruler::H - BORDER }
    }

    pub(super) fn ruler_cell(&self) -> Rect {
        Rect::new(0, self.arrangement_bottom(), self.width, ruler::H + 2 * BORDER)
    }

    pub(super) fn ruler_readout(&self) -> Rect {
        Rect::new(BORDER, self.arrangement_bottom() + BORDER, column_w() - 2 * BORDER, ruler::H)
    }

    pub(super) fn ruler_strip(&self) -> Rect {
        Rect::new(column_w(), self.arrangement_bottom() + BORDER, self.width - column_w() - BORDER, ruler::H)
    }

    pub(super) fn ruler_lane(&self) -> Lane {
        Lane { rect: self.ruler_strip(), scroll: self.timeline.grid_scroll, beat_w: self.timeline.beat_w, snap: self.timeline.grid_step() }
    }

    pub(super) fn ruler_zone(&self) -> Rect {
        let cell = self.ruler_cell();
        Rect::new(cell.x, cell.y - ruler::H, cell.w, cell.h + 2 * ruler::H)
    }

    pub(super) fn track_viewport(&self) -> Rect {
        Rect::new(BORDER, RAIL_H + BORDER, self.width - 2 * BORDER, self.arrangement_bottom() - RAIL_H - BORDER)
    }

    pub(super) fn tiles(&self) -> Vec<(Strip, Rect)> {
        let mut top = RAIL_H - self.timeline.track_scroll;
        self.strips()
            .into_iter()
            .map(|strip| {
                let tile = Rect::new(0, top, TILE, strip.height());
                top += strip.height() - BORDER;
                (strip, tile)
            })
            .collect()
    }

    pub(super) fn track_tile(&self, slot: usize) -> Rect {
        let strips = self.strips();
        let top = RAIL_H - self.timeline.track_scroll + strips.iter().take(slot).map(|strip| strip.height() - BORDER).sum::<i32>();
        Rect::new(0, top, TILE, strips.get(slot).map_or(TILE, |strip| strip.height()))
    }

    pub(super) fn tile_shown(&self, tile: Rect) -> bool {
        let viewport = self.track_viewport();
        tile.y < viewport.y + viewport.h && tile.y + tile.h > viewport.y
    }

    pub(super) fn append_tile(&self) -> Option<Rect> {
        if self.view.auto != Auto::Off {
            return None;
        }
        Some(Rect::new(0, RAIL_H + self.strips().len() as i32 * (TILE - BORDER) - self.timeline.track_scroll, TILE, APPEND_H))
    }

    pub(super) fn max_track_scroll(&self) -> i32 {
        let last = self.append_tile().unwrap_or_else(|| self.track_tile(self.strips().len().saturating_sub(1)));
        let column = last.y + last.h + self.timeline.track_scroll - RAIL_H;
        let region = self.track_viewport().h + 2 * BORDER;
        (column - region).max(0)
    }

    pub(super) fn carried(&self, drag: TrackDrag) -> Vec<(usize, Rect)> {
        let shown: Vec<usize> = self.dragged_tracks(drag).into_iter().filter(|&track| !self.hidden(track)).collect();
        let lead = shown.iter().position(|&track| track == drag.track).unwrap_or(0) as i32;
        shown.into_iter().enumerate().map(|(at, track)| (track, Rect::new(0, drag.y - drag.grab + (at as i32 - lead) * (TILE - BORDER), TILE, TILE))).collect()
    }

    pub(super) fn drop_spot(&self, drag: TrackDrag) -> DropSpot {
        let moving = self.dragged_tracks(drag);
        let others: Vec<usize> = self.visible_tracks().into_iter().filter(|track| !moving.contains(track)).collect();
        let pitch = TILE - BORDER;
        let top = self.carried(drag).first().map_or(drag.y - drag.grab, |(_, tile)| tile.y);
        let reach = top + self.timeline.track_scroll - RAIL_H;
        let slot = (reach + pitch / 2).div_euclid(pitch).clamp(0, others.len() as i32);
        let before = others.get(slot as usize).copied();
        let tracks = &self.project.tracks;
        let end = before.unwrap_or(tracks.len());
        let over = (0..end).rev().find(|track| !moving.contains(track));
        let in_group = over.is_some_and(|over| tracks[over].group || tracks[over].inside);
        let among_members = before.is_some_and(|before| tracks[before].inside);
        let group_leads = moving.first().is_some_and(|&lead| tracks[lead].group);
        let upper_half = reach < slot * pitch;
        let in_place = moving.iter().copied().eq(end.saturating_sub(moving.len())..end);
        let kept = in_place && (drag.y - drag.origin_y).abs() < SLIP_PX;
        let inside = if kept { moving.first().is_some_and(|&lead| tracks[lead].inside) } else { in_group && (among_members || (!group_leads && upper_half)) };
        DropSpot { before, slot: slot as usize, inside, kept }
    }

    pub(super) fn lane_of(&self, track: usize) -> usize {
        self.project.group_of(track).filter(|&group| self.project.tracks[group].folded).unwrap_or(track)
    }

    pub(super) fn hidden(&self, track: usize) -> bool {
        self.lane_of(track) != track
    }

    pub(super) fn visible_tracks(&self) -> Vec<usize> {
        (0..self.project.tracks.len()).filter(|&track| !self.hidden(track)).collect()
    }

    pub(super) fn display_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.project.tracks.len()).collect();
        if let Some(drag) = self.track_drag().filter(|drag| drag.armed) {
            let moving = self.dragged_tracks(drag);
            order.retain(|track| !moving.contains(track));
            let at = self.drop_spot(drag).before.and_then(|before| order.iter().position(|&track| track == before)).unwrap_or(order.len());
            order.splice(at..at, moving);
        }
        order
    }

    pub(super) fn group_label(&self) -> Option<&'static str> {
        let picked = self.selection.picked_tracks();
        if picked.iter().any(|&track| self.project.tracks[track].group) {
            return (picked.len() == 1).then_some("UNGROUP");
        }
        if self.all_grouped(&picked) {
            return Some("UNGROUP");
        }
        (picked.len() >= 2).then_some("GROUP")
    }

    pub(super) fn strips(&self) -> Vec<Strip> {
        match self.view.auto {
            Auto::Off => self.display_order().into_iter().filter(|&track| !self.hidden(track)).map(Strip::Track).collect(),
            Auto::Track => std::iter::once(Strip::Track(self.selection.track)).chain((0..self.automations().len()).map(Strip::Curve)).collect(),
            Auto::All => {
                let lanes = self.automations();
                let mut strips = Vec::new();
                let mut lane = 0;
                for track in self.visible_tracks() {
                    let host = self.lane_host(track);
                    let count = lanes[lane..].iter().take_while(|(owner, _)| *owner == host).count();
                    strips.push(Strip::Track(track));
                    strips.extend((lane..lane + count).map(Strip::Curve));
                    lane += count;
                }
                strips
            }
        }
    }

    pub(super) fn lane_host(&self, track: usize) -> Host {
        if track == self.selection.track { self.host() } else { Host::Track(track) }
    }

    pub(super) fn track_slots(&self) -> Vec<(usize, usize)> {
        let strips = self.strips();
        strips
            .into_iter()
            .enumerate()
            .filter_map(|(slot, strip)| match strip {
                Strip::Track(track) => Some((slot, track)),
                Strip::Curve(_) => None,
            })
            .collect()
    }

    pub(super) fn track_slot(&self, track: usize) -> usize {
        self.track_slots().into_iter().find(|&(_, shown)| shown == track).map_or(0, |(slot, _)| slot)
    }

    pub(super) fn lane(&self, tile: Rect) -> Lane {
        Lane { rect: lane_rect(tile, self.width), scroll: self.timeline.grid_scroll, beat_w: self.timeline.beat_w, snap: self.timeline.grid_step() }
    }

    pub(super) fn throttle_pan(&mut self, seconds: f64) -> bool {
        let Some(Drag::Zoom(mut drag)) = self.drag else {
            return false;
        };
        let dx = drag.at_x - drag.origin.0;
        if dx.abs() < DRAG_ARM_PX {
            return false;
        }
        let nearest = f64::from(self.lanes_w()) / 2.0 / self.timeline.beat_w;
        drag.centre = (drag.centre + f64::from(dx * dx.abs()) / grid::PAN_DAMPING * seconds / self.timeline.beat_w).max(nearest);
        self.drag = Some(Drag::Zoom(drag));
        self.set_view(self.timeline.beat_w, drag.centre)
    }

    pub(super) fn lanes_w(&self) -> i32 {
        self.width - column_w()
    }

    pub(super) fn centre_beat(&self) -> f64 {
        (f64::from(self.timeline.grid_scroll) + f64::from(self.lanes_w()) / 2.0) / self.timeline.beat_w
    }

    pub(super) fn set_view_under(&mut self, beat_w: f64, beat: f64, x: i32) -> bool {
        let centre = beat + (f64::from(self.lanes_w()) / 2.0 - f64::from(x - column_w())) / beat_w;
        self.set_view(beat_w, centre)
    }

    pub(super) fn set_view(&mut self, beat_w: f64, centre: f64) -> bool {
        let scroll = ((centre * beat_w - f64::from(self.lanes_w()) / 2.0).round() as i32).max(0);
        let changed = beat_w != self.timeline.beat_w || scroll != self.timeline.grid_scroll;
        self.timeline.beat_w = beat_w;
        self.timeline.grid_scroll = scroll;
        changed
    }

    pub(super) fn bars_label(&self) -> String {
        counted((f64::from(self.lanes_w()) / (self.timeline.beat_w * f64::from(BEATS_PER_BAR))).round().max(1.0) as i32, "BAR")
    }

    pub(super) fn lanes_region(&self) -> Rect {
        Rect::new(column_w(), RAIL_H + BORDER, self.width - column_w(), self.arrangement_bottom() - RAIL_H - BORDER)
    }
}

pub(super) fn clock(seconds: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let millis = (seconds.abs() * 1000.0).round() as i64;
    let (minutes, rest) = (millis / 60_000, millis % 60_000);
    format!("{sign}{minutes}:{:02}.{:03}", rest / 1000, rest % 1000)
}
