use super::*;

const NODE: i32 = 9;
const NODE_GRAB: i32 = 7;
const NODE_GAP: f64 = 0.001;
const SEGMENT_GRAB: i32 = 10;
const DASH: i32 = 3;
const DASH_PERIOD: i32 = 6;

pub fn x_of_t(t: f64, w: i32) -> i32 {
    ((t.clamp(0.0, 1.0) * f64::from(w - 1)).round() as i32).clamp(0, (w - 1).max(0))
}

pub fn y_of_level(level: f64, h: i32) -> i32 {
    (((1.0 - level.clamp(0.0, 1.0)) * f64::from(h - 1)).round() as i32).clamp(0, (h - 1).max(0))
}

fn node_rect(plot: Rect, point: Point) -> Rect {
    Rect::new(plot.x + x_of_t(point.x, plot.w) - NODE / 2, plot.y + y_of_level(point.y, plot.h) - NODE / 2, NODE, NODE)
}

#[derive(Clone, Copy)]
enum Drag {
    Node { index: usize, from: Point, selected: bool, peeled: Option<End> },
    Segment { index: usize, curve0: f64, bow: f64 },
    Idle,
}

pub struct Sketch {
    drag: Option<Drag>,
    pub node: Option<usize>,
    origin: (i32, i32),
    armed: bool,
}

impl Default for Sketch {
    fn default() -> Sketch {
        Sketch::new()
    }
}

impl Sketch {
    pub const fn new() -> Sketch {
        Sketch { drag: None, node: None, origin: (0, 0), armed: false }
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn down(&mut self, x: i32, y: i32, plot: Rect, drawn: &mut Drawn, shift: bool, clicks: i32) -> bool {
        self.origin = (x, y);
        self.armed = false;
        let node = drawn.shape().iter().enumerate().map(|(index, point)| (index, node_rect(plot, *point))).filter(|(_, rect)| Rect::new(rect.x - NODE_GRAB, rect.y - NODE_GRAB, rect.w + 2 * NODE_GRAB, rect.h + 2 * NODE_GRAB).contains(x, y)).min_by_key(|(_, rect)| (rect.x + NODE / 2 - x).abs() + (rect.y + NODE / 2 - y).abs()).map(|(index, _)| index);
        if let Some(index) = node {
            self.drag = Some(Drag::Node { index, from: drawn.points[index], selected: self.node == Some(index), peeled: None });
            self.node = Some(index);
            return true;
        }
        let t = f64::from(x - plot.x) / f64::from((plot.w - 1).max(1));
        let segment = {
            let points = drawn.shape();
            let y_at = plot.y + y_of_level(drawn.value_at(t.min(0.999_999)), plot.h);
            let found = (0..points.len().saturating_sub(1)).find(|&i| t >= points[i].x && t <= points[i + 1].x && (y - y_at).abs() <= SEGMENT_GRAB);
            found.map(|index| (index, points[index], points[index + 1]))
        };
        self.node = None;
        self.drag = Some(match segment {
            Some((index, _, _)) if shift && clicks >= 2 => {
                drawn.points[index].curve = 0.0;
                Drag::Idle
            }
            Some((index, a, b)) if shift => {
                let local = if b.x - a.x > 1e-9 { ((t - a.x) / (b.x - a.x)).clamp(0.0, 1.0) } else { 0.0 };
                Drag::Segment { index, curve0: a.curve, bow: (4.0 * local * (1.0 - local)).max(0.3) }
            }
            Some(_) => match drawn.split(t) {
                Some(index) => {
                    self.node = Some(index);
                    Drag::Node { index, from: drawn.points[index], selected: false, peeled: None }
                }
                None => Drag::Idle,
            },
            None => Drag::Idle,
        });
        true
    }

    pub fn moved(&mut self, x: i32, y: i32, plot: Rect, drawn: &mut Drawn) -> bool {
        let (dx, dy) = (x - self.origin.0, y - self.origin.1);
        self.armed |= dx.abs() >= ARM || dy.abs() >= ARM;
        if !self.armed {
            return false;
        }
        match self.drag {
            Some(Drag::Node { .. }) => self.moved_node(dx, dy, plot, drawn),
            Some(Drag::Segment { .. }) => self.moved_segment(dy, plot, drawn),
            Some(Drag::Idle) | None => false,
        }
    }

    pub fn up(&mut self, drawn: &mut Drawn) -> bool {
        let Some(drag) = self.drag.take() else {
            return false;
        };
        if self.armed {
            return true;
        }
        if let Drag::Node { index, selected: true, .. } = drag {
            drawn.remove(index);
            self.node = None;
        }
        true
    }

    fn moved_node(&mut self, dx: i32, dy: i32, plot: Rect, drawn: &mut Drawn) -> bool {
        let Some(Drag::Node { index, from, peeled, .. }) = &mut self.drag else {
            return false;
        };
        match *peeled {
            None if *index == 0 && dx >= ARM => {
                if let Some(at) = drawn.peel(End::First) {
                    *index = at;
                    *peeled = Some(End::First);
                }
            }
            None if *index + 1 == drawn.count && dx <= -ARM => {
                if let Some(at) = drawn.peel(End::Last) {
                    *index = at;
                    *peeled = Some(End::Last);
                }
            }
            Some(End::First) if dx <= 0 => {
                *index = drawn.mend(End::First);
                *peeled = None;
            }
            Some(End::Last) if dx >= 0 => {
                *index = drawn.mend(End::Last);
                *peeled = None;
            }
            _ => {}
        }
        let index = *index;
        self.node = Some(index);
        let level = (from.y - f64::from(dy) / f64::from((plot.h - 1).max(1))).clamp(0.0, 1.0);
        let t = if index == 0 {
            0.0
        } else if index + 1 >= drawn.count {
            1.0
        } else {
            let (before, after) = (drawn.points[index - 1].x, drawn.points[index + 1].x);
            let (lo, hi) = (before + NODE_GAP, after - NODE_GAP);
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (before.min(after), before.max(after)) };
            (from.x + f64::from(dx) / f64::from((plot.w - 1).max(1))).max(lo).min(hi)
        };
        drawn.points[index] = Point { x: t, y: level, ..drawn.points[index] };
        true
    }

    fn moved_segment(&self, dy: i32, plot: Rect, drawn: &mut Drawn) -> bool {
        let Some(Drag::Segment { index, curve0, bow, .. }) = &self.drag else {
            return false;
        };
        let delta = -f64::from(dy) / f64::from(plot.h.max(1)) / *bow;
        drawn.points[*index].curve = (*curve0 + delta).clamp(-0.5, 0.5);
        true
    }
}

pub fn draw_nodes(list: &mut DrawList, plot: Rect, drawn: &Drawn, selected: Option<usize>) {
    for (index, point) in drawn.shape().iter().enumerate() {
        let rect = node_rect(plot, *point);
        if selected == Some(index) {
            list.fill(rect, Tone::Ink100);
        } else {
            list.fill(rect, Tone::Ground);
            list.frame(rect, 1, Tone::Ink100);
        }
    }
}

pub fn draw_sketch(list: &mut DrawList, plot: Rect, drawn: &Drawn, selected: Option<usize>, phase: Option<f64>, live: Option<f64>) {
    for t in [0.25, 0.5, 0.75] {
        list.fill(Rect::new(plot.x + x_of_t(t, plot.w), plot.y, 1, plot.h), Tone::Ink25);
    }
    list.fill(Rect::new(plot.x, plot.y + y_of_level(0.5, plot.h), plot.w, 1), Tone::Ink25);
    let floor = plot.y + plot.h;
    for column in 0..plot.w {
        let t = f64::from(column) / f64::from((plot.w - 1).max(1));
        let y = plot.y + y_of_level(drawn.value_at(t.min(0.999_999)), plot.h);
        list.fill(Rect::new(plot.x + column, y, 1, floor - y), Tone::Ink50);
        list.fill(Rect::new(plot.x + column, y, 1, 1), Tone::Ink100);
    }
    if let Some(phase) = phase {
        list.fill(Rect::new(plot.x + x_of_t(phase, plot.w), plot.y, 1, plot.h), Tone::Ink100);
    }
    if let Some(live) = live {
        let x = plot.x + x_of_t(live, plot.w);
        let mut y = plot.y;
        while y < floor {
            list.fill(Rect::new(x, y, 1, DASH.min(floor - y)), Tone::Ink100);
            y += DASH_PERIOD;
        }
    }
    draw_nodes(list, plot, drawn, selected);
}
