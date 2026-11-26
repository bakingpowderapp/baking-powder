use super::*;
use crate::draw::{DrawList, Rect, Tone};

const GRIP_LINE_W: i32 = 13;
const GRIP_LINES: [i32; 3] = [10, 14, 18];
const ARROW_W: i32 = 5;
const ARROW_RISE: i32 = 6;
const BRACKET: i32 = 5;
const BRACKET_INSET: i32 = 2;
const TITLE_INSET: i32 = 3;
const TITLE_RISE: i32 = 4;

pub fn draw_dot(list: &mut DrawList, x: i32, y: i32) {
    list.fill(Rect::new(x - 1, y - 1, DOT + 2, DOT + 2), Tone::Ground);
    list.fill(Rect::new(x, y, DOT, DOT), Tone::Ink100);
}

pub fn cell_rect<P>(cell: &Cell<P>, rows: i32) -> Rect {
    Rect::new(cell.column * PITCH, (rows + cell.row) * PITCH, cell.span * PITCH + FRAME, CELL)
}

pub(super) fn slot_of<P>(cell: &Cell<P>) -> (i32, i32, i32) {
    (cell.row, cell.column, cell.span)
}

pub fn readout_rect(rows: i32) -> Rect {
    Rect::new(0, 0, W, rows * PITCH + FRAME)
}

pub(super) fn grid_rect(rows: i32) -> Rect {
    Rect::new(0, rows * PITCH, W, (ROWS - rows) * PITCH + FRAME)
}

pub fn interior(rect: Rect) -> Rect {
    Rect::new(rect.x + FRAME, rect.y + FRAME, rect.w - 2 * FRAME, rect.h - 2 * FRAME)
}

struct Lines {
    x: i32,
    top: i32,
    value_y: i32,
    hint_y: i32,
    fit: usize,
    room: usize,
}

fn lines(rows: i32) -> Lines {
    let inner = interior(readout_rect(rows));
    let x = inner.x + READOUT_PAD;
    let width = inner.w - 2 * READOUT_PAD;
    let top = inner.y + READOUT_PAD + 2 * READOUT_LINE;
    let hint_y = inner.y + inner.h - READOUT_PAD - HINT_LINES as i32 * HINT_LINE + LEADING;
    let value_y = top + (hint_y - top - font::GLYPH_H * VALUE_SCALE) / 2;
    Lines { x, top, value_y, hint_y, fit: font::glyphs_in(width, LABEL_SCALE), room: font::glyphs_in(width, VALUE_SCALE) }
}

fn readout_lines(list: &mut DrawList, rows: i32, band: &str, param: &str) -> Lines {
    let rect = readout_rect(rows);
    let inner = interior(rect);
    list.fill(inner, Tone::Ground);
    list.frame(rect, FRAME, Tone::Ink100);
    let lines = lines(rows);
    let heading = font::glyphs_in(inner.w - 2 * READOUT_PAD, READOUT_SCALE);
    list.text(lines.x, inner.y + READOUT_PAD, &font::shortened(band, heading), Tone::Ink50, READOUT_SCALE);
    list.text(lines.x, inner.y + READOUT_PAD + READOUT_LINE, &font::shortened(param, heading), Tone::Ink75, READOUT_SCALE);
    lines
}

fn show_hint(list: &mut DrawList, lines: &Lines, hint: &str) {
    for (index, line) in font::fitted(hint, lines.fit, HINT_LINES).iter().enumerate() {
        list.text(lines.x, lines.hint_y + index as i32 * HINT_LINE, line, Tone::Ink75, LABEL_SCALE);
    }
}

pub fn show_sound(list: &mut DrawList, rows: i32, name: &str, description: &str) {
    if name.is_empty() {
        return;
    }
    let lines = lines(rows);
    list.text(lines.x, lines.value_y, &font::shortened(name, lines.room), Tone::Ink75, VALUE_SCALE);
    for (index, line) in font::fitted(description, lines.fit, HINT_LINES).iter().enumerate() {
        list.text(lines.x, lines.hint_y + index as i32 * HINT_LINE, line, Tone::Ink50, LABEL_SCALE);
    }
}

pub fn show_readout(list: &mut DrawList, rows: i32, band: &str, param: &str, value: &str, desc: &str) {
    let lines = readout_lines(list, rows, band, param);
    list.text(lines.x, lines.value_y, &font::shortened(value, lines.room), Tone::Ink100, VALUE_SCALE);
    show_hint(list, &lines, desc);
}

pub fn show_pictured(list: &mut DrawList, rows: i32, band: &str, param: &str, desc: &str, picture: impl FnOnce(&mut DrawList, Rect)) {
    let lines = readout_lines(list, rows, band, param);
    let inner = interior(readout_rect(rows));
    picture(list, Rect::new(lines.x, lines.top, inner.w - 2 * READOUT_PAD, lines.hint_y - lines.top - READOUT_PAD));
    show_hint(list, &lines, desc);
}

pub fn show_entry(list: &mut DrawList, rows: i32, band: &str, param: &str, text: &str, unit: &str, hint: &str) {
    let lines = readout_lines(list, rows, band, param);
    list.text(lines.x, lines.value_y, text, Tone::Ink100, VALUE_SCALE);
    let caret = lines.x + text.chars().count() as i32 * font::ADVANCE * VALUE_SCALE;
    list.fill(Rect::new(caret, lines.value_y, VALUE_SCALE, font::GLYPH_H * VALUE_SCALE), Tone::Ink100);
    list.text(caret + VALUE_SCALE, lines.value_y, unit, Tone::Ink50, VALUE_SCALE);
    show_hint(list, &lines, hint);
}

pub fn panel_rect(anchor: Rect, options: &[&str], icon_w: i32, bounds: Rect) -> Rect {
    let widest = options.iter().map(|option| font::width(option, LABEL_SCALE)).max().unwrap_or(0);
    let w = anchor.w.max(PANEL_MIN_W).max(widest + icon_w + 2 * OPTION_PAD + 2 * FRAME);
    let h = options.len() as i32 * OPTION_H + 2 * FRAME;
    Rect::new(anchor.x.min(bounds.x + bounds.w - w), (anchor.y + CELL - FRAME).min(bounds.y + bounds.h - h), w, h)
}

pub fn option_rect(panel: Rect, index: usize) -> Rect {
    Rect::new(panel.x + FRAME, panel.y + FRAME + index as i32 * OPTION_H, panel.w - 2 * FRAME, OPTION_H)
}

pub fn option_at(panel: Rect, options: &[&str], x: i32, y: i32) -> Option<usize> {
    (0..options.len()).find(|&index| option_rect(panel, index).contains(x, y))
}

pub fn draw_options(list: &mut DrawList, panel: Rect, options: &[&str], current: Option<usize>, hovered: Option<usize>, pressed: Option<usize>, icon: Option<Icon>) {
    list.fill(panel, Tone::Ground);
    let icon_w = if icon.is_some() { ICON_W + OPTION_PAD } else { 0 };
    for (index, option) in options.iter().enumerate() {
        let row = option_rect(panel, index);
        let ink = if current == Some(index) || pressed == Some(index) {
            list.fill(row, Tone::Ink100);
            Tone::Ground
        } else {
            if hovered == Some(index) {
                list.fill(row, Tone::Ink25);
            }
            Tone::Ink100
        };
        if let Some(icon) = icon {
            icon(list, index, row.x + OPTION_PAD, row.y + (OPTION_H - ICON_H) / 2, ink);
        }
        list.text(row.x + OPTION_PAD + icon_w, row.y + (OPTION_H - font::GLYPH_H * LABEL_SCALE) / 2, option, ink, LABEL_SCALE);
    }
    list.frame(panel, FRAME, Tone::Ink100);
}

pub const fn entry_glyphs(w: i32) -> usize {
    font::glyphs_in(w - 2 * FRAME - 2 * OPTION_PAD - LABEL_SCALE, LABEL_SCALE)
}

pub fn draw_entry_cell(list: &mut DrawList, rect: Rect, text: &str, hint: &str) {
    let inner = interior(rect);
    list.fill(rect, Tone::Ground);
    list.frame(rect, FRAME, Tone::Ink100);
    let x = inner.x + OPTION_PAD;
    let y = inner.y + (inner.h - font::GLYPH_H * LABEL_SCALE) / 2;
    list.text(x, y, text, Tone::Ink100, LABEL_SCALE);
    let caret = x + text.chars().count() as i32 * font::ADVANCE * LABEL_SCALE;
    list.fill(Rect::new(caret, y, LABEL_SCALE, font::GLYPH_H * LABEL_SCALE), Tone::Ink100);
    if text.is_empty() {
        list.text(caret + 2 * LABEL_SCALE, y, hint, Tone::Ink50, LABEL_SCALE);
    }
}

fn fill_rise(travel: f64) -> Option<(i32, i32, i32)> {
    if travel < EPSILON {
        return None;
    }
    let h = ((travel * f64::from(INTERIOR)).round() as i32).clamp(1, INTERIOR);
    Some((INTERIOR - h, INTERIOR - 1, INTERIOR - h))
}

fn fill_centre(travel: f64) -> Option<(i32, i32, i32)> {
    let offset = travel - 0.5;
    if offset.abs() < EPSILON {
        return None;
    }
    let h = ((offset.abs() * f64::from(2 * CENTRE)).round() as i32).clamp(1, CENTRE);
    Some(if offset > 0.0 { (CENTRE - h, CENTRE, CENTRE - h) } else { (CENTRE, CENTRE + h, CENTRE + h) })
}

pub(super) fn fill_rows(centre: bool, travel: f64) -> Option<(i32, i32, i32)> {
    if centre { fill_centre(travel) } else { fill_rise(travel) }
}

pub(super) fn draw_arrow(list: &mut DrawList, inner: Rect, fill: Option<(i32, i32, i32)>, ink: Tone) {
    for row in 0..=ARROW_W / 2 {
        let w = ARROW_W - 2 * row;
        let y = inner.h - ARROW_RISE + row;
        let filled = fill.is_some_and(|(lo, hi, _)| (lo..=hi).contains(&y));
        list.fill(Rect::new(inner.x + (inner.w - w) / 2, inner.y + y, w, 1), if filled { Tone::Ground } else { ink });
    }
}

pub(super) fn draw_brackets(list: &mut DrawList, inner: Rect, ink: Tone) {
    let (left, top) = (inner.x + BRACKET_INSET, inner.y + BRACKET_INSET);
    let (right, bottom) = (inner.x + inner.w - BRACKET_INSET, inner.y + inner.h - BRACKET_INSET);
    for x in [left, right - BRACKET] {
        list.fill(Rect::new(x, top, BRACKET, 1), ink);
        list.fill(Rect::new(x, bottom - 1, BRACKET, 1), ink);
    }
    for y in [top, bottom - BRACKET] {
        list.fill(Rect::new(left, y, 1, BRACKET), ink);
        list.fill(Rect::new(right - 1, y, 1, BRACKET), ink);
    }
}

fn pan_centre(width: i32) -> (i32, i32) {
    ((width - 1) / 2, width / 2)
}

fn fill_pan(travel: f64, width: i32) -> Option<(i32, i32, i32)> {
    let offset = travel - 0.5;
    if offset.abs() < EPSILON {
        return None;
    }
    let (near, far) = pan_centre(width);
    let w = ((offset.abs() * f64::from(2 * near)).round() as i32).clamp(1, near.max(1));
    Some(if offset > 0.0 { (near, far + w, far + w) } else { (near - w, far, near - w) })
}

pub(super) fn draw_pan_cell(list: &mut DrawList, rect: Rect, travel: f64, label: &str, hovered: bool) {
    let inner = interior(rect);
    let label = &fit(label, inner.w);
    list.frame(rect, FRAME, Tone::Ink100);
    if hovered {
        list.fill(inner, Tone::Ink25);
    }
    let (near, far) = pan_centre(inner.w);
    list.fill(Rect::new(inner.x + near, inner.y, far - near + 1, inner.h), Tone::Ink25);
    let x = inner.x + (inner.w - font::width(label, LABEL_SCALE)) / 2;
    let y = inner.y + (inner.h - font::GLYPH_H * LABEL_SCALE) / 2;
    match fill_pan(travel, inner.w) {
        Some((lo, hi, cap)) => {
            list.fill(Rect::new(inner.x + lo, inner.y, hi - lo + 1, inner.h), Tone::Ink75);
            list.fill(Rect::new(inner.x + cap, inner.y, 1, inner.h), Tone::Ink100);
            list.text_between(x, y, label, inner.x + lo, inner.x + hi + 1, Tone::Ground, Tone::Ink100, LABEL_SCALE);
        }
        None => list.text(x, y, label, Tone::Ink100, LABEL_SCALE),
    }
}

pub fn fit(label: &str, width: i32) -> String {
    if font::width(label, LABEL_SCALE) <= width {
        return String::from(label);
    }
    font::shortened(label.split(' ').next().unwrap_or(label), font::glyphs_in(width, LABEL_SCALE))
}

pub fn draw_value_cell(list: &mut DrawList, rect: Rect, centre: bool, travel: f64, label: &str, hovered: bool) {
    let inner = interior(rect);
    let label = &fit(label, inner.w);
    list.frame(rect, FRAME, Tone::Ink100);
    if hovered {
        list.fill(inner, Tone::Ink25);
    }
    if centre {
        list.fill(Rect::new(inner.x, inner.y + CENTRE, inner.w, 1), Tone::Ink25);
    }
    let fill = fill_rows(centre, travel);
    let x = inner.x + (inner.w - font::width(label, LABEL_SCALE)) / 2;
    let y = inner.y + (inner.h - font::GLYPH_H * LABEL_SCALE) / 2;
    match fill {
        Some((lo, hi, cap)) => {
            list.fill(Rect::new(inner.x, inner.y + lo, inner.w, hi - lo + 1), Tone::Ink75);
            list.fill(Rect::new(inner.x, inner.y + cap, inner.w, 1), Tone::Ink100);
            list.text_band(x, y, label, inner.y + lo, inner.y + hi + 1, Tone::Ground, Tone::Ink100, LABEL_SCALE);
        }
        None => list.text(x, y, label, Tone::Ink100, LABEL_SCALE),
    }
}

fn draw_chevron(list: &mut DrawList, x: i32, y: i32, direction: i32, ink: Tone) {
    for row in 0..CHEVRON_H {
        let reach = (row - CHEVRON_H / 2).abs();
        let column = if direction < 0 { reach } else { CHEVRON_W - 1 - reach };
        list.fill(Rect::new(x + column, y + row, 1, 1), ink);
    }
}

pub(super) fn draw_pick_cell(list: &mut DrawList, rect: Rect, label: &str, zone: i32) {
    let inner = interior(rect);
    list.frame(rect, FRAME, Tone::Ink100);
    let third = inner.w / 3;
    let (x, w) = match zone {
        -1 => (inner.x, third),
        1 => (inner.x + inner.w - third, third),
        _ => (inner.x + third, inner.w - 2 * third),
    };
    list.fill(Rect::new(x, inner.y, w, inner.h), Tone::Ink25);
    list.text_centered(inner, &fit(label, inner.w), Tone::Ink100, LABEL_SCALE);
    draw_chevron(list, inner.x + CHEVRON_INSET, inner.y + CHEVRON_INSET, -1, Tone::Ink100);
    draw_chevron(list, inner.x + inner.w - CHEVRON_INSET - CHEVRON_W, inner.y + CHEVRON_INSET, 1, Tone::Ink100);
}

pub fn lit(list: &mut DrawList, rect: Rect, on: bool, hovered: bool) -> (Rect, Tone) {
    let inner = interior(rect);
    list.frame(rect, FRAME, Tone::Ink100);
    let (wash, ink) = match (on, hovered) {
        (true, _) => (Some(Tone::Ink100), Tone::Ground),
        (false, true) => (Some(Tone::Ink25), Tone::Ink100),
        (false, false) => (None, Tone::Ink100),
    };
    if let Some(tone) = wash {
        list.fill(inner, tone);
    }
    (inner, ink)
}

pub fn draw_grip(list: &mut DrawList, inner: Rect, ink: Tone) {
    for line in GRIP_LINES {
        list.fill(Rect::new(inner.x + (inner.w - GRIP_LINE_W) / 2, inner.y + line, GRIP_LINE_W, 1), ink);
    }
}

pub(super) fn draw_title(list: &mut DrawList, rect: Rect, label: &str) {
    list.text(rect.x + FRAME + TITLE_INSET, rect.y + PITCH - TITLE_RISE - font::GLYPH_H * LABEL_SCALE, label, Tone::Ink50, LABEL_SCALE);
}

pub fn draw_word_cell(list: &mut DrawList, rect: Rect, label: &str, on: bool, hovered: bool) {
    let (inner, ink) = lit(list, rect, on, hovered);
    list.text_centered(inner, &fit(label, inner.w), ink, LABEL_SCALE);
}

