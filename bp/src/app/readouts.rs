use super::*;
use super::files::DELAY;
use super::pool::{Landing, Peek, Score};
use super::presets::{STOCK_DESC, Site, stocked};

const WAVE_HINT: &str = "Press to hear, double-press for a new audio track";
const WAVE_PLAYING_HINT: &str = "Press again to stop it; double-press for a new audio track";
const MIDI_HINT: &str = "Press to hear through the selected track; double-press for a new MIDI track";
const MIDI_PLAYING_HINT: &str = "Press again to stop it; double-press for a new MIDI track";
const TIMES_HINT: &str = "Press for 2, 3 or 4 of this row; drag to move it";
const TIMES_FIXED_HINT: &str = "Press for 2, 3 or 4 of this row";
const SCORE_ROWS: i32 = 7;
const SCORE_ROW_H: f32 = 6.0;
const NOTE_GAP: i32 = 1;
const PLAYHEAD_HALO: i32 = 1;

fn draw_overview(list: &mut DrawList, area: Rect, wave: &Wave, progress: Option<f64>) {
    let half = area.h / 2;
    let middle = area.y + half;
    list.fill(Rect::new(area.x, middle, area.w, 1), Tone::Ink25);
    let per_column = wave.frames() as f64 / f64::from(area.w.max(1));
    for column in 0..area.w {
        let from = f64::from(column) * per_column;
        let h = grid::wave_height(wave.peak(from, from + per_column), 1.0, half);
        if h > 0 {
            list.fill(Rect::new(area.x + column, middle - h, 1, 2 * h), Tone::Ink100);
        }
    }
    let seconds = format!("{:.2} s", wave.seconds());
    list.text(area.x + area.w - font::width(&seconds, face::LABEL_SCALE), area.y, &seconds, Tone::Ink50, face::LABEL_SCALE);
    if let Some(progress) = progress {
        draw_playhead(list, area, area.x + (progress * f64::from(area.w)) as i32);
    }
}

fn draw_score(list: &mut DrawList, area: Rect, score: &Score, beat: Option<f64>) {
    let length = score.beats();
    let x_of = |beat: f64| area.x + ((beat / length * f64::from(area.w)).round() as i32).clamp(0, area.w);
    let (lo, hi) = score.pitch_range();
    let span = i32::from(hi) - i32::from(lo) + 1;
    let rows = span.max(SCORE_ROWS);
    let row_h = (area.h as f32 / rows as f32).min(SCORE_ROW_H);
    let top = area.y + ((area.h as f32 - row_h * rows as f32) / 2.0).round() as i32;
    let bottom_pitch = i32::from(lo) - (rows - span) / 2;
    let h = ((row_h - NOTE_GAP as f32).round() as i32).max(1);
    for note in &score.notes {
        let row = rows - 1 - (i32::from(note.pitch) - bottom_pitch);
        let y = top + (row as f32 * row_h).round() as i32;
        let (x0, x1) = (x_of(note.start), x_of(note.end));
        list.fill(Rect::new(x0, y, (x1 - x0 - NOTE_GAP).max(1), h), Tone::Ink100);
    }
    if let Some(beat) = beat {
        draw_playhead(list, area, x_of(beat));
    }
}

fn draw_playhead(list: &mut DrawList, area: Rect, x: i32) {
    let x = x.clamp(area.x, area.x + area.w - grid::PLAYHEAD_W);
    list.fill(Rect::new(x - PLAYHEAD_HALO, area.y, grid::PLAYHEAD_W + 2 * PLAYHEAD_HALO, area.h), Tone::Ground);
    list.fill(Rect::new(x, area.y, grid::PLAYHEAD_W, area.h), Tone::Ink100);
}

impl App {
    pub(super) fn draw_square_readout(&mut self) {
        let dragged = self.browser_shown().then(|| self.browser.lifted()).flatten().map(|(lifted, at)| {
            let rect = self.browser_rect();
            let (x, y) = (rect.x + at.0, rect.y + at.1);
            match lifted {
                Lifted::Preset { name, preset } => {
                    let mut shown = String::from(name);
                    let outcome = match (self.site_at(x, y), preset) {
                        (None, _) => (String::new(), "Drop on the Core, rack or a track, or outside the window as a file"),
                        (Some(_), None) => (String::from(UNREADABLE), "Not a preset this app can read"),
                        (Some(site), Some(preset)) => match self.plan(site, preset, false) {
                            Err(denial) => {
                                shown = turned_away(name, denial);
                                (String::from(denial.reason()), "Let go elsewhere to keep the rack")
                            }
                            Ok(plan) => plan_readout(site, plan),
                        },
                    };
                    ("PRESET", shown, outcome)
                }
                Lifted::Project { name } => ("PROJECT", String::from(name), (String::new(), "Drop outside the window as a file")),
                Lifted::Wave { name } => {
                    let outcome = match self.landing_at(x, y, browser::Kind::Wave) {
                        Some(Landing { track: Some(_), .. }) => (String::from("JOINS"), "Let go to place it on this audio track"),
                        Some(_) => (String::from("NEW TRACK"), "Let go for a new audio track"),
                        None => (String::new(), "Drop on the tracks, or outside the window as a file"),
                    };
                    (AUDIO, String::from(name), outcome)
                }
                Lifted::Midi { name } => {
                    let outcome = match self.landing_at(x, y, browser::Kind::Midi) {
                        Some(Landing { track: Some(_), .. }) => (String::from("JOINS"), "Let go to place it here, other parts on new tracks below"),
                        Some(_) => (String::from("NEW TRACK"), "Let go for new MIDI tracks, one per part"),
                        None => (String::new(), "Drop on the tracks, or outside the window as a file"),
                    };
                    (MIDI, String::from(name), outcome)
                }
            }
        });
        let rows = self.readout_rows();
        if let Some((band, name, (value, desc))) = dragged {
            face::show_readout(&mut self.list, rows, band, &name, &value, desc);
            return;
        }
        let lit = |on: bool| String::from(if on { "ON" } else { "OFF" });
        let glimpsed = self.glimpsed();
        let shown: Option<(&str, String, String, &str)> = match self.hover {
            Hover::Button(Button::Save) => Some(("PRESET", String::from("SAVE"), String::new(), "Saves this row, the whole rack or its effects alone as a JSON file")),
            Hover::Load => Some(("PRESET", String::from("LOAD"), String::new(), "Opens a preset; a Core rack replaces, others join")),
            Hover::Button(Button::Stock) => self.stock_kind().map(|kind| ("PRESET", String::from("FACTORY"), counted(stocked(kind).count() as i32, "PRESET"), STOCK_DESC)),
            Hover::StockOption(index) => self.stock.and_then(|panel| stocked(panel.kind).nth(index)).map(|entry| ("PRESET", entry.name.clone(), entry.kind.label().to_uppercase(), entry.description.as_str())),
            Hover::Button(Button::Show(Aside::Spectrum)) => Some(("SPECTRUM", String::from("SHOW"), lit(self.view.aside == Aside::Spectrum), "The master's output, 20 Hz to 20 kHz")),
            Hover::Button(Button::Show(Aside::Browser)) => Some(("BROWSER", String::from("SHOW"), lit(self.view.aside == Aside::Browser), "")),
            Hover::Button(Button::Show(Aside::Wave)) => Some((AUDIO, String::from("SHOW"), lit(self.view.aside == Aside::Wave), "The picked audio clip's file, its played part in ink")),
            Hover::Button(Button::Show(Aside::Pattern)) => Some(("PATTERN", String::from("SHOW"), lit(self.view.aside == Aside::Pattern), "The MIDI row's step pattern, lane by lane")),
            Hover::Button(Button::Show(Aside::Picture)) => Some(("PICTURE", String::from("BIG"), lit(self.view.aside == Aside::Picture), "Its picture in the big view, with room to edit")),
            Hover::Clear(lane) => self.automations().get(lane).map(|(_, armed)| {
                let desc = if armed.lfo.is_some() { "Deletes every node on this lane; LOCK keeps them" } else { "Deletes every node on this lane and its LFO; LOCK keeps them" };
                let (band, param) = armed.aim.words();
                ("AUTOMATION", String::from("CLEAR"), format!("{band} {param}"), desc)
            }),
            Hover::Roll => self.edited_clip().and_then(|(track, index)| self.roll.under(&self.project.tracks[track].clips[index])).map(|under| match under {
                roll::Under::Keys(keys) => ("ROLL", String::from("KEYS"), keys, "Drag across to zoom, up or down to scroll; double-press resets"),
                roll::Under::Zoom(bars) => ("ROLL", String::from("ZOOM"), bars, "Drag up or down to zoom, across to pan; double-press fits the clip"),
                roll::Under::Big(on) => ("ROLL", String::from("BIG"), lit(on), "Fills the window with the roll"),
                roll::Under::Save => ("ROLL", String::from("SAVE"), String::new(), "Saves the clip as a MIDI file"),
                roll::Under::Preview(on) => ("ROLL", String::from("PREVIEW"), lit(on), "Sounds notes as you press keys, draw or move them"),
                roll::Under::Guide(on) => ("ROLL", String::from("GUIDE"), lit(on), "Shows the other tracks' notes behind this clip's"),
                roll::Under::Fit => ("ROLL", String::from("FIT"), String::new(), "Lists the keys the clip's notes fit; pick one to set the song's"),
                roll::Under::Chords => ("ROLL", String::from("CHORDS"), String::new(), "Turns the clip's arp into chords, each held until the next"),
                roll::Under::Double => ("ROLL", String::from("×2"), String::new(), "Doubles the clip's length, its notes stretching with it"),
                roll::Under::Halve => ("ROLL", String::from("/2"), String::new(), "Halves the clip's length, its notes shrinking with it"),
                roll::Under::Reverse => ("ROLL", String::from("REV"), String::new(), "Reverses the clip's notes in time, the last heard first"),
                roll::Under::Up => ("ROLL", String::from("+OCT"), String::new(), "Moves every note up an octave"),
                roll::Under::Down => ("ROLL", String::from("−OCT"), String::new(), "Moves every note down an octave"),
                roll::Under::Tie(picked, join) => ("TIE", counted(picked as i32, "NOTE"), join.map_or_else(String::new, |join| String::from(join.spell())), "Cycles PLAY, TIE, SLIDE: TIE joins with no attack, SLIDE glides at GLD. SHIFT-press for PLAY"),
                roll::Under::Dial(dial, picked, value) => (dial.label(), counted(picked as i32, "NOTE"), value.map_or_else(String::new, |value| face::spell(dial.format(), value)), dial.describe()),
                roll::Under::Note(note) => ("NOTE", roll::name(note.pitch), articulated(&note), "Press to pick, drag to move or stretch; SHIFT-press adds, SHIFT-drag copies, double-press shortens to a step"),
            }),
            Hover::Lanes => self
                .shown_arp()
                .and_then(|arp| self.lanes.under(arp))
                .map(|under| match under {
                    lanes::Under::Length(length) => ("STEPS", counted(length as i32, "STEP"), String::new(), "Pattern length. Press to show 16, 32 or 64 steps; SHIFT-press sets 16"),
                    lanes::Under::Count(count) => ("STEPS", counted(count as i32, "STEP"), String::new(), "Press to make the pattern this long, or drag along the row"),
                    lanes::Under::Rests(rests) => ("REST", counted(rests as i32, "REST"), String::new(), "Silent steps that keep their notes. SHIFT-press clears them"),
                    lanes::Under::Rest(index, rested) => ("REST", format!("STEP {}", index + 1), lit(rested), "Press to rest or play it, drag to paint; SHIFT-press plays it"),
                    lanes::Under::Ties(ties) => ("TIE", counted(ties as i32, "TIE"), String::new(), "TIE joins the step before with no attack, SLIDE glides at GLD. SHIFT-press clears them"),
                    lanes::Under::Tie(index, join) => ("TIE", format!("STEP {}", index + 1), String::from(join.spell()), "Press to cycle PLAY, TIE, SLIDE, drag to paint; SHIFT-press for PLAY"),
                    lanes::Under::Lane(lane) => (arp::Lane::LABELS[lane.index()], String::new(), String::new(), lane.describe()),
                    lanes::Under::Velocity(fixed) => ("VEL", String::from(if fixed { "FIXED" } else { "FOLLOWS KEY" }), String::new(), "FIXED ignores the key's velocity, FOLLOWS KEY scales by it; SHIFT-press clears the lane"),
                    lanes::Under::Step(lane, index, value) => (arp::Lane::LABELS[lane.index()], format!("STEP {}", index + 1), lane.spell(value), step_hint(lane)),
                }),
            Hover::Browser => self.browser.under().and_then(|under| Some(match under {
                Under::Add => ("BROWSER", String::from("ADD FOLDER"), String::new(), "Press to choose a folder, or drop one in"),
                Under::Search { text, found } => {
                    let value = match found {
                        Some(found) => counted(found as i32, "FILE"),
                        None if text.is_empty() => String::new(),
                        None => String::from("SCANNING"),
                    };
                    ("BROWSER", String::from(if text.is_empty() { "SEARCH" } else { text }), value, "Search filenames in these folders. Up and Down select, Escape leaves")
                }
                Under::Clear => ("BROWSER", String::from("SEARCH"), String::from("CLEAR"), "Clears the search and shows the folders"),
                Under::Refresh => ("BROWSER", String::from("FOLDERS"), String::from("REFRESH"), "Reads every folder again from disk"),
                Under::Big => ("BROWSER", String::from("BIG"), lit(self.view.browser_big), "Fills the window with the browser"),
                Under::Locate => return self.pool.auditioned().map(|path| ("BROWSER", stem_of(path), String::from("FIND"), "Shows the sound that is playing")),
                Under::Collapse => ("BROWSER", String::from("FOLDERS"), String::from("CLOSE ALL"), "Closes every folder"),
                Under::Hearts(on) => ("BROWSER", String::from("HEARTED"), lit(on), "Shows only what you have hearted"),
                Under::Heart { name, on } => ("HEART", String::from(name), lit(on), "Keeps it under the heart at the top"),
                Under::Remove { name } => ("FOLDER", String::from(name), String::from("REMOVE"), "Removes the folder from the browser, not from disk"),
                Under::Reveal { name } => ("BROWSER", String::from(name), String::from("OPEN"), "Shows it on disk, in its folder"),
                Under::Copy { name, done } => ("BROWSER", String::from(name), String::from(if done { "COPIED" } else { "COPY" }), "Copies its path"),
                Under::Grip { name } => ("FOLDER", String::from(name), String::from("MOVE"), "Drag up or down to reorder"),
                Under::Folder { name, open, root, fixed } => ("FOLDER", String::from(name), lit(open), match (fixed, root) {
                    (true, true) => "Baking Powder's own presets; press to open or close it",
                    (true, false) | (false, true) => "Press to open or close it",
                    (false, false) => "Press to open or close it; Return renames, Delete trashes",
                }),
                Under::Preset { name, preset, author, about } => ("PRESET", byline(name, author), preset.name().to_uppercase(), match preset {
                    _ if !about.is_empty() => about,
                    Preset::Chords(_) => "Press to load it into the chord palette",
                    Preset::Rack(sound) if sound.rack.iter().any(Row::is_core) => "Press or drag to load; SHIFT loads only its MIDI and Core rows",
                    Preset::Rack(..) | Preset::Row(..) => "Press or drag to load",
                }),
                Under::Project { name, tracks, length } => ("PROJECT", format!("{name} \u{b7} {}", counted(bars(length), "BAR")), counted(tracks as i32, "TRACK"), "Double-press to open, or drag outside the window as a file"),
                Under::Unreadable { name } => ("FILE", String::from(name), String::from(UNREADABLE), "Not a preset or project; Delete trashes it"),
                Under::Wave { name, path } if self.pool.auditioning(path) => (AUDIO, String::from(name), String::from("PLAYING"), WAVE_PLAYING_HINT),
                Under::Wave { name, .. } => (AUDIO, String::from(name), String::from("WAV"), WAVE_HINT),
                Under::Midi { name, path, .. } if self.pool.auditioning(path) => (MIDI, String::from(name), String::from("PLAYING"), MIDI_PLAYING_HINT),
                Under::Midi { name, extension, .. } => (MIDI, String::from(name), extension.trim_start_matches('.').to_ascii_uppercase(), MIDI_HINT),
            })),
            Hover::Button(Button::Send(slot)) => {
                let send = self.send_ordinal(slot);
                match self.rack.landing() {
                    Some(row) => {
                        let label = self.project.rack(self.host())[row].label();
                        let short = self.send_short(send, row);
                        let (state, desc) = if short == 0 { ("JOINS", "Let go to move the effect to this return") } else { ("FULL", "Let go elsewhere to keep the effect") };
                        Some(("SEND", lacking(&format!("{} \u{b7} {label}", SEND_NAMES[send]), short, ROW), String::from(state), desc))
                    }
                    None => {
                        let state = if self.soloed == Some(send) {
                            "SOLO SEND"
                        } else if self.monitored == Some(Host::Send(send)) {
                            "ON"
                        } else {
                            "OFF"
                        };
                        Some(("SEND", String::from(SEND_NAMES[send]), String::from(state), "Press to open the return, again to solo it"))
                    }
                }
            }
            Hover::Item(index) => self.menu.and_then(|menu| self.delay_bar(menu, index)).map(|track| (TRACK, String::from(DELAY), milliseconds(self.project.tracks[track].delay_ms), "Drag up for later, down for earlier, 100 ms at most; SHIFT-drag for tenths, double-press for 0")),
            Hover::Rack => self.rack.handled().and_then(|row| self.project.rack(self.host()).get(row).map(|entry| (row, entry))).map(|(row, entry)| (entry.label(), String::from("TIMES"), format!("{}\u{d7}", entry.times()), if row == 0 && entry.is_midi() { TIMES_FIXED_HINT } else { TIMES_HINT })),
            _ => None,
        };
        let rows = self.readout_rows();
        let audio = self.audio.as_ref();
        match glimpsed {
            Some((name, Peek::Wave(wave), playing)) => {
                let progress = playing.then(|| audio.and_then(Audio::preview_progress)).flatten();
                let hint = if playing { WAVE_PLAYING_HINT } else { WAVE_HINT };
                face::show_pictured(&mut self.list, rows, AUDIO, &name, hint, |list, area| draw_overview(list, area, &wave, progress));
            }
            Some((name, Peek::Score(score), playing)) => {
                let beat = playing.then(|| audio.and_then(Audio::rehearsal_beat)).flatten();
                let hint = if playing { MIDI_PLAYING_HINT } else { MIDI_HINT };
                face::show_pictured(&mut self.list, rows, MIDI, &name, hint, |list, area| draw_score(list, area, &score, beat));
            }
            None => {
                if let Some((band, param, value, desc)) = shown {
                    face::show_readout(&mut self.list, rows, band, &param, &value, desc);
                }
            }
        }
    }

    fn glimpsed(&self) -> Option<(String, Peek, bool)> {
        if self.hover != Hover::Browser {
            return None;
        }
        let (name, path) = match self.browser.under()? {
            Under::Wave { name, path } | Under::Midi { name, path, .. } => (name, path),
            _ => return None,
        };
        Some((String::from(name), self.pool.peeked(path)?, self.pool.auditioning(path)))
    }
}

fn byline(name: &str, author: &str) -> String {
    if author.is_empty() { String::from(name) } else { format!("{name} by {author}") }
}

fn plan_readout(site: Site, plan: Plan) -> (String, &'static str) {
    let (value, desc) = match (plan, site) {
        (Plan::Palette, _) => ("REPLACES", "Let go to load it into the chord palette"),
        (Plan::Replaces, Site::Generator) => ("REPLACES", "Let go to replace the rack"),
        (Plan::Replaces, Site::Track(_)) => ("REPLACES", "Let go to make it this track's sound"),
        (Plan::Cores, _) => ("CORES", "Let go to swap in only its MIDI and Core rows"),
        (Plan::Joins { stripped: true }, _) => ("JOINS", "Let go to add its effects, not its MIDI and Core rows"),
        (Plan::Joins { .. }, Site::Generator) => ("JOINS", "Let go to add it to the end of this rack"),
        (Plan::Joins { .. }, Site::Track(_)) => ("JOINS", "Let go to add it to the end of the track's rack"),
    };
    (String::from(value), desc)
}

fn articulated(note: &Note) -> String {
    let articulation = note.articulation;
    let mut parts = vec![format!("VEL {}", note.velocity)];
    if articulation.cents != 0.0 {
        parts.push(face::spell(Format::SignedCents, f64::from(articulation.cents)));
    }
    if let Some(modulation) = articulation.modulation {
        parts.push(format!("MOD {}", face::spell(Format::SignedPercent, f64::from(modulation))));
    }
    if articulation.shift != 0 {
        parts.push(face::spell(Format::Semitones, f64::from(articulation.shift)));
    }
    if articulation.join != arp::Join::Play {
        parts.push(String::from(articulation.join.spell()));
    }
    parts.join(" \u{b7} ")
}

fn step_hint(lane: arp::Lane) -> &'static str {
    match lane {
        arp::Lane::Mod => "Held MOD value for this step's voice; drag to set, SHIFT-press resets",
        _ => "Drag to set, SHIFT-press resets; SHIFT-drag resets a run",
    }
}
