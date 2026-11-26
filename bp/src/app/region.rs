use super::*;
use super::automation::Armed;
use super::edit::Clipboard;

#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) enum Covered {
    Track(usize),
    Curve(Host, Armed),
}

fn relative_host(host: Host, top: usize) -> Host {
    match host {
        Host::Track(track) => Host::Track(track - top),
        Host::Send(_) | Host::Master => host,
    }
}

fn landed_host(host: Host, top: Option<usize>) -> Option<Host> {
    match host {
        Host::Track(offset) => top.map(|top| Host::Track(top + offset)),
        Host::Send(_) | Host::Master => Some(host),
    }
}

#[derive(Clone, PartialEq, Debug)]
pub(super) struct Region {
    pub(super) start: i32,
    pub(super) end: i32,
    pub(super) covers: Vec<Covered>,
}

impl Region {
    pub(super) fn length(&self) -> i32 {
        self.end - self.start
    }

    pub(super) fn range(&self) -> Loop {
        Loop { start: self.start, end: self.end }
    }

    pub(super) fn holds(&self, clip: &Clip) -> bool {
        clip.start < self.end && clip.end() > self.start
    }

    pub(super) fn label(&self) -> String {
        let ticks = self.length();
        if ticks % TICKS_PER_BAR == 0 {
            return counted(ticks / TICKS_PER_BAR, "BAR");
        }
        let length = beats(ticks);
        if length.fract() == 0.0 { counted(length as i32, "BEAT") } else { format!("{} BEATS", json::tidy(length)) }
    }
}

#[derive(Clone, Copy)]
pub(super) struct RegionDrag {
    pub(super) origin: (i32, i32),
    pub(super) at: (i32, i32),
    pub(super) armed: bool,
}

impl Armed {
    pub(super) fn keys(&self, curve: &Curve) -> bool {
        curve.aim == self.aim && curve.lfo == self.lfo
    }
}

impl App {
    pub(super) fn sweep_region(&mut self, x: i32, y: i32) {
        self.drag = Some(Drag::Region(RegionDrag { origin: (x, y), at: (x, y), armed: false }));
    }

    pub(super) fn region_moved(&mut self, mut drag: RegionDrag, x: i32, y: i32) -> bool {
        drag.at = (x, y);
        drag.armed |= (x - drag.origin.0).abs() >= DRAG_ARM_PX || (y - drag.origin.1).abs() >= DRAG_ARM_PX;
        self.drag = Some(Drag::Region(drag));
        if !drag.armed {
            return false;
        }
        let lane = self.ruler_lane();
        let lane = if self.shift { lane.free() } else { lane };
        let (from, to) = (lane.floored(drag.origin.0).max(0), lane.floored(x).max(0));
        let covers = self.covered_between(drag.origin.1.min(y), drag.origin.1.max(y));
        self.selection.region = Some(Region { start: from.min(to), end: lane.cell_end(from.max(to)), covers });
        self.selection.clips.clear();
        self.selection.paste_point = None;
        true
    }

    fn covered_between(&self, top: i32, bottom: i32) -> Vec<Covered> {
        let (automations, viewport) = (self.automations(), self.track_viewport());
        let covered = self.tiles().into_iter().filter_map(|(strip, tile)| {
            let lane = self.lane(tile).rect.intersect(viewport);
            (lane.h > 0 && lane.y <= bottom && top < lane.y + lane.h).then(|| match strip {
                Strip::Track(track) => Covered::Track(track),
                Strip::Curve(curve) => {
                    let (host, armed) = automations[curve].clone();
                    Covered::Curve(host, armed)
                }
            })
        });
        covered.collect()
    }

    pub(super) fn region_tracks(&self, region: &Region) -> Vec<usize> {
        let mut tracks = Vec::new();
        for covered in &region.covers {
            if let Covered::Track(track) = *covered {
                tracks.push(track);
                if self.project.tracks[track].folded {
                    tracks.extend(self.project.members_of(track));
                }
            }
        }
        tracks.sort_unstable();
        tracks.dedup();
        tracks
    }

    fn region_curves(region: &Region) -> impl Iterator<Item = (Host, Armed)> + '_ {
        region.covers.iter().filter_map(|covered| match covered {
            Covered::Curve(host, armed) => Some((*host, armed.clone())),
            Covered::Track(_) => None,
        })
    }

    fn region_passages(&self, region: &Region) -> Vec<(Covered, Passage)> {
        let (bpm, carry) = (self.project.bpm, !self.project.lock);
        let mut passages: Vec<(Covered, Passage)> = self.region_tracks(region).into_iter().map(|track| (Covered::Track(track), self.project.tracks[track].passage(region.start, region.end, carry, bpm))).collect();
        for (host, armed) in App::region_curves(region) {
            let (curves, edges) = self.project.automation(host).between(region.start, region.end);
            let (curves, edges) = (curves.into_iter().filter(|curve| armed.keys(curve)).collect(), edges.into_iter().filter(|curve| armed.keys(curve)).collect());
            passages.push((Covered::Curve(host, armed), Passage { clips: Vec::new(), curves, edges, length: region.length() }));
        }
        passages
    }

    fn lay_passages(&mut self, passages: Vec<(Covered, Passage)>, at: i32) -> Vec<Covered> {
        let (bpm, carry) = (self.project.bpm, !self.project.lock);
        let mut laid = Vec::new();
        for (covered, passage) in passages {
            match &covered {
                &Covered::Track(track) => {
                    let Some(owner) = self.project.tracks.get_mut(track) else {
                        continue;
                    };
                    owner.lay_passage(passage, at, carry, bpm);
                }
                Covered::Curve(host, armed) => {
                    let host = *host;
                    if !carry || !self.automatable(host, &armed.aim) {
                        continue;
                    }
                    let end = at + passage.length;
                    self.project.automation_mut(host).replace_where(|curve| armed.keys(curve), at, end, &passage.edges, &passage.curves);
                }
            }
            laid.push(covered);
        }
        laid
    }

    pub(super) fn duplicate_region(&mut self) -> bool {
        let Some(region) = self.selection.region.clone() else {
            return false;
        };
        let length = region.length();
        if region.end + length > LONGEST_TICKS {
            return false;
        }
        let passages = self.region_passages(&region);
        if self.lay_passages(passages, region.end).is_empty() {
            return false;
        }
        self.selection.region = Some(Region { start: region.end, end: region.end + length, covers: region.covers });
        true
    }

    pub(super) fn delete_region(&mut self) -> bool {
        let Some(region) = self.selection.region.clone() else {
            return false;
        };
        let (bpm, carry) = (self.project.bpm, !self.project.lock);
        for track in self.region_tracks(&region) {
            self.project.tracks[track].clear_between(region.start, region.end, carry, bpm);
        }
        for (host, armed) in App::region_curves(&region).filter(|_| carry) {
            self.project.automation_mut(host).replace_where(|curve| armed.keys(curve), region.start, region.end, &[], &[]);
        }
        true
    }

    pub(super) fn copy_region(&mut self) -> bool {
        let Some(region) = self.selection.region.clone() else {
            return false;
        };
        let passages = self.region_passages(&region);
        let owners = App::region_curves(&region).filter_map(|(host, _)| host.track());
        let top = self.region_tracks(&region).into_iter().chain(owners).min().unwrap_or(0);
        let relative = |covered: Covered| match covered {
            Covered::Track(track) => Covered::Track(track - top),
            Covered::Curve(host, armed) => Covered::Curve(relative_host(host, top), armed),
        };
        self.clipboard = Clipboard::Region(passages.into_iter().map(|(covered, passage)| (relative(covered), passage)).collect());
        true
    }

    pub(super) fn paste_region(&mut self, passages: Vec<(Covered, Passage)>) -> bool {
        let Some(length) = passages.first().map(|(_, passage)| passage.length) else {
            return false;
        };
        let (top, at) = match (&self.selection.region, self.selection.paste_point) {
            (Some(region), _) => (self.region_tracks(region).first().copied().or_else(|| self.landing(self.selection.track)), region.start),
            (None, Some((track, tick))) if track < self.project.tracks.len() => (Some(track), tick),
            (None, _) => (self.landing(self.selection.track), self.play_from),
        };
        if at + length > LONGEST_TICKS {
            return false;
        }
        let mut targets = Vec::with_capacity(passages.len());
        for (covered, passage) in passages {
            let Covered::Track(offset) = covered else {
                if let Covered::Curve(host, armed) = covered
                    && let Some(host) = landed_host(host, top).filter(|host| host.track().is_none_or(|track| track < self.project.tracks.len()))
                {
                    targets.push((Covered::Curve(host, armed), passage));
                }
                continue;
            };
            let Some(top) = top else {
                continue;
            };
            let track = top + offset;
            let audio = passage.clips.iter().any(|clip| clip.file.is_some());
            let fresh = if audio { Track::audio } else { Track::blank };
            while self.project.tracks.len() <= track {
                if !self.insert_track(self.project.tracks.len(), fresh(), true) {
                    break;
                }
            }
            if !self.project.tracks.get(track).is_some_and(|target| !target.group && (passage.clips.is_empty() || target.audio == audio)) {
                continue;
            }
            targets.push((Covered::Track(track), passage));
        }
        let laid = self.lay_passages(targets, at);
        if laid.is_empty() {
            return false;
        }
        let mut covers: Vec<Covered> = laid
            .into_iter()
            .map(|covered| match covered {
                Covered::Track(track) => Covered::Track(self.lane_of(track)),
                Covered::Curve(..) => covered,
            })
            .collect();
        covers.dedup();
        self.selection.region = Some(Region { start: at, end: at + length, covers });
        self.selection.clips.clear();
        self.selection.paste_point = None;
        true
    }

    pub(super) fn join_region(&mut self) -> bool {
        let Some(region) = self.selection.region.clone() else {
            return false;
        };
        let tracks = self.region_tracks(&region);
        if tracks.is_empty() {
            return false;
        }
        self.join_between(region.range(), tracks);
        true
    }
}
