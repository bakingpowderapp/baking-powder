use crate::json::Json;
use crate::schema::{Fields, bounded};

pub const LFO_POINTS: usize = 32;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub curve: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum End {
    First,
    Last,
}

#[derive(Clone, Copy, Debug)]
pub struct Drawn {
    pub points: [Point; LFO_POINTS],
    pub count: usize,
}

impl PartialEq for Drawn {
    fn eq(&self, other: &Drawn) -> bool {
        self.shape() == other.shape()
    }
}

impl Default for Drawn {
    fn default() -> Drawn {
        Drawn::RAMP
    }
}

impl Drawn {
    pub const RAMP: Drawn = {
        let mut points = [Point { x: 0.0, y: 0.0, curve: 0.0 }; LFO_POINTS];
        points[1] = Point { x: 1.0, y: 1.0, curve: 0.0 };
        Drawn { points, count: 2 }
    };

    pub fn shape(&self) -> &[Point] {
        &self.points[..self.count]
    }

    fn segment(&self, x: f64) -> usize {
        let points = self.shape();
        let mut segment = 0;
        while segment + 2 < points.len() && x >= points[segment + 1].x {
            segment += 1;
        }
        segment
    }

    pub fn value_at(&self, phase: f64) -> f64 {
        let phase = phase - phase.floor();
        let segment = self.segment(phase);
        let (_, y) = along(self.points[segment], self.points[segment + 1], phase);
        y
    }

    pub fn split(&mut self, x: f64) -> Option<usize> {
        if self.count >= LFO_POINTS {
            return None;
        }
        let x = x.clamp(0.0, 1.0);
        let segment = self.segment(x);
        let a = self.points[segment];
        let (s, y) = along(a, self.points[segment + 1], x);
        let index = segment + 1;
        self.points.copy_within(index..self.count, index + 1);
        self.points[segment].curve = a.curve * s * s;
        self.points[index] = Point { x, y, curve: a.curve * (1.0 - s) * (1.0 - s) };
        self.count += 1;
        Some(index)
    }

    pub fn remove(&mut self, index: usize) {
        if index == 0 || index + 1 >= self.count {
            return;
        }
        self.points.copy_within(index + 1..self.count, index);
        self.count -= 1;
    }

    pub fn peel(&mut self, end: End) -> Option<usize> {
        if self.count >= LFO_POINTS {
            return None;
        }
        let floor = |x| Point { x, y: 0.0, curve: 0.0 };
        self.count += 1;
        Some(match end {
            End::First => {
                self.points.copy_within(0..self.count - 1, 1);
                self.points[0] = floor(0.0);
                1
            }
            End::Last => {
                self.points[self.count - 1] = floor(1.0);
                self.points[self.count - 2].curve = 0.0;
                self.count - 2
            }
        })
    }

    pub fn mend(&mut self, end: End) -> usize {
        if self.count > 2 {
            self.count -= 1;
            match end {
                End::First => {
                    self.points.copy_within(1..=self.count, 0);
                    self.points[0].x = 0.0;
                }
                End::Last => self.points[self.count - 1].x = 1.0,
            }
        }
        match end {
            End::First => 0,
            End::Last => self.count - 1,
        }
    }

    pub fn to_json(&self) -> Json {
        Json::Array(self.shape().iter().map(|p| Json::Object(vec![(String::from("x"), Json::Number(p.x)), (String::from("y"), Json::Number(p.y)), (String::from("bend"), Json::Number(p.curve))])).collect())
    }

    pub fn from_json(items: &[Json]) -> Option<Drawn> {
        let mut read: Vec<Point> = items
            .iter()
            .filter_map(|item| {
                let at = |key: &str| item.get(key).and_then(Json::as_f64).filter(|v| v.is_finite());
                Some(Point { x: at("x")?.clamp(0.0, 1.0), y: at("y")?.clamp(0.0, 1.0), curve: at("bend").unwrap_or(0.0).clamp(-1.0, 1.0) })
            })
            .take(LFO_POINTS)
            .collect();
        if read.len() < 2 {
            return None;
        }
        read.sort_by(|a, b| a.x.total_cmp(&b.x));
        read[0].x = 0.0;
        let last = read.len() - 1;
        read[last].x = 1.0;
        let mut drawn = Drawn { points: [Point::default(); LFO_POINTS], count: read.len() };
        drawn.points[..read.len()].copy_from_slice(&read);
        Some(drawn)
    }

    pub fn manual(about: &str) -> Json {
        let mut point = Fields::new();
        point.push("x", Json::from("number 0 to 1: where in the cycle the point sits"));
        point.push("y", Json::from("number 0 to 1: the level there"));
        point.manual("bend", bounded(0.0, -1.0..=1.0, false), "curves the segment to the next point");
        Json::Array(vec![Json::from(about), point.into_json()])
    }
}

fn along(a: Point, b: Point, x: f64) -> (f64, f64) {
    let span = b.x - a.x;
    let s = if span > 1e-9 { ((x - a.x) / span).clamp(0.0, 1.0) } else { 0.0 };
    (s, (a.y + (b.y - a.y) * s + 4.0 * s * (1.0 - s) * a.curve).clamp(0.0, 1.0))
}
