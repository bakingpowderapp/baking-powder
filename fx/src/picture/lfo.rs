use super::plot;
use sdk::draw::{DrawList, Rect};
use sdk::face::draw_sketch;
use crate::params::VolumeLfo;

pub(crate) fn draw_shape(list: &mut DrawList, inner: Rect, lfo: &VolumeLfo, selected: Option<usize>, live: f32) {
    draw_sketch(list, plot(inner), &lfo.drawn, selected, Some(lfo.phase), Some(f64::from(live)));
}
