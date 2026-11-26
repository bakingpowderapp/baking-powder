use super::*;
use super::region::Covered;
use crate::project::TRACKS;
use std::cell::Cell;

#[derive(Clone)]
pub(super) enum Clipboard {
    Empty,
    Notes(Vec<Note>),
    Setting(Setting),
    Rows { rows: Vec<Copied>, song: u64, gone: Vec<Track> },
    Clips(Vec<(usize, Piece)>),
    Region(Vec<(Covered, Passage)>),
    Tracks { tracks: Vec<Track>, homes: Vec<Option<usize>>, song: u64, gone: Vec<Track> },
}

impl Clipboard {
    pub(super) fn clips(&self) -> Vec<&Clip> {
        match self {
            Clipboard::Clips(pieces) => pieces.iter().map(|(_, piece)| &piece.clip).collect(),
            Clipboard::Region(passages) => passages.iter().flat_map(|(_, passage)| &passage.clips).collect(),
            Clipboard::Tracks { tracks, .. } => tracks.iter().flat_map(|track| &track.clips).collect(),
            Clipboard::Empty | Clipboard::Notes(_) | Clipboard::Setting(_) | Clipboard::Rows { .. } => Vec::new(),
        }
    }

    pub(super) fn clips_mut(&mut self) -> Vec<&mut Clip> {
        match self {
            Clipboard::Clips(pieces) => pieces.iter_mut().map(|(_, piece)| &mut piece.clip).collect(),
            Clipboard::Region(passages) => passages.iter_mut().flat_map(|(_, passage)| &mut passage.clips).collect(),
            Clipboard::Tracks { tracks, .. } => tracks.iter_mut().flat_map(|track| &mut track.clips).collect(),
            Clipboard::Empty | Clipboard::Notes(_) | Clipboard::Setting(_) | Clipboard::Rows { .. } => Vec::new(),
        }
    }

    fn gone_from(&mut self, here: u64) -> Option<&mut Vec<Track>> {
        match self {
            Clipboard::Rows { song, gone, .. } | Clipboard::Tracks { song, gone, .. } if *song == here => Some(gone),
            Clipboard::Empty | Clipboard::Notes(_) | Clipboard::Setting(_) | Clipboard::Rows { .. } | Clipboard::Clips(_) | Clipboard::Region(_) | Clipboard::Tracks { .. } => None,
        }
    }

    fn remap(&mut self, map: impl Fn(usize) -> Option<usize>) {
        match self {
            Clipboard::Rows { rows, .. } => {
                for copied in rows.iter_mut() {
                    copied.row = copied.row.renumbered(&map);
                }
            }
            Clipboard::Tracks { tracks, homes, .. } => {
                for track in tracks.iter_mut() {
                    track.renumber(&map);
                }
                for home in homes.iter_mut() {
                    *home = home.and_then(&map);
                }
            }
            Clipboard::Empty | Clipboard::Notes(_) | Clipboard::Setting(_) | Clipboard::Clips(_) | Clipboard::Region(_) => {}
        }
    }

    pub(super) fn renumber(&mut self, here: u64, map: impl Fn(usize) -> Option<usize>) {
        if self.gone_from(here).is_some() {
            self.remap(|index| if index < TRACKS { map(index) } else { Some(index) });
        }
    }

    pub(super) fn lose(&mut self, here: u64, index: usize, track: &Track) {
        let Some(parked) = self.gone_from(here).map(|gone| TRACKS + gone.len()) else {
            return;
        };
        let held = Cell::new(false);
        self.remap(|reference| {
            held.set(held.get() || reference == index);
            Some(if reference == index { parked } else { reference })
        });
        if let Some(gone) = self.gone_from(here).filter(|_| held.get()) {
            gone.push(track.clone());
        }
    }

    pub(super) fn find(&mut self, here: u64, found: impl Fn(&Track) -> Option<usize>) {
        let Some(back) = self.gone_from(here).map(|gone| gone.iter().map(&found).collect::<Vec<Option<usize>>>()) else {
            return;
        };
        self.remap(|reference| Some(reference.checked_sub(TRACKS).and_then(|at| back.get(at).copied().flatten()).unwrap_or(reference)));
    }
}

#[derive(Clone)]
pub(super) struct Copied {
    pub(super) row: Row,
    pub(super) curves: Vec<Curve>,
    pub(super) lfos: Vec<LaneLfo>,
}

#[derive(Clone)]
pub(super) struct Setting {
    pub(super) band: &'static str,
    pub(super) param: &'static str,
    pub(super) value: f64,
    pub(super) curves: Vec<Curve>,
    pub(super) lfo: Option<LaneLfo>,
}

impl App {
    pub(super) fn join(&mut self) -> bool {
        if let Some((track, index)) = self.picked_notes() {
            return self.roll.join_picked(&mut self.project.tracks[track].clips[index]);
        }
        if self.selection.region.is_some() {
            return self.join_region();
        }
        if self.selection.clips.is_empty() {
            return self.join_loop();
        }
        self.join_selected_clips()
    }

    pub(super) fn quantize(&mut self) -> bool {
        let Some((track, index)) = self.edited_clip().filter(|_| self.roll_shown()) else {
            return false;
        };
        self.roll.quantize(&mut self.project.tracks[track].clips[index])
    }

    pub(super) fn picked_notes(&self) -> Option<(usize, usize)> {
        self.edited_clip().filter(|_| self.roll_shown() && self.roll.picking())
    }

    pub(super) fn duplicate(&mut self) -> bool {
        if let Some((track, index)) = self.picked_notes() {
            return self.roll.duplicate_picked(&mut self.project.tracks[track].clips[index]);
        }
        let rows = self.picked_rows();
        if !rows.is_empty() {
            return self.paste_rows(&rows, self.disk.song);
        }
        if self.selection.region.is_some() {
            return self.duplicate_region();
        }
        if !self.selection.clips.is_empty() {
            return self.duplicate_selected_clips();
        }
        let chosen = self.with_members(self.selection.picked_tracks());
        let last = chosen[chosen.len() - 1];
        let selected_at = chosen.iter().position(|track| *track == self.selection.track).unwrap_or(0);
        let kinds: Vec<&'static str> = chosen.iter().map(|&track| self.project.tracks[track].kind()).collect();
        let added = self.project.duplicate_tracks(&chosen);
        let added = self.inserted(last + 1, &kinds, added);
        if added > 0 {
            self.select(last + 1 + selected_at.min(added - 1));
            self.selection.track_picks = (last + 1..last + 1 + added).filter(|track| *track != self.selection.track).collect();
        }
        true
    }

    pub(super) fn copy(&mut self) -> bool {
        if let Some((track, index)) = self.picked_notes() {
            self.clipboard = Clipboard::Notes(self.roll.copy_picked(&self.project.tracks[track].clips[index]));
            return false;
        }
        if let Some(setting) = self.picked_setting() {
            self.clipboard = Clipboard::Setting(setting);
            return false;
        }
        let rows = self.picked_rows();
        if !rows.is_empty() {
            self.clipboard = Clipboard::Rows { rows, song: self.disk.song, gone: Vec::new() };
            return self.rack.panel_open();
        }
        if self.copy_region() {
            return false;
        }
        let picked = self.picked_pieces();
        let (Some(first), Some(top)) = (picked.iter().map(|(_, piece)| piece.clip.start).min(), picked.iter().map(|(track, _)| *track).min()) else {
            let homes = self.with_members(self.selection.picked_tracks());
            self.clipboard = Clipboard::Tracks { tracks: self.project.copies(&homes), homes: homes.into_iter().map(Some).collect(), song: self.disk.song, gone: Vec::new() };
            return false;
        };
        self.clipboard = Clipboard::Clips(picked.into_iter().map(|(track, piece)| (track - top, piece.shifted(-first))).collect());
        false
    }

    pub(super) fn paste(&mut self) -> bool {
        match self.clipboard.clone() {
            Clipboard::Empty => false,
            Clipboard::Notes(notes) => {
                let Some((track, index)) = self.edited_clip().filter(|_| self.roll_shown()) else {
                    return false;
                };
                self.roll.paste(&mut self.project.tracks[track].clips[index], &notes)
            }
            Clipboard::Setting(setting) => self.core_shown() && self.paste_setting(&setting),
            Clipboard::Rows { rows, song, .. } => self.core_shown() && self.paste_rows(&rows, song),
            Clipboard::Region(passages) => self.paste_region(passages),
            Clipboard::Clips(pieces) => {
                let Some((top, at)) = self.selection.paste_point.filter(|(track, _)| *track < self.project.tracks.len()).or_else(|| self.landing(self.selection.track).map(|top| (top, self.play_from))) else {
                    return false;
                };
                let span = pieces.iter().map(|(_, piece)| piece.clip.end()).max().unwrap_or(0);
                if span > LONGEST_TICKS - at {
                    return false;
                }
                let carry = !self.project.lock;
                let mut pasted = Vec::with_capacity(pieces.len());
                for (offset, piece) in pieces {
                    let track = top + offset;
                    let fresh = if piece.clip.file.is_some() { Track::audio } else { Track::blank };
                    while self.project.tracks.len() <= track {
                        if !self.insert_track(self.project.tracks.len(), fresh(), true) {
                            break;
                        }
                    }
                    if !self.project.tracks.get(track).is_some_and(|target| !target.group && target.audio == piece.clip.file.is_some()) {
                        continue;
                    }
                    let copy = piece.shifted(at);
                    pasted.push((track, copy.clip.start));
                    let bpm = self.project.bpm;
                    self.project.tracks[track].lay(copy, carry, bpm);
                }
                self.selection.clips = pasted;
                if !self.selection.clips.is_empty() {
                    self.selection.paste_point = Some((top, at + span));
                }
                true
            }
            Clipboard::Tracks { tracks, homes, song, .. } => {
                let at = self.selection.track + 1;
                let added = self.paste_tracks(at, tracks, &homes, song);
                if added > 0 {
                    self.select(at);
                    self.selection.track_picks = (at + 1..at + added).collect();
                }
                true
            }
        }
    }

    pub(super) fn settle_roll(&mut self) {
        self.roll.set_snap(self.timeline.snap.beats() as f32);
        match self.edited_clip() {
            Some((track, index)) => {
                let clip = &self.project.tracks[track].clips[index];
                if self.roll.shown() == Some((track, clip.start)) {
                    self.roll.settle(clip);
                } else {
                    self.roll.open((track, clip.start), clip);
                }
                if self.roll.picking() && self.selection.clips.len() > 1 {
                    self.selection.clips = vec![(track, clip.start)];
                }
            }
            None => self.roll.close(),
        }
    }

    pub(super) fn roll_hover(&mut self, x: i32, y: i32) -> bool {
        match self.edited_clip().filter(|_| self.hover == Hover::Roll) {
            Some((track, index)) => {
                let (rx, ry) = self.roll_point(x, y);
                self.roll.hover(rx, ry, &self.project.tracks[track].clips[index])
            }
            None => self.roll.leave(),
        }
    }

    pub(super) fn roll_down(&mut self, x: i32, y: i32, clicks: i32) -> bool {
        let Some((track, index)) = self.edited_clip() else {
            return false;
        };
        let clips = &self.project.tracks[track].clips;
        let room = clips.get(index + 1).map_or(LONGEST_TICKS, |next| next.start) - clips[index].end();
        let (changed, action) = self.roll.down(x, y, clicks, self.shift, &mut self.project.tracks[track].clips[index], room);
        match action {
            Some(roll::Action::Save) => {
                let clip = &self.project.tracks[track].clips[index];
                let stem = if clip.label.is_empty() { MIDI } else { clip.label.as_str() };
                let part = (clip.label.clone(), vec![crate::project::Clip { start: 0, ..clip.clone() }]);
                self.requests.push(Request::Midi { name: format!("{stem}.mid"), bytes: midi::write(&[part], self.project.bpm) });
                changed
            }
            Some(roll::Action::Fit) => {
                self.fits = self.project.tracks[track].clips[index].fits();
                if self.fits.is_empty() {
                    return changed;
                }
                self.menu = Some(Menu::Fit);
                true
            }
            Some(roll::Action::Ghost(ghost)) => self.ghost_down(track, ghost, x, y) || changed,
            None => changed,
        }
    }

    pub(super) fn roll_zoom_at(&mut self, x: i32, y: i32) -> bool {
        let Some((track, index)) = self.edited_clip() else {
            return false;
        };
        let (rx, ry) = self.roll_point(x, y);
        self.roll.zoom_at(rx, ry, &self.project.tracks[track].clips[index])
    }

    pub(super) fn roll_moved(&mut self, x: i32, y: i32) -> bool {
        let Some((track, index)) = self.edited_clip() else {
            return false;
        };
        self.roll.moved(x, y, &mut self.project.tracks[track].clips[index])
    }

    pub(super) fn roll_up(&mut self, x: i32, y: i32) -> bool {
        let Some((track, index)) = self.edited_clip().filter(|_| self.roll.dragging()) else {
            return false;
        };
        let (rx, ry) = self.roll_point(x, y);
        self.roll.up(rx, ry, &mut self.project.tracks[track].clips[index])
    }
}
