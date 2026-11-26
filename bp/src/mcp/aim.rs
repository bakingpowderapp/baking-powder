use crate::project::codec::{TRACK_GAIN, Units, curve_to_json, fader_position, inside_of, lfo_to_json};
use crate::flatten::Sent;
use crate::project::{Aim, Curve, Host, LaneLfo, Project, RACK_ROWS, Row, SEND_COUNT, beats};
use sdk::json::{Json, tidy};

const SAMPLED: &str = "path is a knob, gain or send: tracks/T/rack/R/key, returns/N/rack/R/key or master/rack/R/key, a gain, or sendLevels/S";

fn aimed(project: &Project, steps: &[String]) -> Result<(Host, Aim, Units, f64), String> {
    let (host, rest) = match steps {
        [tracks, track, rest @ ..] if tracks == "tracks" => (Host::Track(index_of(track, project.tracks.len()).ok_or_else(|| format!("There is no tracks/{track}: the song has {}", super::counted(project.tracks.len(), "track")))?), rest),
        [returns, send, rest @ ..] if returns == "returns" => (Host::Send(index_of(send, SEND_COUNT).ok_or(SAMPLED)?), rest),
        [master, rest @ ..] if master == "master" => (Host::Master, rest),
        _ => return Err(String::from(SAMPLED)),
    };
    let (aim, units, base) = match rest {
        [gain] if gain == TRACK_GAIN => (Aim::Gain, Units::Gain, fader_position(f64::from(project.gain_of(host)))),
        [levels, send] if levels == "sendLevels" => {
            let (Host::Track(track), Some(send)) = (host, index_of(send, SEND_COUNT)) else {
                return Err(String::from(SAMPLED));
            };
            (Aim::Send(send), Units::Share, f64::from(project.tracks[track].sends[send]))
        }
        [rack, row, key @ ..] if rack == "rack" && !key.is_empty() => {
            let row = index_of(row, RACK_ROWS).ok_or(SAMPLED)?;
            let target = project.rack(host).get(row).ok_or_else(|| format!("The rack has no row {row}"))?;
            let key = dotted(key);
            if let Some(mixed) = target.mix_aim(row).filter(|_| key == target.mix_key()) {
                (mixed, Units::Share, target.mix())
            } else {
                let knob = target.knobs().iter().find(|knob| knob.path == key).ok_or_else(|| format!("{key} is not a key a lane moves; the row's manual lists lanes"))?;
                let travel = target.travel_of(knob.band, knob.name).ok_or(SAMPLED)?;
                (Aim::knob(row, knob.band, knob.name), Units::Knob(knob), travel)
            }
        }
        _ => return Err(String::from(SAMPLED)),
    };
    Ok((host, aim, units, base))
}

pub(crate) fn sampled(project: &Project, sent: &Sent, steps: &[String], beats: &[f64]) -> Result<Vec<f64>, String> {
    let (host, aim, units, base) = aimed(project, steps)?;
    let automation = project.automation(host);
    let curves = &automation.curves;
    let direct = automation.curve(&aim);
    let mut lfos: Vec<LaneLfo> = automation.lfos.iter().filter(|lfo| lfo.amount > 0.0 && lfo.aim == aim).cloned().collect();
    if lfos.is_empty() && curves.iter().any(|curve| curve.lfo.is_some() && curve.aim == aim) {
        lfos.push(automation.lfos.iter().find(|lfo| lfo.aim == aim).cloned().unwrap_or_else(|| LaneLfo::blank(aim.clone())));
    }
    let target = project.rack(host).get(aim.row_or_top());
    let lives = match &aim {
        Aim::Knob { band, name, .. } => target.is_some_and(|target| target.automatable(band, name)),
        Aim::Level(_) | Aim::Wet(_) => target.is_some(),
        Aim::Gain | Aim::Send(_) => true,
    };
    lfos.retain(|lfo| lfo.audio_rate().is_none() && lives);
    let share = target.map_or(1.0, |target| target.lane_share(&aim));
    let notes = host.track().map_or_else(Vec::new, |track| sent.heard(project, track, &aim));
    let summed = host.track().is_some();
    let waved = |beat: f64| {
        let (first, last) = (lfos.first()?, lfos.last()?);
        let swing: f64 = if summed { lfos.iter().map(|lfo| lfo.swing(curves, &notes, beat, project.bpm, share)).sum() } else { last.swing(curves, &notes, beat, project.bpm, share) };
        let centre = if summed { first } else { last }.sweep(curves, beat, share).centre();
        Some((centre + swing).clamp(0.0, 1.0))
    };
    Ok(beats.iter().map(|&beat| units.value(waved(beat).unwrap_or_else(|| direct.map_or(base, |curve| curve.at(beat))))).collect())
}

pub(super) fn sources(document: &Json, steps: &[String], (from, until): (f64, f64)) -> Result<Vec<(&'static str, Json)>, String> {
    let project = Project::from_json(document).ok_or("The document would not read as a project")?;
    let (host, aim, units, _) = aimed(&project, steps)?;
    let automation = project.automation(host);
    let owner = match host {
        Host::Track(track) => format!("tracks/{track}"),
        Host::Send(send) => format!("returns/{send}"),
        Host::Master => String::from("master"),
    };
    let rows = |row: usize| project.rack(host).get(row).copied();
    let curves: Vec<&Curve> = automation.curves.iter().filter(|curve| curve_to_json(curve, &rows).is_some()).collect();
    let mut lanes = Vec::new();
    for (index, curve) in curves.iter().enumerate().filter(|(_, curve)| curve.aim == aim) {
        let at = format!("{owner}/curves/{index}");
        lanes.push(match curve.lfo {
            Some(field) => source_at(at, vec![("moves", Json::from(field.name()))]),
            None => {
                let heard: Vec<f64> = [from, until].into_iter().chain(curve.nodes.iter().map(|node| beats(node.tick)).filter(|beat| (from..until).contains(beat))).map(|beat| units.value(curve.at(beat))).collect();
                let (min, max) = heard.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), &value| (min.min(value), max.max(value)));
                source_at(at, vec![("nodes", Json::Number(curve.nodes.len() as f64)), ("min", Json::Number(tidy(min))), ("max", Json::Number(tidy(max)))])
            }
        });
    }
    for (index, lfo) in automation.lfos.iter().filter(|lfo| lfo_to_json(lfo, &rows).is_some()).enumerate().filter(|(_, lfo)| lfo.aim == aim) {
        lanes.push(source_at(format!("{owner}/lfos/{index}"), vec![("shape", Json::from(lfo.shape.name())), ("amount", Json::Number(lfo.amount)), ("clock", Json::from(lfo.clock.name()))]));
    }
    let inside = match (host.track(), &aim, project.rack(host).get(aim.row_or_top())) {
        (Some(track), Aim::Knob { row, band, name }, Some(Row::Core(layer))) => inside_of(&layer.patch, band, name).into_iter().map(|(key, source, amount)| source_at(format!("tracks/{track}/rack/{row}/{key}"), vec![("source", Json::from(source)), ("amount", Json::Number(amount))])).collect(),
        _ => Vec::new(),
    };
    Ok([("sources", lanes), ("inside", inside)].into_iter().filter(|(_, list)| !list.is_empty()).map(|(key, list)| (key, Json::Array(list))).collect())
}

fn source_at(path: String, fields: Vec<(&str, Json)>) -> Json {
    Json::Object(std::iter::once((String::from("path"), Json::String(path))).chain(fields.into_iter().map(|(key, value)| (String::from(key), value))).collect())
}

fn index_of(step: &str, most: usize) -> Option<usize> {
    step.parse().ok().filter(|index| *index < most)
}

fn dotted(steps: &[String]) -> String {
    steps.iter().fold(String::new(), |mut path, step| {
        match step.parse::<usize>() {
            Ok(index) => path.push_str(&format!("[{index}]")),
            Err(_) => {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(step);
            }
        }
        path
    })
}
