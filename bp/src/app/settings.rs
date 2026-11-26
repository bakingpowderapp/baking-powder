use super::*;
use super::sorry::{MIN_H, MIN_W};

const CONTRAST_MIN: i32 = 50;
const CONTRAST_MAX: i32 = 100;
const ZOOM_MIN: i32 = 50;
const ZOOM_MAX: i32 = 150;
const ZOOM_STEP: i32 = 5;
const ZOOM_DEFAULT: i32 = 100;
const PREVIEW_MAX: i32 = 100;
const BUFFER_LADDER: [u32; 6] = [256, 512, 1024, 2048, 4096, 8192];
const BUFFER_DEFAULT: u32 = 256;
const CONTRAST_PX_PER_STEP: i32 = 2;
const ZOOM_PX_PER_STEP: i32 = 6;
const PREVIEW_PX_PER_STEP: i32 = 2;
const BUFFER_PX_PER_STEP: i32 = 20;
const FOLLOWS: &str = "BPM FOLLOWS PITCH";
const IGNORES: &str = "BPM IGNORES PITCH";
const HIDE_FACTORY: &str = "HIDE FACTORY";
const SHOW_FACTORY: &str = "SHOW FACTORY";
const HIDE_GHOSTS: &str = "HIDE GHOST NOTES";
const SHOW_GHOSTS: &str = "SHOW GHOST NOTES";
const HIDE_NUMBERS: &str = "HIDE BAR NUMBERS";
const SHOW_NUMBERS: &str = "SHOW BAR NUMBERS";
const FOLLOW_ON: &str = "FOLLOW ON";
const FOLLOW_OFF: &str = "FOLLOW OFF";
const MCP_ON: &str = "MCP ON";
const MCP_OFF: &str = "MCP OFF";
const PORT_TAKEN: &str = "PORT TAKEN";
pub(super) const PORT: &str = "PORT";
const PORT_DIGITS: usize = 5;
const COPY_CONFIG: &str = "COPY CONFIG";
const FEEDBACK_ON: &str = "FEEDBACK ON";
const FEEDBACK_OFF: &str = "FEEDBACK OFF";
pub(super) const COPIED: &str = "COPIED!";
const COPIED_SECONDS: f32 = 1.5;
const AGENT: &str = "AGENT";
pub(super) const NONE: &str = "NONE";
const HELP_LINE: i32 = font::GLYPH_H * TEXT_SCALE + 6;
const HINT_SCALE: i32 = 2 * face::LABEL_SCALE;
const HINT_LINE: i32 = font::GLYPH_H * HINT_SCALE + 6;
pub(super) const EDITS: usize = history::STEPS;

pub(super) fn age(seconds: u64) -> String {
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    if seconds < 60 {
        String::from("NOW")
    } else if minutes < 60 {
        format!("{minutes} MIN")
    } else if hours < 24 {
        format!("{hours} HR")
    } else {
        counted(days as i32, "DAY")
    }
}

fn latency_words(timing: &crate::project::Timing) -> String {
    if timing.output == 0 {
        return String::from("No row delays the sound");
    }
    let samples = if timing.output == 1 { "sample" } else { "samples" };
    format!("The sound leaves {} {samples} after the playhead, {:.1} ms.", timing.output, timing.ms(timing.output))
}

pub(super) fn story(at: u64, tracks: usize, length: i32) -> String {
    format!("{} \u{b7} {} \u{b7} {}", age(history::now().saturating_sub(at)), counted(tracks as i32, "TRACK"), counted(bars(length), "BAR"))
}

pub(super) fn edit_story(step: &Step) -> String {
    story(step.at, step.project.tracks.len(), step.project.end())
}

remembered!(Settings {
    flag dark: bool = false, "dark";
    flag bpm_follows_pitch: bool = false, "bpmFollowsPitch";
    flag factory: bool = true, "factory";
    flag ghost_notes: bool = true, "ghostNotes";
    flag bar_numbers: bool = true, "barNumbers";
    flag follow: bool = true, "follow";
    flag mcp: bool = false, "mcp";
    whole port: u16 = mcp::DEFAULT_PORT, "mcpPort", mcp::LEAST_PORT..=u16::MAX;
    flag feedback: bool = false, "mcpFeedback";
    whole contrast: i32 = CONTRAST_MAX, "contrast", CONTRAST_MIN..=CONTRAST_MAX;
    whole zoom: i32 = ZOOM_DEFAULT, "zoom", ZOOM_MIN..=ZOOM_MAX, |percent| percent / ZOOM_STEP * ZOOM_STEP;
    whole preview: i32 = PREVIEW_MAX, "preview", 0..=PREVIEW_MAX;
    whole buffer: u32 = BUFFER_DEFAULT, "buffer", 0..=u32::MAX, |frames| BUFFER_LADDER[buffer_rung(frames)];
    state port_typing: Option<String> = None;
    state zoom_room: (i32, i32) = (ZOOM_DEFAULT, ZOOM_DEFAULT);
    state copied: f32 = 0.0;
    state versions_scroll: i32 = 0;
    state edits_scroll: i32 = 0;
});

impl Settings {
    pub(super) fn theme(&self) -> Theme {
        Theme { dark: self.dark, contrast: self.contrast }
    }
}

struct Column {
    left: i32,
    width: i32,
    count: usize,
    offset: usize,
    fit: usize,
}

impl Column {
    fn rows(&self) -> impl Iterator<Item = (usize, Rect)> + '_ {
        (0..self.fit.min(self.count - self.offset)).map(|row| (self.offset + row, column_box(self.left, self.width, row)))
    }

    fn rect(&self, height: i32) -> Rect {
        Rect::new(self.left, row_box(0).y, self.width, height - BORDER - row_box(0).y)
    }

    fn most(&self) -> i32 {
        self.count.saturating_sub(self.fit) as i32 * (CONTROL_H + HEADER_PAD)
    }

    fn label_x(&self) -> i32 {
        self.left + CONTROL_BORDER + TEXT_PAD
    }
}

#[derive(Clone, Copy)]
pub(super) struct StepDrag {
    origin_y: i32,
    origin: i32,
}

#[derive(Clone, Copy)]
pub(super) struct BufferDrag {
    origin_y: i32,
    origin: u32,
}

enum Look {
    Button(String, bool),
    Meter(String, f32, bool),
    Port(String, bool),
    Readout(String),
    Gauge(String, f32),
}

struct Setting {
    band: &'static str,
    name: &'static str,
    widths: &'static [&'static str],
    look: fn(&App) -> Look,
    press: fn(&mut App, i32, bool) -> bool,
    help: fn(&App) -> (String, String),
}

fn flip(flag: &mut bool) -> bool {
    *flag ^= true;
    true
}

fn words(on: bool, yes: &str, no: &str) -> String {
    String::from(if on { yes } else { no })
}

fn shown(on: bool) -> String {
    words(on, "SHOWN", "HIDDEN")
}

fn load(app: &App) -> u32 {
    app.audio.as_ref().map_or(0, Audio::load)
}

fn dropouts(app: &App) -> String {
    app.audio.as_ref().map_or(0, Audio::dropouts).to_string()
}

fn served(app: &mut App, flag: fn(&mut Settings) -> &mut bool) -> bool {
    flip(flag(&mut app.settings));
    app.settle_server();
    true
}

const SETTINGS: [Setting; 17] = [
    Setting { band: "SCREEN", name: "CONTRAST", widths: &[], look: |app| Look::Meter(format!("{}%", app.settings.contrast), app.settings.contrast as f32 / 100.0, matches!(app.drag, Some(Drag::Contrast(_)))), press: |app, y, reset| {
        if reset { app.settings.contrast = CONTRAST_MAX } else { app.drag = Some(Drag::Contrast(StepDrag { origin_y: y, origin: app.settings.contrast })) }
        true
    }, help: |app| (format!("{}%", app.settings.contrast), String::from("Ink contrast, 50-100%. Drag vertically; double-press resets to 100%.")) },
    Setting { band: "SCREEN", name: "ZOOM", widths: &[], look: |app| Look::Meter(format!("{}%", app.zoomed()), (app.zoomed() - ZOOM_MIN) as f32 / (ZOOM_MAX - ZOOM_MIN) as f32, matches!(app.drag, Some(Drag::ScreenZoom(_)))), press: |app, y, reset| {
        if reset { app.reset_zoom(); } else { app.drag = Some(Drag::ScreenZoom(StepDrag { origin_y: app.on_glass(y), origin: app.zoomed() })) }
        true
    }, help: |app| (format!("{}%", app.zoomed()), format!("Size of everything on screen, from {}% to {}% in this window; smaller shows more. Drag vertically; double-press or Command 0 resets to 100%. Command minus and plus step it.", app.settings.zoom_room.0, app.settings.zoom_room.1)) },
    Setting { band: "SCREEN", name: "THEME", widths: &[], look: |app| Look::Button(words(app.settings.dark, "DARK", "LIGHT"), false), press: |app, _, _| flip(&mut app.settings.dark), help: |app| (words(app.settings.dark, "DARK", "LIGHT"), String::from("Switch light/dark theme.")) },
    Setting { band: "SERVER", name: "MCP", widths: &[MCP_ON, MCP_OFF], look: |app| Look::Button(String::from(app.mcp_label()), false), press: |app, _, _| served(app, |settings| &mut settings.mcp), help: |app| (app.mcp_story(), format!("Toggle local MCP access to this project; each write is undoable. HTTP address: {}", mcp::address(app.settings.port))) },
    Setting { band: "SERVER", name: PORT, widths: &[], look: |app| Look::Port(app.port_label(), app.typing_port()), press: |app, _, reset| {
        app.settings.port_typing = (!reset).then(String::new);
        if reset { app.set_port(mcp::DEFAULT_PORT) }
        true
    }, help: |app| (app.port_label().trim_start_matches(PORT).trim().to_string(), String::from("Server port, 1024-65535. Press to type; Return applies, Escape cancels, double-press resets to 9393.")) },
    Setting { band: "SERVER", name: COPY_CONFIG, widths: &[COPY_CONFIG], look: |app| Look::Button(words(app.settings.copied > 0.0, COPIED, COPY_CONFIG), app.settings.copied > 0.0), press: |app, _, _| {
        app.copy_config();
        true
    }, help: |_| (String::new(), String::from("Copy the MCP address and JSON/TOML configuration for a client or assistant.")) },
    Setting { band: "SERVER", name: "FEEDBACK", widths: &[FEEDBACK_OFF], look: |app| Look::Button(words(app.settings.feedback, FEEDBACK_ON, FEEDBACK_OFF), false), press: |app, _, _| served(app, |settings| &mut settings.feedback), help: |app| (words(app.settings.feedback, "ON", "OFF"), String::from("Ask each MCP session to say how the MCP could be improved; COPY CONFIG carries the ask too. Press to toggle.")) },
    Setting { band: "SERVER", name: AGENT, widths: &[], look: |app| Look::Readout(app.agent.label(font::glyphs_in(controls_w() - 2 * (CONTROL_BORDER + TEXT_PAD) - font::width(AGENT, TEXT_SCALE) - TEXT_PAD, TEXT_SCALE))), press: |_, _, _| false, help: |app| (app.agent.status.client().map_or_else(|| String::from(NONE), str::to_uppercase), app.agent.story()) },
    Setting { band: "AUDIO", name: "BUFFER", widths: &[], look: |app| Look::Meter(app.settings.buffer.to_string(), buffer_rung(app.settings.buffer) as f32 / (BUFFER_LADDER.len() - 1) as f32, matches!(app.drag, Some(Drag::Buffer(_)))), press: |app, y, reset| {
        if !reset {
            app.drag = Some(Drag::Buffer(BufferDrag { origin_y: y, origin: app.settings.buffer }));
        } else if app.set_buffer(BUFFER_DEFAULT) {
            app.requests.push(Request::Buffer { frames: app.settings.buffer });
        }
        true
    }, help: |app| (format!("{} FRAMES", app.settings.buffer), String::from("Audio buffer, 256-8192 frames. Smaller reduces latency; larger reduces dropouts. Drag vertically; double-press resets to 256.")) },
    Setting { band: "BROWSER", name: "PREVIEW", widths: &[], look: |app| Look::Meter(format!("{}%", app.settings.preview), app.settings.preview as f32 / PREVIEW_MAX as f32, matches!(app.drag, Some(Drag::Preview(_)))), press: |app, y, reset| {
        if reset { app.set_preview(PREVIEW_MAX); } else { app.drag = Some(Drag::Preview(StepDrag { origin_y: y, origin: app.settings.preview })) }
        true
    }, help: |app| (format!("{}%", app.settings.preview), String::from("Browser audition volume. Drag vertically; double-press resets to 100%.")) },
    Setting { band: "AUDIO", name: "BPM", widths: &[FOLLOWS, IGNORES], look: |app| Look::Button(words(app.settings.bpm_follows_pitch, FOLLOWS, IGNORES), false), press: |app, _, _| flip(&mut app.settings.bpm_follows_pitch), help: |app| (words(app.settings.bpm_follows_pitch, "FOLLOWS PITCH", "IGNORES PITCH"), String::from("Retuning audio: FOLLOWS scales BPM with pitch, preserving bars; IGNORES preserves BPM and changes clip length. Press to toggle.")) },
    Setting { band: "BROWSER", name: "FACTORY", widths: &[HIDE_FACTORY, SHOW_FACTORY], look: |app| Look::Button(words(app.settings.factory, HIDE_FACTORY, SHOW_FACTORY), false), press: |app, _, _| {
        app.settings.factory ^= true;
        app.show_factory();
        true
    }, help: |app| (shown(app.settings.factory), String::from("Show/hide the built-in Instruments and Effects folders.")) },
    Setting { band: "ROLL", name: "GHOST NOTES", widths: &[HIDE_GHOSTS, SHOW_GHOSTS], look: |app| Look::Button(words(app.settings.ghost_notes, HIDE_GHOSTS, SHOW_GHOSTS), false), press: |app, _, _| flip(&mut app.settings.ghost_notes), help: |app| (shown(app.settings.ghost_notes), String::from("Show transposed Core oscillator pitches as ghost notes. Drag a ghost to retune its oscillator; press to toggle visibility.")) },
    Setting { band: "RULER", name: "BAR NUMBERS", widths: &[HIDE_NUMBERS, SHOW_NUMBERS], look: |app| Look::Button(words(app.settings.bar_numbers, HIDE_NUMBERS, SHOW_NUMBERS), false), press: |app, _, _| flip(&mut app.settings.bar_numbers), help: |app| (shown(app.settings.bar_numbers), String::from("Show bar numbers on the ruler, every bar when they fit, else every fourth; the ZOOM cell then reads the bar under the mouse. Bar 1 starts at beat 0. Press to toggle.")) },
    Setting { band: "TRACKS", name: "FOLLOW", widths: &[FOLLOW_ON, FOLLOW_OFF], look: |app| Look::Button(words(app.settings.follow, FOLLOW_ON, FOLLOW_OFF), false), press: |app, _, _| flip(&mut app.settings.follow), help: |app| (words(app.settings.follow, "ON", "OFF"), String::from("Toggle automatic paging to follow the playhead.")) },
    Setting { band: "ENGINE", name: "LOAD", widths: &[], look: |app| Look::Gauge(format!("LOAD {}%", load(app)), load(app) as f32 / 100.0), press: |_, _, _| false, help: |app| (format!("{}%", load(app)), format!("Peak-held buffer processing load; below 100% keeps up. {}", app.audio.as_ref().map_or(String::new(), |audio| latency_words(audio.timing())))) },
    Setting { band: "ENGINE", name: "DROPOUTS", widths: &[], look: |app| Look::Readout(dropouts(app)), press: |_, _, _| false, help: |app| (dropouts(app), String::from("Late audio buffers since launch. Increase BUFFER to reduce dropouts.")) },
];

fn controls_w() -> i32 {
    SETTINGS.iter().flat_map(|setting| setting.widths).map(|label| font::width(label, TEXT_SCALE)).max().unwrap_or(0) + 2 * TEXT_PAD
}

fn row_box(row: usize) -> Rect {
    Rect::new(BORDER + HEADER_PAD, RAIL_H + BORDER + HEADER_PAD + row as i32 * (CONTROL_H + HEADER_PAD), controls_w(), CONTROL_H)
}

fn setting_box(at: usize) -> Rect {
    let row = row_box(at.saturating_sub(1));
    let half = (row.w - HEADER_PAD) / 2;
    match at {
        0 => Rect::new(row.x, row.y, half, row.h),
        1 => Rect::new(row.x + row.w - half, row.y, half, row.h),
        _ => row,
    }
}

fn buffer_rung(frames: u32) -> usize {
    (0..BUFFER_LADDER.len()).min_by_key(|&rung| BUFFER_LADDER[rung].abs_diff(frames)).unwrap_or(0)
}

fn help_left() -> i32 {
    let first = row_box(0);
    first.x + first.w + 2 * BORDER
}

fn saves_left() -> i32 {
    help_left() + controls_w() + 2 * BORDER
}

fn column_rows_top() -> i32 {
    row_box(1).y
}

fn column_box(left: i32, width: i32, row: usize) -> Rect {
    Rect::new(left, column_rows_top() + row as i32 * (CONTROL_H + HEADER_PAD), width, CONTROL_H)
}

fn list_offset(scroll: i32, len: usize, fit: usize) -> usize {
    ((scroll / (CONTROL_H + HEADER_PAD)).max(0) as usize).min(len.saturating_sub(fit))
}

impl App {
    pub(super) fn open_settings(&mut self) {
        self.screen = Screen::Settings;
        self.settings.versions_scroll = 0;
        self.settings.edits_scroll = 0;
    }

    pub(super) fn preview_gain(&self) -> f32 {
        self.settings.preview as f32 / PREVIEW_MAX as f32
    }

    pub(super) fn settings_press(&mut self, name: &str, y: i32, reset: bool) -> bool {
        SETTINGS.iter().find(|setting| setting.name == name).is_some_and(|setting| (setting.press)(self, y, reset))
    }

    pub(super) fn typing_port(&self) -> bool {
        self.settings.port_typing.is_some()
    }

    pub(super) fn port_key(&mut self, key: Key) -> bool {
        let Some(typed) = &mut self.settings.port_typing else {
            return false;
        };
        match key {
            Key::Return => self.finish_port(),
            Key::Escape => {
                self.settings.port_typing = None;
                true
            }
            Key::Delete => typed.pop().is_some(),
            Key::Char(digit) if digit.is_ascii_digit() && typed.len() < PORT_DIGITS => {
                typed.push(digit);
                true
            }
            _ => false,
        }
    }

    pub(super) fn finish_port(&mut self) -> bool {
        let Some(typed) = self.settings.port_typing.take() else {
            return false;
        };
        if let Some(port) = typed.parse::<u16>().ok().filter(|port| *port >= mcp::LEAST_PORT) {
            self.set_port(port);
        }
        true
    }

    fn set_port(&mut self, port: u16) {
        self.settings.port = port;
        self.settle_server();
    }

    pub(super) fn copy_config(&mut self) {
        self.requests.push(Request::Copy { text: mcp::setup(self.settings.port, self.settings.feedback) });
        self.settings.copied = COPIED_SECONDS;
    }

    pub(super) fn linger_copied(&mut self, seconds: f32) -> bool {
        if self.settings.copied <= 0.0 {
            return false;
        }
        self.settings.copied = (self.settings.copied - seconds).max(0.0);
        true
    }

    fn draw_readout(&mut self, row: Rect, label: &str, value: &str) {
        self.list.text(row.x + CONTROL_BORDER + TEXT_PAD, label_y(row), label, Tone::Ink50, TEXT_SCALE);
        self.list.text(row.x + row.w - CONTROL_BORDER - TEXT_PAD - font::width(value, TEXT_SCALE), label_y(row), value, Tone::Ink50, TEXT_SCALE);
    }

    fn mcp_label(&self) -> &'static str {
        if !self.settings.mcp {
            MCP_OFF
        } else if self.agent.serving() {
            MCP_ON
        } else {
            PORT_TAKEN
        }
    }

    fn port_label(&self) -> String {
        match &self.settings.port_typing {
            Some(typed) => format!("{PORT} {typed}"),
            None => format!("{PORT} {}", self.settings.port),
        }
    }

    fn draw_port(&mut self, rect: Rect, label: &str, hover: bool, typing: bool) {
        self.draw_control(rect, CONTROL_BORDER, label, false, hover, typing);
        if typing {
            let x = rect.x + (rect.w + font::width(label, TEXT_SCALE)) / 2 + TEXT_SCALE;
            self.list.fill(Rect::new(x, label_y(rect), TEXT_SCALE, font::GLYPH_H * TEXT_SCALE), Tone::Ground);
        }
    }

    pub(super) fn settings_drag(&mut self, y: i32) -> bool {
        match self.drag {
            Some(Drag::Contrast(drag)) => self.set_contrast(drag.origin + (drag.origin_y - y) / CONTRAST_PX_PER_STEP),
            Some(Drag::ScreenZoom(drag)) => {
                let (travel, pitch, wobble) = (drag.origin_y - self.on_glass(y), ZOOM_PX_PER_STEP * ZOOM_DEFAULT, ZOOM_MAX);
                let held = (self.zoomed() - drag.origin) / ZOOM_STEP * pitch;
                (travel - held).abs() > pitch / 2 + wobble && self.set_zoom(drag.origin + (travel + pitch / 2).div_euclid(pitch) * ZOOM_STEP)
            }
            Some(Drag::Preview(drag)) => self.set_preview(drag.origin + (drag.origin_y - y) / PREVIEW_PX_PER_STEP),
            Some(Drag::Buffer(drag)) => {
                let rung = (buffer_rung(drag.origin) as i32 + (drag.origin_y - y) / BUFFER_PX_PER_STEP).clamp(0, BUFFER_LADDER.len() as i32 - 1);
                self.set_buffer(BUFFER_LADDER[rung as usize])
            }
            _ => false,
        }
    }

    pub(super) fn buffer_released(&mut self, drag: BufferDrag) {
        if self.settings.buffer != drag.origin {
            self.requests.push(Request::Buffer { frames: self.settings.buffer });
        }
    }

    pub(super) fn set_contrast(&mut self, contrast: i32) -> bool {
        let contrast = contrast.clamp(CONTRAST_MIN, CONTRAST_MAX);
        let changed = contrast != self.settings.contrast;
        self.settings.contrast = contrast;
        changed
    }

    pub fn lens(&mut self, width: i32, height: i32, pixels: i32) -> Lens {
        let pixels = pixels.max(1);
        let fits = (width * ZOOM_DEFAULT / MIN_W).min(height * ZOOM_DEFAULT / MIN_H) / ZOOM_STEP * ZOOM_STEP;
        self.settings.zoom_room = ((ZOOM_DEFAULT / pixels).max(ZOOM_MIN), fits.clamp(ZOOM_DEFAULT, ZOOM_MAX));
        Lens { percent: self.zoomed(), pixels }
    }

    pub(super) fn zoomed(&self) -> i32 {
        let (least, most) = self.settings.zoom_room;
        self.settings.zoom.clamp(least, most)
    }

    fn on_glass(&self, y: i32) -> i32 {
        y * self.zoomed()
    }

    fn set_zoom(&mut self, percent: i32) -> bool {
        let (least, most) = self.settings.zoom_room;
        let percent = percent.clamp(least, most);
        let changed = percent != self.zoomed();
        if changed {
            self.settings.zoom = percent;
        }
        changed
    }

    fn reset_zoom(&mut self) -> bool {
        std::mem::replace(&mut self.settings.zoom, ZOOM_DEFAULT) != ZOOM_DEFAULT
    }

    pub(super) fn zoom_key(&mut self, key: char) -> bool {
        match key {
            '-' => self.set_zoom(self.zoomed() - ZOOM_STEP),
            '0' => self.reset_zoom(),
            _ => self.set_zoom(self.zoomed() + ZOOM_STEP),
        }
    }

    pub(super) fn set_preview(&mut self, level: i32) -> bool {
        let level = level.clamp(0, PREVIEW_MAX);
        let changed = level != self.settings.preview;
        self.settings.preview = level;
        if changed && let Some(audio) = &self.audio {
            audio.preview_gain(level as f32 / PREVIEW_MAX as f32);
        }
        changed
    }

    pub(super) fn set_buffer(&mut self, frames: u32) -> bool {
        let changed = frames != self.settings.buffer;
        self.settings.buffer = frames;
        changed
    }

    pub(super) fn settings_hover(&self, x: i32, y: i32) -> Hover {
        if let Some((_, setting)) = SETTINGS.iter().enumerate().find(|(at, _)| setting_box(*at).contains(x, y)) {
            return Hover::Setting(setting.name);
        }
        if !self.disk.versions.is_empty() {
            for (at, rect) in self.saves_column().rows() {
                if rect.contains(x, y) {
                    return Hover::Version(self.disk.versions.len() - 1 - at);
                }
            }
        }
        for (at, rect) in self.edits_column().rows() {
            if rect.contains(x, y) {
                return Hover::Edit(self.history.steps() - 1 - at);
            }
        }
        Hover::Nothing
    }

    fn saves_column(&self) -> Column {
        let fit = self.column_fit();
        Column { left: saves_left(), width: self.saves_width(), count: self.disk.versions.len(), offset: list_offset(self.settings.versions_scroll, self.disk.versions.len(), fit), fit }
    }

    fn edits_column(&self) -> Column {
        let fit = self.column_fit();
        let count = self.history.recent(EDITS).len();
        Column { left: self.edits_left(), width: self.edits_width(), count, offset: list_offset(self.settings.edits_scroll, count, fit), fit }
    }

    pub(super) fn column_fit(&self) -> usize {
        ((self.height - BORDER - column_rows_top() + HEADER_PAD) / (CONTROL_H + HEADER_PAD)).max(0) as usize
    }

    pub(super) fn save_story(&self, index: usize) -> String {
        let version = &self.disk.versions[index];
        story(version.at, version.tracks, version.length)
    }

    pub(super) fn saves_width(&self) -> i32 {
        let widest = (0..self.disk.versions.len()).map(|index| font::width(&self.save_story(index), TEXT_SCALE)).max().unwrap_or(0);
        (widest.max(font::width("SAVES", TEXT_SCALE)) + 2 * TEXT_PAD).min((self.width - BORDER - HEADER_PAD - saves_left()) / 2 - BORDER)
    }

    pub(super) fn edits_left(&self) -> i32 {
        saves_left() + if self.disk.versions.is_empty() { 0 } else { self.saves_width() + 2 * BORDER }
    }

    pub(super) fn edits_width(&self) -> i32 {
        let widest = self.history.recent(EDITS).iter().map(|step| font::width(&edit_story(step), TEXT_SCALE)).max().unwrap_or(0);
        (widest.max(font::width("EDITS", TEXT_SCALE)) + 2 * TEXT_PAD).min(self.width - BORDER - HEADER_PAD - self.edits_left())
    }

    pub(super) fn scroll_settings(&mut self, x: i32, y: i32, dy: i32) -> bool {
        let mut scrolled = false;
        let (saves, edits) = (self.saves_column(), self.edits_column());
        if !self.disk.versions.is_empty() && saves.rect(self.height).contains(x, y) {
            let scroll = (self.settings.versions_scroll - dy).clamp(0, saves.most());
            scrolled = scroll != self.settings.versions_scroll;
            self.settings.versions_scroll = scroll;
        } else if edits.rect(self.height).contains(x, y) {
            let scroll = (self.settings.edits_scroll - dy).clamp(0, edits.most());
            scrolled = scroll != self.settings.edits_scroll;
            self.settings.edits_scroll = scroll;
        }
        scrolled
    }

    pub(super) fn draw_settings(&mut self) {
        self.list.frame(Rect::new(0, RAIL_H, self.width, self.height - RAIL_H), BORDER, Tone::Ink100);
        for (at, setting) in SETTINGS.iter().enumerate() {
            let (rect, hover) = (setting_box(at), self.hover == Hover::Setting(setting.name));
            match (setting.look)(self) {
                Look::Button(label, lit) => self.draw_control(rect, CONTROL_BORDER, &label, false, hover, lit),
                Look::Meter(value, fraction, pressed) => self.draw_meter(rect, CONTROL_BORDER, setting.name, &value, fraction, hover, pressed),
                Look::Port(label, typing) => self.draw_port(rect, &label, hover, typing),
                Look::Readout(value) => self.draw_readout(rect, setting.name, &value),
                Look::Gauge(label, fraction) => self.draw_meter(rect, CONTROL_BORDER, &label, &label, fraction, false, false),
            }
        }
        if !self.disk.versions.is_empty() {
            let saves = self.saves_column();
            let rows: Vec<(usize, Rect, String)> = saves.rows().map(|(at, rect)| {
                let index = self.disk.versions.len() - 1 - at;
                (index, rect, font::shortened(&self.save_story(index), font::glyphs_in(rect.w - 2 * TEXT_PAD, TEXT_SCALE)))
            }).collect();
            self.list.text(saves.label_x(), label_y(row_box(0)), "SAVES", Tone::Ink50, TEXT_SCALE);
            for (index, rect, label) in &rows {
                self.draw_control(*rect, CONTROL_BORDER, label, false, self.hover == Hover::Version(*index), *index + 1 == self.disk.versions.len());
            }
        }
        let edits = self.edits_column();
        let rows: Vec<(usize, Rect, String)> = {
            let recent = self.history.recent(EDITS);
            edits.rows().map(|(at, rect)| (self.history.steps() - 1 - at, rect, font::shortened(&edit_story(&recent[edits.count - 1 - at]), font::glyphs_in(rect.w - 2 * TEXT_PAD, TEXT_SCALE)))).collect()
        };
        self.list.text(edits.label_x(), label_y(row_box(0)), "EDITS", Tone::Ink50, TEXT_SCALE);
        for (index, rect, label) in &rows {
            self.draw_control(*rect, CONTROL_BORDER, label, false, self.hover == Hover::Edit(*index), *index + 1 == self.history.steps());
        }
        self.draw_settings_help();
    }

    fn settings_help(&self) -> Option<(&'static str, String, String, String)> {
        let (band, name, value, desc) = match self.hover {
            Hover::Setting(name) => {
                let setting = SETTINGS.iter().find(|setting| setting.name == name)?;
                let (value, desc) = (setting.help)(self);
                (setting.band, String::from(name), value, desc)
            }
            Hover::Version(index) if index < self.disk.versions.len() => ("SAVES", self.save_story(index), String::new(), String::from("Saved versions, newest first. Press to restore; the current state remains in EDITS.")),
            Hover::Edit(index) => ("EDITS", edit_story(self.history.peek(index)?), String::new(), String::from("Session edits, newest first. Press to restore; the current state remains in history.")),
            _ => return None,
        };
        Some((band, name, value, desc))
    }

    fn mcp_story(&self) -> String {
        if !self.settings.mcp {
            String::from("OFF")
        } else if self.agent.serving() {
            String::from("ON")
        } else {
            format!("OFF \u{b7} PORT {} IS TAKEN", self.settings.port)
        }
    }

    fn draw_settings_help(&mut self) {
        let Some((band, name, value, desc)) = self.settings_help() else {
            return;
        };
        let desc = desc.as_str();
        let row = Rect::new(help_left(), row_box(0).y, controls_w(), CONTROL_H);
        let top = row.y;
        let inner_w = row.w - 2 * (CONTROL_BORDER + TEXT_PAD);
        let head = HELP_LINE * if value.is_empty() { 2 } else { 3 };
        let room = self.height - BORDER - top - 2 * (CONTROL_BORDER + TEXT_PAD) - head;
        let mut lines = font::wrapped(desc, font::glyphs_in(inner_w, HINT_SCALE));
        lines.truncate((room / HINT_LINE).max(0) as usize);
        let rect = Rect::new(row.x, top, row.w, 2 * (CONTROL_BORDER + TEXT_PAD) + head + lines.len() as i32 * HINT_LINE);
        self.list.frame(rect, CONTROL_BORDER, Tone::Ink100);
        let x = row.x + CONTROL_BORDER + TEXT_PAD;
        let mut y = top + CONTROL_BORDER + TEXT_PAD;
        let glyphs = font::glyphs_in(inner_w, TEXT_SCALE);
        for (text, tone) in [(String::from(band), Tone::Ink50), (name, Tone::Ink75), (value, Tone::Ink100)] {
            if text.is_empty() {
                continue;
            }
            self.list.text(x, y, &font::shortened(&text, glyphs), tone, TEXT_SCALE);
            y += HELP_LINE;
        }
        for line in &lines {
            self.list.text(x, y, line, Tone::Ink75, HINT_SCALE);
            y += HINT_LINE;
        }
    }
}
