use sdk::draw::{DrawList, Rect, Tone};
use sdk::dsp::svf::{self, BiquadCoeffs};
use sdk::face::{LABEL_SCALE, WHEEL_PX};
use sdk::font;
use crate::eq;
use crate::face::soloed;
use crate::params::{CURVE_HI_HZ, CURVE_LO_HZ, Channel, Eq, EqBand};
use sdk::spectrum::{self, Spectrum};
use super::{DASH, DASH_PERIOD, Drag, GRAB, LABEL_PAD, Picture};
use std::f64::consts::TAU;

pub(super) const CURVE_FS: f64 = 48_000.0;
const PASS_FLOOR: f64 = 1e-9;
const CURVE_MIN_DB: f64 = -36.0;
const CURVE_MAX_DB: f64 = 18.0;
pub(super) const HANDLE: i32 = 11;
const Q_PER_PIXEL: f64 = 200.0;
const GRID_DB: [f64; 7] = [-24.0, -18.0, -12.0, -6.0, 0.0, 6.0, 12.0];
pub(super) const GRID_HZ: [(f64, &str); 3] = [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")];

struct Response {
    coeffs: BiquadCoeffs,
    weight: f64,
    channel: Channel,
}

fn responses(eq: &Eq) -> Vec<Response> {
    eq.bands.iter().filter(|band| band.on).map(|band| Response { coeffs: eq::design(band, band.gain_db * eq.scale, CURVE_FS), weight: if band.shape.is_plus() { 4.0 } else { 1.0 }, channel: band.channel }).collect()
}

fn passes(eq: &Eq) -> Vec<(BiquadCoeffs, f64)> {
    eq.pass.bands.iter().filter(|band| eq.pass.on && band.level > 0.0).map(|band| (svf::band_pass(band.hz, band.q, CURVE_FS), band.level * band.q)).collect()
}

fn turn(c: &BiquadCoeffs, hz: f64) -> (f64, f64) {
    let w = TAU * hz / CURVE_FS;
    let (cos1, sin1, cos2, sin2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    let (top_re, top_im) = (c.b0 + c.b1 * cos1 + c.b2 * cos2, -c.b1 * sin1 - c.b2 * sin2);
    let (low_re, low_im) = (1.0 + c.a1 * cos1 + c.a2 * cos2, -c.a1 * sin1 - c.a2 * sin2);
    let size = low_re * low_re + low_im * low_im;
    ((top_re * low_re + top_im * low_im) / size, (top_im * low_re - top_re * low_im) / size)
}

fn pass_db(passes: &[(BiquadCoeffs, f64)], trim_db: f64, hz: f64) -> f64 {
    let (re, im) = passes.iter().fold((0.0, 0.0), |(re, im), (coeffs, gain)| {
        let (band_re, band_im) = turn(coeffs, hz);
        (re + gain * band_re, im + gain * band_im)
    });
    trim_db + 20.0 * re.hypot(im).max(PASS_FLOOR).log10()
}

fn lane_db(responses: &[Response], hz: f64, lane: Channel) -> f64 {
    responses.iter().filter(|r| !(lane == Channel::Mid && r.channel == Channel::Side) && !(lane == Channel::Side && r.channel == Channel::Mid)).map(|r| r.coeffs.magnitude_db(hz, CURVE_FS) * r.weight).sum()
}

pub(super) fn x_of_hz(hz: f64, w: i32) -> i32 {
    let t = (hz.clamp(CURVE_LO_HZ, CURVE_HI_HZ).log10() - CURVE_LO_HZ.log10()) / (CURVE_HI_HZ.log10() - CURVE_LO_HZ.log10());
    ((t * f64::from(w - 1)).round() as i32).clamp(0, w - 1)
}

pub(super) fn hz_of_x(x: i32, w: i32) -> f64 {
    let t = f64::from(x.clamp(0, w - 1)) / f64::from((w - 1).max(1));
    10f64.powf(CURVE_LO_HZ.log10() + t * (CURVE_HI_HZ.log10() - CURVE_LO_HZ.log10()))
}

pub(super) fn y_of_db(db: f64, h: i32) -> i32 {
    let t = ((db - CURVE_MIN_DB) / (CURVE_MAX_DB - CURVE_MIN_DB)).clamp(0.0, 1.0);
    (((1.0 - t) * f64::from(h - 1)).round() as i32).clamp(0, h - 1)
}

pub(super) fn db_of_y(y: i32, h: i32) -> f64 {
    let t = (1.0 - f64::from(y) / f64::from((h - 1).max(1))).clamp(0.0, 1.0);
    CURVE_MIN_DB + t * (CURVE_MAX_DB - CURVE_MIN_DB)
}

fn q_to_db(q: f64, max_q: f64) -> f64 {
    let t = (q.clamp(0.1, max_q).log10() - 0.1f64.log10()) / (max_q.log10() - 0.1f64.log10());
    -6.0 + t * 18.0
}

fn db_to_q(db: f64, max_q: f64) -> f64 {
    let t = ((db + 6.0) / 18.0).clamp(0.0, 1.0);
    10f64.powf(0.1f64.log10() + t * (max_q.log10() - 0.1f64.log10()))
}

fn handle_db(band: &EqBand, scale: f64) -> f64 {
    if band.shape.has_gain() { band.gain_db * scale } else { q_to_db(band.q, band.shape.max_q()) }
}

pub(super) fn handle_at(area: Rect, hz: f64, db: f64) -> Rect {
    let cx = area.x + x_of_hz(hz, area.w);
    let cy = area.y + y_of_db(db, area.h);
    let half = HANDLE / 2;
    Rect::new((cx - half).clamp(area.x, area.x + area.w - HANDLE), (cy - half).clamp(area.y, area.y + area.h - HANDLE), HANDLE, HANDLE)
}

fn handle_rect(inner: Rect, band: &EqBand, scale: f64) -> Rect {
    handle_at(inner, band.hz, handle_db(band, scale))
}

pub(crate) fn draw_curve(list: &mut DrawList, inner: Rect, eq: &Eq, selected: Option<usize>, spectrum: Option<&Spectrum>) {
    for db in GRID_DB {
        list.fill(Rect::new(inner.x, inner.y + y_of_db(db, inner.h), inner.w, 1), Tone::Ink25);
    }
    for (hz, _) in GRID_HZ {
        list.fill(Rect::new(inner.x + x_of_hz(hz, inner.w), inner.y, 1, inner.h), Tone::Ink25);
    }
    if let Some(spectrum) = spectrum.filter(|spectrum| spectrum.live()) {
        spectrum::draw(list, inner, spectrum.pre(), Tone::Ink25);
        spectrum::draw(list, inner, spectrum.post(), Tone::Ink50);
    }
    for db in GRID_DB {
        if db != 0.0 {
            list.text(inner.x + LABEL_PAD, inner.y + y_of_db(db, inner.h) - font::GLYPH_H - 1, &format!("{db:+.0}"), Tone::Ink50, LABEL_SCALE);
        }
    }
    for (hz, label) in GRID_HZ {
        list.text(inner.x + x_of_hz(hz, inner.w) - LABEL_PAD - font::width(label, LABEL_SCALE), inner.y + inner.h - font::GLYPH_H - LABEL_PAD, label, Tone::Ink50, LABEL_SCALE);
    }
    let responses = responses(eq);
    let passes = passes(eq);
    let split = responses.iter().any(|r| r.channel != Channel::Stereo);
    let zero = inner.y + y_of_db(0.0, inner.h);
    for column in 0..inner.w {
        let hz = hz_of_x(column, inner.w);
        let passed = if eq.pass.on { pass_db(&passes, eq.pass.trim_db, hz) } else { 0.0 };
        if split && column % DASH_PERIOD < DASH {
            let y = inner.y + y_of_db(passed + lane_db(&responses, hz, Channel::Side), inner.h);
            list.fill(Rect::new(inner.x + column, y, 1, 1), Tone::Ink50);
        }
        let y = inner.y + y_of_db(passed + lane_db(&responses, hz, Channel::Mid), inner.h);
        let (top, bottom) = (y.min(zero), y.max(zero));
        list.fill(Rect::new(inner.x + column, top, 1, bottom - top), Tone::Ink75);
        list.fill(Rect::new(inner.x + column, y, 1, 1), Tone::Ink100);
    }
    if let Some(band) = soloed(eq) {
        let coeffs = eq::listen(band, CURVE_FS);
        let weight = if band.shape.is_plus() { 4.0 } else { 1.0 };
        for column in (0..inner.w).filter(|column| column % DASH_PERIOD < DASH) {
            let y = inner.y + y_of_db(coeffs.magnitude_db(hz_of_x(column, inner.w), CURVE_FS) * weight, inner.h);
            list.fill(Rect::new(inner.x + column, y, 1, 1), Tone::Ink100);
        }
    }
    for (index, band) in eq.bands.iter().enumerate().filter(|(_, band)| band.on) {
        let rect = handle_rect(inner, band, eq.scale);
        let on = selected == Some(index);
        list.fill(rect, if on { Tone::Ink100 } else { Tone::Ground });
        list.frame(rect, 1, Tone::Ink100);
        list.text_centered(rect, &(index + 1).to_string(), if on { Tone::Ground } else { Tone::Ink100 }, LABEL_SCALE);
        if band.channel != Channel::Stereo {
            let badge = if band.channel == Channel::Mid { "M" } else { "S" };
            list.text((rect.x + HANDLE).min(inner.x + inner.w - font::GLYPH_W), (rect.y - font::GLYPH_H).max(inner.y), badge, Tone::Ink50, LABEL_SCALE);
        }
    }
}

impl Picture {
    pub(super) fn down_eq(&mut self, x: i32, y: i32, inner: Rect, eq: &Eq) -> bool {
        let hit = eq.bands.iter().enumerate().filter(|(_, band)| band.on).map(|(index, band)| (index, handle_rect(inner, band, eq.scale))).filter(|(_, rect)| Rect::new(rect.x - GRAB / 2, rect.y - GRAB / 2, rect.w + GRAB, rect.h + GRAB).contains(x, y)).min_by_key(|(_, rect)| (rect.x + HANDLE / 2 - x).abs() + (rect.y + HANDLE / 2 - y).abs());
        match hit {
            Some((band, _)) => {
                self.band = Some(band);
                self.drag = Some(Drag::Handle { band, q0: eq.bands[band].q });
            }
            None => {
                self.band = None;
                self.drag = Some(Drag::Idle);
            }
        }
        true
    }

    pub(super) fn moved_handle(&self, x: i32, y: i32, dy: i32, inner: Rect, command: bool, eq: &mut Eq) -> bool {
        let Some(Drag::Handle { band, q0 }) = &self.drag else {
            return false;
        };
        let (band, q0) = (*band, *q0);
        let shape = eq.bands[band].shape;
        if command {
            eq.bands[band].q = (q0 * 10f64.powf(-f64::from(dy) / Q_PER_PIXEL)).clamp(0.1, shape.max_q());
        } else {
            eq.bands[band].hz = hz_of_x(x - inner.x, inner.w);
            let db = db_of_y(y - inner.y, inner.h);
            if shape.has_gain() {
                eq.bands[band].gain_db = (db / eq.scale).clamp(-18.0, 18.0);
            } else if shape.has_q() {
                eq.bands[band].q = db_to_q(db, shape.max_q());
            }
        }
        true
    }

    pub(super) fn wheel_eq(&mut self, dy: i32, eq: &mut Eq) -> bool {
        let Some(index) = self.band else {
            return false;
        };
        let band = &mut eq.bands[index];
        if !band.on || !band.shape.has_q() {
            return false;
        }
        if self.wheel.0 != Some(index) {
            self.wheel = (Some(index), 0);
        }
        self.wheel.1 += dy;
        let steps = self.wheel.1 / WHEEL_PX;
        if steps == 0 {
            return false;
        }
        self.wheel.1 -= steps * WHEEL_PX;
        band.q = (band.q * 10f64.powf(f64::from(steps * WHEEL_PX) / Q_PER_PIXEL)).clamp(0.1, band.shape.max_q());
        true
    }
}
