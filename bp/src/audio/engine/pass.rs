use super::*;

fn pack_gauge(level: f32, signal: f32) -> u64 {
    (u64::from(level.to_bits()) << 32) | u64::from(signal.to_bits())
}

fn step_gauge(playing: Option<i64>) -> u64 {
    pack_gauge(0.0, playing.map_or(0.0, |step| (step + 1) as f32))
}

fn meter_byte(db: f32) -> u64 {
    u64::from((db.clamp(0.0, BAND_METER_DB) / BAND_METER_DB * f32::from(u8::MAX)).round() as u8)
}

fn pack_strips(strips: [[f32; 2]; FREAK_BANDS]) -> u64 {
    strips.iter().fold(0, |bits, [reduction, level]| (bits << 16) | (meter_byte(*reduction) << 8) | meter_byte(level + BAND_METER_DB))
}

pub(super) struct Pass<'a> {
    pub(super) meters: &'a Meters,
    pub(super) d: &'a Dispatch,
    pub(super) host: Host,
    pub(super) solo: Option<Alone>,
    pub(super) segments: &'a Segments,
    pub(super) measured: Option<(i64, i64)>,
}

const REST_CURVE: [f32; sdk::spectrum::CURVE] = [0.0; sdk::spectrum::CURVE];

fn apply(pass: &Pass, insert: &mut Insert, position: usize, l: &mut [f32], r: &mut [f32], stamped: bool, upstream: usize) {
    let n = l.len();
    let gauges = pass.meters.gauges(pass.host);
    let gauge = &gauges.rows[position];
    let d = pass.d;
    match insert {
        Insert::Midi { runner, .. } => gauge.store(step_gauge(runner.playing()), Ordering::Relaxed),
        Insert::Core { layer, core, stream, l: core_l, r: core_r, pass: rendered } => {
            if !stamped {
                stream.play(core.as_mut(), lane_of(layer, pass.host.track().unwrap_or(TRACKS) * RACK_ROWS + position), layer.leaving(), &mut core_l[..n], &mut core_r[..n], d);
                *rendered = d.generation;
            }
            gauge.store(step_gauge(None), Ordering::Relaxed);
        }
        Insert::Bus { master, .. } => gauges.restart(position, *master),
        Insert::Effect { slot, unit, vision, capture, warm, .. } => {
            if let Some(capture) = capture {
                capture.record(l, r);
            }
            let heard = !slot.bypass && pass.solo.is_none_or(|solo| solo.effects);
            let tap = tap_for(pass.meters, &d.targets, pass.host.insert(position));
            if let Some(tap) = tap {
                tap.write_pre(l, r);
            }
            if heard {
                let clock = d.behind(upstream, pass.segments);
                match pass.measured.filter(|_| slot.kind().meters_reduction()) {
                    Some((from, to)) => {
                        let edge = |frame: i64| (frame + upstream as i64 - d.now).clamp(0, n as i64) as usize;
                        measure(unit, slot, l, r, &clock, (edge(from), edge(to)), |level| gauges.hold(position, level));
                    }
                    None => unit.process(l, r, slot, &clock),
                }
                if let Some((vision, beat)) = vision.as_deref_mut().zip(clock.beat.map(|beat| beat.at(0))) {
                    vision.lay(beat, l, r, (slot.wet * slot.reverse_level()) as f32);
                }
                warm.apply(l, r);
            }
            if let Some(tap) = tap {
                tap.write_post(l, r);
                if let Some(curve) = unit.curve() {
                    tap.write_curve(if heard { curve } else { &REST_CURVE });
                }
            }
            let level = if heard { unit.gauge() } else { 0.0 };
            gauge.store(if heard { pack_gauge(level, unit.signal()) } else { pack_gauge(0.0, f32::NEG_INFINITY) }, Ordering::Relaxed);
            if pass.measured.is_none() && slot.kind().meters_reduction() {
                gauges.hold(position, level);
            }
            let landing = |frame: i64| (frame + upstream as i64 - d.now).clamp(0, n as i64);
            if matches!(slot.kind(), fx::Kind::Utility) && pass.measured.is_some_and(|(from, to)| landing(to) > landing(from)) {
                gauges.hold(position, level);
            }
            gauges.strips[position].store(if heard { pack_strips(unit.strips()) } else { 0 }, Ordering::Relaxed);
        }
    }
}

fn measure(unit: &mut Effect, slot: &fx::Slot, l: &mut [f32], r: &mut [f32], clock: &Clock, (from, to): (usize, usize), mut hold: impl FnMut(f32)) {
    for (start, end) in [(0, from), (from, to), (to, l.len())] {
        if start == end {
            continue;
        }
        let clock = Clock { beat: clock.beat.map(|beat| Beat { since: beat.since + start as i64, ..beat }), ..*clock };
        unit.process(&mut l[start..end], &mut r[start..end], slot, &clock);
        if (start, end) == (from, to) {
            hold(unit.gauge());
        }
    }
}

pub(super) fn run_twin(rows: &Rows, d: &Dispatch, segments: &Segments, measured: Option<(i64, i64)>, l: &mut [f32], r: &mut [f32], (aside_l, aside_r): (&mut [f32], &mut [f32]), mut plan: Option<&mut TrackPlan>) -> bool {
    let n = l.len();
    let (mut split, mut whole) = (None, true);
    for (position, row) in rows.iter().enumerate() {
        let Some(mut row) = grab(row) else {
            continue;
        };
        let Some(insert) = row.as_mut() else {
            break;
        };
        let mut planned = plan.as_deref_mut().and_then(|plan| plan.rows.get_mut(position));
        let upstream = planned.as_ref().map_or(0, |planned| planned.upstream);
        if let Some(planned) = planned.as_deref_mut() {
            planned.twin_before.apply(l, r);
        }
        let fold = planned.and_then(|planned| planned.folds.iter_mut().find(|fold| fold.core == position));
        match insert {
            Insert::Effect { slot, vision, twin, .. } => {
                let Some(twin) = twin.as_deref_mut() else {
                    whole = false;
                    continue;
                };
                if slot.bypass {
                    continue;
                }
                let clock = d.behind(upstream, segments);
                match measured.filter(|_| slot.kind().meters_reduction()) {
                    Some((from, to)) => {
                        let edge = |frame: i64| (frame + upstream as i64 - d.now).clamp(0, n as i64) as usize;
                        measure(&mut twin.unit, slot, l, r, &clock, (edge(from), edge(to)), |_| {});
                    }
                    None => twin.unit.process(l, r, slot, &clock),
                }
                if let Some((vision, beat)) = twin.vision.as_deref().or(vision.as_deref()).zip(clock.beat.map(|beat| beat.at(0))) {
                    vision.lay_with(&mut twin.level, beat, l, r, (slot.wet * slot.reverse_level()) as f32);
                }
                twin.warm.apply(l, r);
            }
            Insert::Core { layer, l: core_l, r: core_r, pass, .. } => {
                if let Some(fold) = fold {
                    fold.twin_sum.apply(l, r);
                }
                if *pass == d.generation {
                    pour_core(layer, core_l, core_r, l, r);
                }
            }
            Insert::Bus { levels, .. } if split.is_none() => {
                if let Some(fold) = fold {
                    fold.twin_lane.apply(l, r);
                }
                for i in 0..n {
                    aside_l[i] = l[i] * levels[0];
                    aside_r[i] = r[i] * levels[0];
                }
                l.fill(0.0);
                r.fill(0.0);
                split = Some(position);
            }
            Insert::Midi { .. } | Insert::Bus { .. } => {}
        }
    }
    let Some(position) = split else {
        return whole;
    };
    if let Some(fold) = plan.and_then(|plan| plan.rows.get_mut(position)).and_then(|planned| planned.folds.iter_mut().find(|fold| fold.core == position)) {
        fold.twin_sum.apply(l, r);
    }
    if let Some(row) = grab(&rows[position])
        && let Some(Insert::Bus { levels, .. }) = row.as_ref()
    {
        for i in 0..n {
            l[i] = l[i] * levels[1] + aside_l[i];
            r[i] = r[i] * levels[1] + aside_r[i];
        }
    }
    whole
}

fn pour(pass: &Pass, insert: &Insert, position: usize, l: &mut [f32], r: &mut [f32]) {
    let Insert::Core { layer, l: core_l, r: core_r, pass: rendered, .. } = insert else {
        return;
    };
    if *rendered != pass.d.generation || pass.solo.is_some_and(|solo| !solo.holds(position)) {
        return;
    }
    pour_core(layer, core_l, core_r, l, r);
}

pub(in crate::audio) fn run_rows(rows: &Rows, host: Host, l: &mut [f32], r: &mut [f32], meters: &Meters, d: &Dispatch, lags: &Lags, segments: &Segments, measured: Option<(i64, i64)>) {
    let pass = Pass { meters, d, host, solo: None, segments, measured };
    for (position, (row, lag)) in rows.iter().zip(lags).enumerate() {
        let Some(mut row) = grab(row) else {
            continue;
        };
        let Some(insert) = row.as_mut() else {
            break;
        };
        apply(&pass, insert, position, l, r, false, lag.upstream);
        pour(&pass, insert, position, l, r);
    }
}

fn lingers(insert: &Insert, n: usize) -> bool {
    match insert {
        Insert::Core { layer, core, l, r, .. } => layer.leaving() && (core.active() || l[..n].iter().chain(&r[..n]).any(|sample| sample.abs() > QUIET)),
        Insert::Midi { row, runner, stream } => row.leaving() && !runner.idle(stream.sounding()),
        Insert::Effect { .. } | Insert::Bus { .. } => false,
    }
}

pub(super) fn run_routed(rows: &Rows, pass: &Pass, l: &mut [f32], r: &mut [f32], mut plan: Option<&mut TrackPlan>) {
    let n = l.len();
    let mut split = None;
    let mut sounding = 0;
    for (position, row) in rows.iter().enumerate() {
        let Some(mut row) = grab(row) else {
            continue;
        };
        let Some(insert) = row.as_mut() else {
            break;
        };
        let mut planned = plan.as_deref_mut().and_then(|plan| plan.rows.get_mut(position));
        let upstream = planned.as_ref().map_or(0, |planned| planned.upstream);
        if let Some(planned) = planned.as_deref_mut() {
            planned.before.apply(l, r);
        }
        apply(pass, insert, position, l, r, true, upstream);
        sounding |= u64::from(lingers(insert, n)) << position;
        let fold = planned.and_then(|planned| planned.folds.iter_mut().find(|fold| fold.core == position));
        match insert {
            Insert::Core { .. } => {
                if let Some(fold) = fold {
                    delay_lane(insert, n, &mut fold.lane);
                    fold.sum.apply(l, r);
                }
                pour(pass, insert, position, l, r);
            }
            Insert::Bus { levels, l: aside_l, r: aside_r, .. } if split.is_none() && aside_l.len() >= n => {
                if let Some(fold) = fold {
                    fold.lane.apply(l, r);
                }
                for i in 0..n {
                    aside_l[i] = l[i] * levels[0];
                    aside_r[i] = r[i] * levels[0];
                }
                l.fill(0.0);
                r.fill(0.0);
                split = Some(position);
            }
            Insert::Midi { .. } | Insert::Effect { .. } | Insert::Bus { .. } => {}
        }
    }
    pass.meters.gauges(pass.host).linger(sounding, pass.d.batches);
    let Some(position) = split else {
        return;
    };
    if let Some(fold) = plan.and_then(|plan| plan.rows.get_mut(position)).and_then(|planned| planned.folds.iter_mut().find(|fold| fold.core == position)) {
        fold.sum.apply(l, r);
    }
    if let Some(row) = grab(&rows[position])
        && let Some(Insert::Bus { levels, l: aside_l, r: aside_r, .. }) = row.as_ref()
    {
        for i in 0..n {
            l[i] = l[i] * levels[1] + aside_l[i];
            r[i] = r[i] * levels[1] + aside_r[i];
        }
    }
}

fn delay_lane(insert: &mut Insert, n: usize, delay: &mut Delay) {
    if let Insert::Core { l, r, .. } = insert {
        delay.apply(&mut l[..n], &mut r[..n]);
    }
}

fn each_runner_of(rows: &Rows, f: &mut impl FnMut(usize, &MidiRow, &mut Runner, &mut Stream)) {
    for (position, row) in rows.iter().enumerate() {
        let Some(mut row) = grab(row) else {
            continue;
        };
        match row.as_mut() {
            Some(Insert::Midi { row, runner, stream }) => f(position, row, runner.as_mut(), stream.as_mut()),
            Some(Insert::Core { .. } | Insert::Effect { .. } | Insert::Bus { .. }) => {}
            None => break,
        }
    }
}

pub(in crate::audio) fn each_struck(table: &Table, count: usize, track: usize, now: i64, velocity: u8, mut f: impl FnMut(&mut Runner, &mut Stream, u8)) {
    if track >= count {
        return;
    }
    if let Some(cell) = table.tracks.get(track) {
        let key = table.key.load(Ordering::Relaxed);
        each_runner_of(&cell.rows, &mut |position, row, runner, stream| {
            if position == 0 {
                stream.now = now;
                runner.snap_to(row.snap(key));
                runner.transpose_by(row.shift());
                f(runner, stream, row.struck(velocity));
            }
        });
    }
}

pub(in crate::audio) fn each_midi(table: &Table, count: usize, track: usize, now: i64, mut f: impl FnMut(&mut Runner, &mut Stream)) {
    each_struck(table, count, track, now, 0, |runner, stream, _| f(runner, stream));
}

pub(in crate::audio) fn forget_onsets(table: &Table, count: usize) {
    for cell in table.tracks.iter().take(count) {
        each_runner_of(&cell.rows, &mut |_, _, _, stream| stream.onsets_mut().forget());
    }
}

pub(in crate::audio) fn retime_onsets(table: &Table, count: usize, now: i64, gained: f64) {
    for cell in table.tracks.iter().take(count) {
        each_runner_of(&cell.rows, &mut |_, _, _, stream| stream.onsets_mut().retime(now, gained));
    }
}

pub(in crate::audio) fn each_runner(table: &Table, count: usize, track: usize, mut f: impl FnMut(&mut Runner)) {
    if track >= count {
        return;
    }
    if let Some(cell) = table.tracks.get(track) {
        each_runner_of(&cell.rows, &mut |_, _, runner, _| f(runner));
    }
}

pub(in crate::audio) fn restart_rows(rows: &Rows, cold: bool) {
    for row in rows {
        if let Some(mut row) = grab(row)
            && let Some(Insert::Effect { unit, warm, twin, .. }) = row.as_mut()
        {
            unit.restart();
            if let Some(twin) = twin.as_deref_mut() {
                twin.unit.restart();
            }
            if cold {
                *warm = Warm::SETTLED;
                if let Some(twin) = twin.as_deref_mut() {
                    twin.warm = Warm::SETTLED;
                }
            }
        }
    }
}
