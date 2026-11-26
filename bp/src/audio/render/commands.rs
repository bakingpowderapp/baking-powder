use super::*;

impl Renderer {
    fn idle(&self) -> bool {
        if self.table.idle() {
            return true;
        }
        if self.patience < PATIENT {
            return false;
        }
        let began = Instant::now();
        while !self.table.idle() {
            if began.elapsed() >= self.patience {
                return false;
            }
            std::hint::spin_loop();
        }
        true
    }

    pub(crate) fn drain(&mut self) {
        self.unspill();
        if !self.idle() || matches!(self.splice, Splice::Down(_)) {
            return;
        }
        while self.pending.len() < self.pending.capacity()
            && let Ok(message) = self.receiver.try_recv()
        {
            self.pending.push(message);
        }
        let ready = self.affordable();
        if self.splice == Splice::Held {
            self.install_ready(ready);
            if !self.parted {
                self.splice = Splice::Up(0);
            }
            return;
        }
        if self.pending[..ready].iter().any(|message| matches!(message, Message::Split) || (self.playing && matches!(message, Message::Timing(plan) if plan.moves))) {
            self.ready = ready;
            self.splice = self.splice.down();
            return;
        }
        self.install_ready(ready);
    }

    fn affordable(&self) -> usize {
        let mut room = BUDGET.saturating_sub(self.retired.load(Ordering::Relaxed));
        let (mut open, mut cost, mut ready) = (false, 0, 0);
        for (at, message) in self.pending.iter().enumerate() {
            open = match message {
                Message::Begin => true,
                Message::Split | Message::Commit => false,
                _ => open,
            };
            cost += message.retires();
            if open {
                continue;
            }
            if cost > room {
                break;
            }
            room -= cost;
            cost = 0;
            ready = at + 1;
        }
        ready
    }

    pub(super) fn install_pending(&mut self) {
        let ready = std::mem::take(&mut self.ready);
        self.install_ready(ready);
    }

    fn install_ready(&mut self, ready: usize) {
        let now = self.frame;
        self.lanes_due |= ready > 0;
        let mut pending = std::mem::take(&mut self.pending);
        for message in pending.drain(..ready) {
            self.parted = match message {
                Message::Split => true,
                Message::Commit => false,
                _ => self.parted,
            };
            self.batches += u64::from(matches!(message, Message::Commit));
            self.replan |= matches!(message, Message::Insert { .. } | Message::Remove { .. } | Message::Move { .. } | Message::Clear | Message::Row { .. } | Message::RenewRow { .. } | Message::InsertRow { .. } | Message::RemoveRow { .. } | Message::MoveRow { .. });
            self.apply(message, now);
        }
        self.pending = pending;
        if std::mem::take(&mut self.replan) {
            self.plan();
        }
    }

    fn install(&mut self, mut plan: Box<Plan>) {
        for (index, cell) in self.table.tracks.iter().enumerate().take(self.count) {
            let Some(mut next) = plan.tracks.get_mut(index).and_then(Option::take) else {
                continue;
            };
            match hold(&cell.strip) {
                Some(mut strip) => {
                    next.adopt(strip.plan.as_deref_mut(), self.playing);
                    if let Some(old) = strip.plan.replace(next) {
                        self.retire(Displaced::Track(old));
                    }
                }
                None => self.retire(Displaced::Track(next)),
            }
        }
        for (cell, next) in self.table.sends.iter().zip(&plan.returns) {
            if let Some(mut ret) = grab(&cell.strip) {
                ret.plan = *next;
            }
        }
        self.master_rows = plan.master;
        self.output = plan.output;
        self.retire(Displaced::Plan(plan));
        let mut reach = 0;
        self.backs(|back| reach = reach.max(back));
        self.reach = reach;
    }

    fn reset_lines(&self) {
        for cell in self.table.tracks.iter().take(self.count) {
            if let Some(mut strip) = hold(&cell.strip) {
                strip.out.clear();
                strip.pre.clear();
                if let Some(twin) = strip.twin.as_deref_mut() {
                    twin.clear();
                }
                if let Some(plan) = strip.plan.as_deref_mut() {
                    plan.clear();
                }
            }
        }
        for cell in &self.table.sends {
            if let Some(mut ret) = grab(&cell.strip) {
                ret.line.clear();
            }
        }
    }

    fn apply(&mut self, message: Message, now: i64) {
        match message {
            Message::Begin | Message::Split | Message::Commit => {}
            Message::Timing(plan) => self.install(plan),
            Message::Insert { at, rack, strip } => self.insert_track(at, rack, strip),
            Message::Remove { at } => self.remove_track(at),
            Message::Move { from, to } => self.move_tracks(from, to),
            Message::Clear => self.clear_all(),
            Message::Row { host, index, row } => self.set_row(host, index, row),
            Message::RenewRow { host, index, insert } => self.renew_row(host, index, insert),
            Message::InsertRow { host, index, insert, joins } => self.insert_row(host, index, insert, joins),
            Message::RemoveRow { host, index } => self.remove_row(host, index),
            Message::MoveRow { host, from, to } => self.move_row(host, from, to),
            Message::Vision { host, index, vision, twin } => self.land(host, index, vision, twin),
            Message::Forget { host, index } => self.forget(host, index),
            Message::Twins { track, lines, units } => self.twin_track(track, lines, units),
            Message::Strip { track, gain, heard, pours, unmuted, group } => {
                if let Some(cell) = self.table.tracks.get(track)
                    && let Some(mut strip) = hold(&cell.strip)
                {
                    strip.gain.set(gain);
                    strip.heard = heard;
                    strip.pours = pours;
                    strip.unmuted = unmuted;
                    self.replan |= strip.group != group;
                    strip.group = group;
                }
            }
            Message::Sends { track, amounts } => {
                if let Some(cell) = self.table.tracks.get(track)
                    && let Some(mut strip) = hold(&cell.strip)
                {
                    strip.sends = amounts;
                }
            }
            Message::Master { gain } => self.master.set(gain),
            Message::PreviewGain { gain } => self.preview_gain = gain,
            Message::Return { send, gain } => {
                if let Some(cell) = self.table.sends.get(send)
                    && let Some(mut ret) = grab(&cell.strip)
                {
                    ret.gain.set(gain);
                }
            }
            Message::SendSolo { send } => self.solo = send,
            Message::CoreSolo { track, solo } => {
                if let Some(cell) = self.table.tracks.get(track)
                    && let Some(mut strip) = hold(&cell.strip)
                {
                    strip.core_solo = solo;
                }
            }
            Message::Plays { track, plays } => {
                if let Some(cell) = self.table.tracks.get(track)
                    && let Some(mut strip) = hold(&cell.strip)
                {
                    let old = std::mem::replace(&mut strip.plays, plays);
                    self.retire(Displaced::Plays(old));
                } else {
                    self.retire(Displaced::Plays(plays));
                }
            }
            Message::Preview(cue) => {
                if let Some(retired) = self.fading.take() {
                    self.retire(Displaced::Preview(retired.cue));
                }
                self.fading = self.preview.take();
                self.preview = cue.map(|cue| {
                    let wait = if cue.warp.is_some() { self.count_in() } else { 0 };
                    Preview::new(cue, wait)
                });
            }
            Message::Rehearse(rehearsal) => self.begin_rehearsal(rehearsal),
            Message::NoteOn { track, midi, velocity } => {
                self.restart_lanes(track);
                each_struck(&self.table, self.count, track, now, velocity, |runner, stream, velocity| runner.note_on(midi, velocity, Articulation::PLAIN, stream, now));
            }
            Message::NoteOff { track, midi } => {
                each_midi(&self.table, self.count, track, now, |runner, stream| runner.note_off(midi, stream));
            }
            Message::Leaders { leaders, order } => {
                self.leaders = leaders.map(|leader| (usize::from(leader) < TRACKS).then_some(usize::from(leader)));
                self.midi_order = order.map(usize::from);
            }
            Message::Song(song) => {
                let old = std::mem::replace(&mut self.song, song);
                self.retire(Displaced::Song(old));
                self.seek_cursors();
                self.resettle_notes();
            }
            Message::Curves(curves) => {
                let old = std::mem::replace(&mut self.curves, curves);
                self.retire(Displaced::Curves(old));
            }
            Message::Lfos(lfos, mut lagged) => {
                carry(&mut lagged, &lfos, &self.lagged, &self.lfos);
                let old = std::mem::replace(&mut self.lfos, lfos);
                self.retire(Displaced::Lfos(old));
                let old = std::mem::replace(&mut self.lagged, lagged);
                self.retire(Displaced::Lagged(old));
            }
            Message::ReturnCurves(curves) => {
                let old = std::mem::replace(&mut self.return_curves, curves);
                self.retire(Displaced::Curves(old));
            }
            Message::ReturnLfos(lfos, mut lagged) => {
                carry(&mut lagged, &lfos, &self.return_lagged, &self.return_lfos);
                let old = std::mem::replace(&mut self.return_lfos, lfos);
                self.retire(Displaced::Lfos(old));
                let old = std::mem::replace(&mut self.return_lagged, lagged);
                self.retire(Displaced::Lagged(old));
            }
            Message::Tempo { bpm } => {
                let ratio = frames_per_beat(self.sample_rate, bpm) / self.frames_per_beat;
                for track in 0..self.count {
                    each_runner(&self.table, self.count, track, |runner| runner.retime(now, ratio));
                }
                let gained = (1.0 - 1.0 / ratio) / self.frames_per_beat;
                retime_onsets(&self.table, self.count, now, gained);
                for key in self.keys.iter_mut().flatten().filter(|key| key.frame < now) {
                    key.drift += (now - key.frame) as f64 * gained;
                }
                self.retime_rehearsal(now, ratio);
                if let Some(preview) = &mut self.preview {
                    preview.retime(ratio, self.output);
                }
                self.bpm = bpm;
                self.frames_per_beat = frames_per_beat(self.sample_rate, bpm);
                self.anchor = self.position;
                self.anchor_frame = now;
                if self.playing {
                    self.begin_segment();
                }
            }
            Message::Key { bits } => self.table.key.store(bits, Ordering::Relaxed),
            Message::Metronome { on } => self.click = on,
            Message::Measure { song } => self.measuring = Some(song),
            Message::Play { from } => {
                self.span = None;
                self.play(from);
            }
            Message::Export { from, lead, song } => {
                self.span = Some((self.frame, self.frame + song as i64));
                self.play(from);
                self.frame -= lead as i64;
                self.position = self.line().beat_at(self.frame);
            }
            Message::Place { beat } => {
                if !self.playing {
                    self.position = beat;
                    self.anchor = beat;
                    self.anchor_frame = now;
                    self.publish();
                }
            }
            Message::Stop => {
                self.release_active();
                if self.playing {
                    self.tail = Some(Outgoing::at(self.position));
                    self.rest();
                    self.playing = false;
                    self.begin_segment();
                }
            }
            Message::Loop { range } => self.range = range.filter(|(start, end)| *end > *start),
        }
    }

    fn count_in(&self) -> usize {
        let held = if self.playing { ((self.position - BEAT_EPSILON).ceil() - self.position).max(0.0) * self.frames_per_beat } else { 0.0 };
        held.round() as usize + self.output
    }

    fn play(&mut self, from: f64) {
        self.measured = self.measuring.take().map(|song| (self.frame, self.frame + song as i64));
        let cold = !self.playing;
        if cold {
            self.reset_lines();
            self.mark.forget();
        }
        for cell in self.table.tracks.iter().take(self.count) {
            restart_rows(&cell.rows, cold);
        }
        for cell in &self.table.sends {
            restart_rows(&cell.rows, cold);
        }
        restart_rows(&self.table.master, cold);
        if self.playing {
            self.tail = Some(Outgoing::at(self.position));
        }
        self.playing = true;
        self.plays = self.plays.wrapping_add(1);
        self.seek(from);
    }

    pub(super) fn insert_track(&mut self, at: usize, mut rack: Vec<Insert>, strip: Strip) {
        if self.count >= TRACKS {
            for insert in rack.drain(..) {
                self.retire(Displaced::Insert(insert));
            }
            self.reclaim_strip(strip);
            self.retire(Displaced::Rack(rack));
            return;
        }
        let at = at.min(self.count);
        shift(self.count, at, |index| self.swap_tracks(index, index + 1));
        if let Some(cell) = self.table.tracks.get(at) {
            self.place_rows(&cell.rows, &mut rack);
            match hold(&cell.strip) {
                Some(mut slot) => {
                    let old = std::mem::replace(&mut *slot, strip);
                    self.reclaim_strip(old);
                }
                None => self.reclaim_strip(strip),
            }
        }
        self.retire(Displaced::Rack(rack));
        self.count += 1;
        self.keys[at..].rotate_right(1);
        self.keys[at] = None;
        for active in &mut self.active {
            if active.track >= at {
                active.track += 1;
            }
        }
        self.retrack_rehearsal(|track| Some(track + usize::from(track >= at)));
    }

    fn place_rows(&self, rows: &Rows, inserts: &mut Vec<Insert>) {
        for (row, insert) in rows.iter().zip(inserts.drain(..)) {
            self.put(row, Some(insert));
        }
    }

    fn take_rows(&self, rows: &Rows) {
        for row in rows {
            self.put(row, None);
        }
    }

    fn put(&self, slot: &Mutex<Option<Insert>>, insert: Option<Insert>) {
        let old = match grab(slot) {
            Some(mut held) => std::mem::replace(&mut *held, insert),
            None => insert,
        };
        if let Some(old) = old {
            self.retire(Displaced::Insert(old));
        }
    }

    fn reclaim_strip(&self, strip: Strip) {
        let Strip { l, r, pre_l, pre_r, signal_l, signal_r, plays, out, pre, plan, twin, .. } = strip;
        for buffer in [l, r, pre_l, pre_r, signal_l, signal_r] {
            if buffer.capacity() > 0 {
                self.retire(Displaced::Buffer(buffer));
            }
        }
        if plays.capacity() > 0 {
            self.retire(Displaced::Plays(plays));
        }
        for line in [out, pre] {
            if line.len() > 0 {
                self.retire(Displaced::Line(line));
            }
        }
        if let Some(plan) = plan {
            self.retire(Displaced::Track(plan));
        }
        if let Some(twin) = twin {
            self.retire(Displaced::TwinLines(twin));
        }
    }

    fn twin_track(&self, track: usize, lines: Option<Box<TwinLines>>, mut units: Vec<Option<Box<Twin>>>) {
        let cell = self.table.tracks.get(track).filter(|_| track < self.count);
        for (at, unit) in units.iter_mut().enumerate() {
            let spare = match cell.and_then(|cell| cell.rows.get(at)).and_then(grab).as_deref_mut() {
                Some(Some(Insert::Effect { twin, .. })) => std::mem::replace(twin, unit.take()),
                _ => unit.take(),
            };
            if let Some(spare) = spare {
                self.retire(Displaced::TwinUnit(spare));
            }
        }
        self.retire(Displaced::TwinUnits(units));
        let spare = match cell.and_then(|cell| hold(&cell.strip)) {
            Some(mut strip) => {
                if let Some(plan) = strip.plan.as_deref_mut() {
                    plan.clear_twin();
                }
                std::mem::replace(&mut strip.twin, lines)
            }
            None => lines,
        };
        if let Some(spare) = spare {
            self.retire(Displaced::TwinLines(spare));
        }
    }

    fn clear_cell(&self, cell: &TrackCell) {
        self.take_rows(&cell.rows);
        if let Some(mut strip) = hold(&cell.strip) {
            let old = std::mem::replace(&mut *strip, Strip::empty());
            self.reclaim_strip(old);
        }
    }

    pub(super) fn swap_tracks(&self, a: usize, b: usize) {
        let (Some(cell_a), Some(cell_b)) = (self.table.tracks.get(a), self.table.tracks.get(b)) else {
            return;
        };
        swap_rows(&cell_a.rows, &cell_b.rows);
        if let Some(mut strip_a) = hold(&cell_a.strip)
            && let Some(mut strip_b) = hold(&cell_b.strip)
        {
            std::mem::swap(&mut *strip_a, &mut *strip_b);
        }
    }

    pub(super) fn remove_track(&mut self, at: usize) {
        if at >= self.count {
            return;
        }
        if let Some(cell) = self.table.tracks.get(at) {
            self.clear_cell(cell);
        }
        shift(at, self.count - 1, |index| self.swap_tracks(index, index + 1));
        self.count -= 1;
        self.keys[at..].rotate_left(1);
        self.keys[TRACKS - 1] = None;
        self.active.retain(|active| active.track != at);
        for active in &mut self.active {
            if active.track > at {
                active.track -= 1;
            }
        }
        self.retrack_rehearsal(|track| if track == at { None } else { Some(track - usize::from(track > at)) });
    }

    pub(super) fn move_tracks(&mut self, from: usize, to: usize) {
        if from >= self.count || to >= self.count {
            return;
        }
        shift(from, to, |index| self.swap_tracks(index, index + 1));
        if from < to {
            self.keys[from..=to].rotate_left(1);
        } else {
            self.keys[to..=from].rotate_right(1);
        }
        for active in &mut self.active {
            active.track = moved_index(active.track, from, to);
        }
        self.retrack_rehearsal(|track| Some(moved_index(track, from, to)));
    }

    pub(super) fn clear_all(&mut self) {
        for cell in self.table.tracks.iter().take(self.count) {
            self.clear_cell(cell);
        }
        self.count = 0;
        for cell in &self.table.sends {
            self.take_rows(&cell.rows);
        }
        self.take_rows(&self.table.master);
        self.active.clear();
        self.keys = [None; TRACKS];
        self.end_rehearsal();
    }

    fn row(&self, host: Host, index: usize) -> Option<MutexGuard<'_, Option<Insert>>> {
        self.table.rows(host).and_then(|rows| rows.get(index)).and_then(grab)
    }

    pub(super) fn set_row(&self, host: Host, index: usize, row: Row) {
        let stale = self.row(host, index).and_then(|mut slot| slot.as_mut().map(|insert| insert.set(row, self.sample_rate)));
        for vision in stale.into_iter().flatten().flatten() {
            self.retire(Displaced::Vision(vision));
        }
    }

    fn renew_row(&self, host: Host, index: usize, mut insert: Insert) {
        let stale = self.row(host, index).and_then(|mut slot| slot.as_mut().map(|held| held.renew(&mut insert, self.sample_rate)));
        for vision in stale.into_iter().flatten().flatten() {
            self.retire(Displaced::Vision(vision));
        }
        self.retire(Displaced::Insert(insert));
    }

    fn land(&self, host: Host, index: usize, mut vision: Box<Vision>, twin: bool) {
        let spare = match self.row(host, index).as_deref_mut() {
            Some(Some(Insert::Effect { slot, twin: Some(double), .. })) if twin && slot.reverse_band().is_some() => double.vision.replace(vision),
            Some(Some(Insert::Effect { slot, vision: mine, .. })) if !twin && slot.reverse_band().is_some() => {
                if let Some(old) = mine.as_deref() {
                    vision.resume(old);
                }
                mine.replace(vision)
            }
            _ => Some(vision),
        };
        if let Some(vision) = spare {
            self.retire(Displaced::Vision(vision));
        }
    }

    fn forget(&self, host: Host, index: usize) {
        let stale = match self.row(host, index).as_deref_mut() {
            Some(Some(Insert::Effect { vision, twin, .. })) => [vision.take(), twin.as_deref_mut().and_then(|twin| twin.vision.take())],
            _ => [None, None],
        };
        for vision in stale.into_iter().flatten() {
            self.retire(Displaced::Vision(vision));
        }
    }

    pub(crate) fn capture(&self, host: Host, index: usize, frames: usize) {
        if let Some(Some(Insert::Effect { capture, .. })) = self.row(host, index).as_deref_mut() {
            *capture = Some(Capture::new(frames));
        }
    }

    pub(crate) fn take_capture(&self, host: Host, index: usize) -> Option<Capture> {
        match self.row(host, index).as_deref_mut() {
            Some(Some(Insert::Effect { capture, .. })) => capture.take(),
            _ => None,
        }
    }

    pub(super) fn insert_row(&self, host: Host, index: usize, mut insert: Insert, joins: bool) {
        insert.wake(self.sample_rate);
        let Some(rows) = self.table.rows(host) else {
            self.retire(Displaced::Insert(insert));
            return;
        };
        let len = Table::len(rows);
        if len >= RACK_ROWS {
            self.retire(Displaced::Insert(insert));
            return;
        }
        let index = index.min(len);
        let notes = takes_notes(&insert);
        let joined = joins && index > 0 && join(&rows[index - 1], &mut insert);
        shift(len, index, |at| swap_slots(&rows[at], &rows[at + 1]));
        self.put(&rows[index], Some(insert));
        self.reshape(host, |plan| plan.inserted(index));
        if notes && !joined {
            release_rows(rows);
        }
    }

    fn reshape(&self, host: Host, change: impl FnOnce(&mut TrackPlan)) {
        if let Some(track) = host.track()
            && let Some(cell) = self.table.tracks.get(track)
            && let Some(mut strip) = hold(&cell.strip)
            && let Some(plan) = strip.plan.as_deref_mut()
        {
            change(plan);
        }
    }

    pub(super) fn remove_row(&self, host: Host, index: usize) {
        let Some(rows) = self.table.rows(host) else {
            return;
        };
        let len = Table::len(rows);
        if index >= len {
            return;
        }
        let notes = grab(&rows[index]).is_some_and(|row| row.as_ref().is_some_and(|insert| takes_notes(insert) && !left(insert)));
        self.put(&rows[index], None);
        shift(index, len - 1, |at| swap_slots(&rows[at], &rows[at + 1]));
        self.reshape(host, |plan| plan.removed(index));
        if notes {
            release_rows(rows);
        }
    }

    pub(super) fn move_row(&self, host: Host, from: usize, to: usize) {
        let Some(rows) = self.table.rows(host) else {
            return;
        };
        let len = Table::len(rows);
        if from >= len || to >= len {
            return;
        }
        let notes = grab(&rows[from]).is_some_and(|row| row.as_ref().is_some_and(takes_notes));
        shift(from, to, |at| swap_slots(&rows[at], &rows[at + 1]));
        self.reshape(host, |plan| plan.moved(from, to));
        if notes {
            release_rows(rows);
        }
    }
}

fn takes_notes(insert: &Insert) -> bool {
    matches!(insert, Insert::Midi { .. } | Insert::Core { .. })
}

fn join(above: &Mutex<Option<Insert>>, insert: &mut Insert) -> bool {
    match (grab(above).as_deref(), insert) {
        (Some(Some(Insert::Core { .. })), Insert::Core { .. }) => true,
        (Some(Some(Insert::Midi { stream: fed, .. })), Insert::Midi { runner, stream, .. }) => {
            runner.join(fed.sounding());
            stream.join(fed.sounding());
            true
        }
        _ => false,
    }
}

fn left(insert: &Insert) -> bool {
    match insert {
        Insert::Core { layer, .. } => layer.leaving(),
        Insert::Midi { row, runner, stream } => row.leaving() && runner.idle(stream.sounding()),
        Insert::Effect { .. } | Insert::Bus { .. } => false,
    }
}

fn release_rows(rows: &Rows) {
    for row in rows {
        let Some(mut row) = grab(row) else {
            continue;
        };
        match row.as_mut() {
            Some(Insert::Midi { runner, stream, .. }) => runner.release(stream.as_mut()),
            Some(Insert::Core { core, .. }) => core.release_all(),
            Some(Insert::Effect { .. } | Insert::Bus { .. }) => {}
            None => break,
        }
    }
}

fn swap_slots(a: &Mutex<Option<Insert>>, b: &Mutex<Option<Insert>>) {
    if let Some(mut a) = grab(a)
        && let Some(mut b) = grab(b)
    {
        std::mem::swap(&mut *a, &mut *b);
    }
}

fn swap_rows(a: &Rows, b: &Rows) {
    for (row_a, row_b) in a.iter().zip(b) {
        swap_slots(row_a, row_b);
    }
}

fn shift(from: usize, to: usize, mut swap: impl FnMut(usize)) {
    if from < to {
        for at in from..to {
            swap(at);
        }
    } else {
        for at in (to..from).rev() {
            swap(at);
        }
    }
}

fn carry(fresh: &mut Lagged, lfos: &Lfos, held: &Lagged, known: &Lfos) {
    for ((fresh, lfos), (held, known)) in fresh.iter_mut().zip(lfos).zip(held.iter().zip(known)) {
        for (slot, lfo) in fresh.iter_mut().zip(lfos) {
            if let Some(at) = known.iter().position(|old| old.aim == lfo.aim) {
                *slot = held.get(at).copied().flatten();
            }
        }
    }
}
