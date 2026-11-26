use super::*;
use super::files::FileAction;
use super::transport::{Latch, TempoDrag};

const NAME_CHARS: usize = font::glyphs_in(TILE - 2 * BORDER - HEADER_PAD, TEXT_SCALE);
const COMMAND_KEYS: [char; 15] = ['a', 'c', 'd', 'f', 'g', 'i', 'j', 's', 'u', 'v', 'z', '-', '=', '+', '0'];

remembered!(Live {
    flag keyboard: bool = true, "keyboard";
    whole octave: i32 = keyboard::DEFAULT_OCTAVE, "octave", keyboard::OCTAVE_MIN..=keyboard::OCTAVE_MAX;
    state keys: Vec<KeyNote> = Vec::new();
    state hold: bool = false;
    state latched: Option<Latch> = None;
    state restrike: bool = false;
    state sounding: Vec<(usize, u8)> = Vec::new();
    state take: Option<Take> = None;
});

impl Live {
    pub(super) fn install(&mut self) {
        self.sounding.clear();
        self.keys.clear();
        self.take = None;
    }
}

#[derive(Clone, Copy)]
pub(super) struct KeyNote {
    key: char,
    pub(super) track: usize,
    pub(super) pitch: u8,
    pub(super) down: bool,
}

pub(super) struct Rename {
    pub(super) track: usize,
    pub(super) text: String,
}

impl App {
    pub(super) fn set_shift(&mut self, shift: bool) -> bool {
        self.shift = shift;
        let dragging_clip = matches!(self.drag, Some(Drag::Clip(_)));
        let repaint = self.palettes.iter_mut().fold(dragging_clip, |repaint, palette| palette.set_shift(shift) | repaint);
        if let Some(Drag::Region(drag)) = self.drag {
            return self.region_moved(drag, drag.at.0, drag.at.1) | repaint;
        }
        let Some(Drag::Tempo(drag)) = &mut self.drag else {
            return repaint;
        };
        let bpm = if shift { self.project.bpm } else { self.project.bpm.round() };
        self.project.bpm = bpm;
        *drag = TempoDrag { origin_y: drag.y, origin: bpm, y: drag.y };
        true
    }

    pub fn takes_command(key: Key) -> bool {
        matches!(key, Key::Char(pressed) if COMMAND_KEYS.contains(&pressed))
    }

    pub(super) fn key(&mut self, key: Key, shift: bool, command: bool, repeat: bool) -> bool {
        if self.disk.exporting.is_some() {
            return key == Key::Escape && self.cancel_export();
        }
        if self.ask.is_some() {
            match key {
                Key::Return => self.answer(true),
                Key::Escape => self.answer(false),
                _ => return false,
            }
            return true;
        }
        if let Some(changed) = self.stock_key(key, repeat) {
            return changed;
        }
        if self.typing_port() {
            return self.port_key(key);
        }
        if self.face.typing() {
            return self.face_key(key, command);
        }
        if self.rename.is_some() {
            return self.rename_key(key, shift, command);
        }
        if self.core_shown() && self.rack.typing() {
            return self.rack_key(key, shift, command);
        }
        if self.browser.typing() {
            let (changed, action) = self.browser.type_key(key, shift, command);
            if let Some(action) = action {
                self.browser_act(action);
            }
            return changed;
        }
        match (key, command) {
            (Key::Char('z'), true) if !self.dragging() => {
                let step = if shift { self.history.redo() } else { self.history.undo() };
                let Some(step) = step else {
                    return false;
                };
                self.restore(step);
                true
            }
            (Key::Left | Key::Right | Key::Up | Key::Down, false) if !self.dragging() && self.browser_shown() && self.browser.has_selection() => {
                let (changed, action) = self.browser.step(key);
                if let Some(action) = action {
                    self.browser_act(action);
                }
                changed
            }
            (Key::Left | Key::Right | Key::Up | Key::Down, false) if self.browser_big() => false,
            (Key::Left | Key::Right | Key::Up | Key::Down, false) if !self.dragging() => self.nudge_span(key, shift) || self.nudge_file(key, shift) || self.nudge_cell(key, shift) || self.nudge_notes(key, shift) || self.nudge_clips(key, shift) || self.step_playhead(key, shift),
            (Key::Char(pressed @ ('-' | '=' | '+' | '0')), true) if !self.dragging() => self.zoom_key(pressed),
            _ if repeat => false,
            (Key::Return, false) => (self.browser_shown() && self.browser.begin_rename()) || (!self.browser_big() && self.begin_entry(None)),
            (Key::Space, false) => {
                if self.playing {
                    self.stop();
                } else {
                    self.play();
                }
                true
            }
            (Key::Char('s'), true) if !self.dragging() => {
                self.perform(if shift { FileAction::SaveAs } else { FileAction::SaveProject });
                false
            }
            (Key::Char('j' | 'f' | 'i' | 'd' | 'u' | 'g' | 'v'), true) if self.browser_big() => false,
            (Key::Char('j'), true) if !self.dragging() => self.join(),
            (Key::Char('f'), true) if !self.dragging() => self.search_rack(),
            (Key::Char('i'), true) if !self.dragging() => self.insert_silence(),
            (Key::Char('d'), true) if !self.dragging() => self.duplicate(),
            (Key::Char('u'), true) if !self.dragging() => self.quantize(),
            (Key::Char('g'), true) if !self.dragging() => self.group_or_ungroup(),
            (Key::Char('c'), true) if !self.dragging() => {
                let shown = self.copy();
                self.carry_waves();
                shown
            }
            (Key::Char('v'), true) if !self.dragging() => {
                let seeking = self.borrow_waves();
                let shown = self.paste();
                if seeking {
                    self.reread();
                    self.recover_missing();
                }
                shown
            }
            (Key::Char('a'), true) => self.pick_all_notes() || self.pick_all_rows(),
            (Key::Escape, false) => {
                if self.browser_shown() && self.browser.escape() {
                    return true;
                }
                if self.view.auto != Auto::Off {
                    self.show_auto(Auto::Off);
                    return true;
                }
                if self.roll_shown() && self.roll.unpick() {
                    return true;
                }
                if self.core_shown() && (self.face.unpick() || self.rack.unpick() || self.wave_face.unpick() || self.wave_escape()) {
                    return true;
                }
                let had = !self.selection.clips.is_empty() || !self.selection.track_picks.is_empty() || self.selection.paste_point.is_some() || self.selection.region.is_some();
                self.selection.clear();
                self.selection.paste_point = None;
                had
            }
            (Key::Delete, false) if self.browser_big() => !self.dragging() && self.browser_delete(),
            (Key::Delete, false) if !self.dragging() => self.browser_delete() || self.delete_picked_notes() || self.delete_picked_cell() || self.delete_picked_span() || self.delete_picked_file_cell() || self.delete_picked_rows() || self.delete_region() || self.delete_selected_clips() || self.delete_picked_tracks(),
            (Key::Char('a'), false) if !self.live.keyboard && !self.dragging() => {
                self.tap(Button::Automation);
                true
            }
            (Key::Char('b'), false) if !self.dragging() => !self.browser_big() && self.bypass_picked_rows(),
            (Key::Char(key), false) => (face::starts_value(key) && !self.browser_big() && self.begin_entry(Some(key))) || self.key_note_down(key),
            _ => false,
        }
    }

    pub(super) fn begin_entry(&mut self, first: Option<char>) -> bool {
        let Some((host, row)) = self.shown().filter(|_| self.core_shown() && self.face.picking()) else {
            return false;
        };
        self.project.rack(host)[row].faced().begin(&mut self.face, first)
    }

    fn face_key(&mut self, key: Key, command: bool) -> bool {
        let Some((host, row)) = self.shown() else {
            return self.face.leave();
        };
        let bpm = self.project.bpm;
        self.project.rack_mut(host)[row].faced_mut().key(&mut self.face, key, command, bpm)
    }

    pub(super) fn rename_key(&mut self, key: Key, shift: bool, command: bool) -> bool {
        let typed = match key {
            Key::Return => return self.finish_rename(),
            Key::Escape => {
                self.rename = None;
                return true;
            }
            Key::Delete => None,
            Key::Space => Some(' '),
            Key::Char(c) => Some(if shift { c.to_ascii_uppercase() } else { c }),
            Key::Left | Key::Right | Key::Up | Key::Down => return false,
        };
        let Some(rename) = &mut self.rename else {
            return false;
        };
        match typed {
            None => rename.text.pop().is_some(),
            Some(_) if command || rename.text.chars().count() >= NAME_CHARS => false,
            Some(c) => {
                rename.text.push(c);
                true
            }
        }
    }

    pub(super) fn finish_rename(&mut self) -> bool {
        let Some(rename) = self.rename.take() else {
            return false;
        };
        let name = rename.text.trim();
        if !name.is_empty() {
            self.project.tracks[rename.track].name = String::from(name);
        }
        true
    }

    pub(super) fn key_note_down(&mut self, key: char) -> bool {
        if !self.live.keyboard {
            return false;
        }
        if let Some(step) = keyboard::octave_step(key) {
            self.live.octave = (self.live.octave + step).clamp(keyboard::OCTAVE_MIN, keyboard::OCTAVE_MAX);
            return false;
        }
        let Some(pitch) = keyboard::pitch(key, self.live.octave) else {
            return false;
        };
        let tracks = self.project.leading(self.selection.picked_tracks());
        if self.live.hold && !self.live.keys.iter().any(|note| note.down) {
            self.release_keys(|note| !note.down);
        }
        self.release_keys(|note| note.pitch == pitch);
        let range = self.loop_beats();
        if let Some(audio) = &self.audio {
            for track in &tracks {
                audio.note_on(*track, pitch, VELOCITY);
            }
            if let Some(take) = &mut self.live.take {
                take.note_on(audio.position(), range, pitch);
            }
        }
        for track in tracks {
            self.live.keys.push(KeyNote { key, track, pitch, down: true });
        }
        false
    }

    pub(super) fn key_up(&mut self, key: Key) -> bool {
        match key {
            Key::Char(key) => self.lift_key(key),
            Key::Left | Key::Right | Key::Up | Key::Down => self.roll.silence(),
            Key::Space | Key::Return | Key::Escape | Key::Delete => {}
        }
        false
    }

    pub(super) fn nudge_notes(&mut self, key: Key, shift: bool) -> bool {
        let Some((track, index)) = self.edited_clip().filter(|_| self.roll_shown()) else {
            return false;
        };
        let (direction, rows) = match (key, shift) {
            (Key::Left, true) => return self.roll.stretch_picked(&mut self.project.tracks[track].clips[index], -1),
            (Key::Right, true) => return self.roll.stretch_picked(&mut self.project.tracks[track].clips[index], 1),
            (Key::Left, false) => (-1, 0),
            (Key::Right, false) => (1, 0),
            (Key::Up, false) => (0, 1),
            (Key::Up, true) => (0, 12),
            (Key::Down, false) => (0, -1),
            (Key::Down, true) => (0, -12),
            _ => return false,
        };
        self.roll.nudge(&mut self.project.tracks[track].clips[index], direction, rows)
    }

    pub(super) fn lift_key(&mut self, key: char) {
        if self.live.hold {
            for note in self.live.keys.iter_mut().filter(|note| note.key == key) {
                note.down = false;
            }
        } else {
            self.release_keys(|note| note.key == key);
        }
    }

    pub(super) fn blur(&mut self) -> bool {
        let down: Vec<char> = self.live.keys.iter().filter(|note| note.down).map(|note| note.key).collect();
        for key in down {
            self.lift_key(key);
        }
        self.roll.silence();
        false
    }

    pub(super) fn release_keys(&mut self, mut chosen: impl FnMut(&KeyNote) -> bool) {
        let range = self.loop_beats();
        let audio = self.audio.as_ref();
        let at = audio.map(Audio::position);
        let take = &mut self.live.take;
        self.live.keys.retain(|note| {
            if !chosen(note) {
                return true;
            }
            if let Some(audio) = audio {
                audio.note_off(note.track, note.pitch);
            }
            if let (Some(take), Some(at)) = (take.as_mut(), at) {
                take.note_off(at, range, note.pitch);
            }
            false
        });
    }

    pub(super) fn pick_all_notes(&mut self) -> bool {
        let Some((track, index)) = self.edited_clip().filter(|_| self.roll_shown()) else {
            return false;
        };
        self.roll.pick_all(&self.project.tracks[track].clips[index])
    }

    pub(super) fn delete_picked_notes(&mut self) -> bool {
        let Some((track, index)) = self.edited_clip().filter(|_| self.roll_shown()) else {
            return false;
        };
        self.roll.delete_picked(&mut self.project.tracks[track].clips[index])
    }
}
