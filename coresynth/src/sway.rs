use crate::params::Shape;
use sdk::dsp::{sample_hold, square, triangle};
use sdk::face::Drawn;
use std::f64::consts::{PI, TAU};
use std::sync::OnceLock;

const CHAOS_CYCLES: usize = 512;
const CHAOS_PER_CYCLE: usize = 256;
const CHAOS_PERIOD: f64 = 6.07;
const CHAOS_SETTLE: f64 = 200.0;
const CHAOS_STEPS_PER_SAMPLE: usize = 4;
const CHAOS_A: f64 = 0.2;
const CHAOS_B: f64 = 0.2;
const CHAOS_C: f64 = 5.7;

pub fn sway(shape: Shape, turns: f64, seed: u64, drawn: &Drawn) -> f64 {
    let cycle = turns.floor();
    let turn = turns - cycle;
    match shape {
        Shape::Sine => (TAU * turns).sin(),
        Shape::Triangle => triangle(turns + 0.75),
        Shape::Saw => 1.0 - 2.0 * turn,
        Shape::Ramp => 2.0 * turn - 1.0,
        Shape::Square => square(turns),
        Shape::Random => sample_hold(cycle as i64, seed),
        Shape::Walk => {
            let (from, to) = (sample_hold(cycle as i64, seed), sample_hold(cycle as i64 + 1, seed));
            from + (to - from) * (0.5 - 0.5 * (PI * turn).cos())
        }
        Shape::Chaos => chaos(turns),
        Shape::Drawn => 2.0 * drawn.value_at(turns) - 1.0,
    }
}

fn chaos(turns: f64) -> f64 {
    let table = CHAOS.get_or_init(chaos_table);
    let position = turns.rem_euclid(CHAOS_CYCLES as f64) * CHAOS_PER_CYCLE as f64;
    let index = position as usize % table.len();
    let next = (index + 1) % table.len();
    let (a, b) = (f64::from(table[index]), f64::from(table[next]));
    a + (position - position.floor()) * (b - a)
}

pub fn ready() {
    CHAOS.get_or_init(chaos_table);
}

static CHAOS: OnceLock<Vec<f32>> = OnceLock::new();

fn chaos_table() -> Vec<f32> {
    let dt = CHAOS_PERIOD / (CHAOS_PER_CYCLE * CHAOS_STEPS_PER_SAMPLE) as f64;
    let mut state = [1.0, 1.0, 0.0];
    let step = |state: &mut [f64; 3]| {
        let f = |[x, y, z]: [f64; 3]| [-y - z, x + CHAOS_A * y, CHAOS_B + z * (x - CHAOS_C)];
        let k1 = f(*state);
        let k2 = f(std::array::from_fn(|i| state[i] + 0.5 * dt * k1[i]));
        let k3 = f(std::array::from_fn(|i| state[i] + 0.5 * dt * k2[i]));
        let k4 = f(std::array::from_fn(|i| state[i] + dt * k3[i]));
        for i in 0..3 {
            state[i] += dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
        }
    };
    for _ in 0..(CHAOS_SETTLE / dt) as usize {
        step(&mut state);
    }
    let mut table = Vec::with_capacity(CHAOS_CYCLES * CHAOS_PER_CYCLE);
    for _ in 0..CHAOS_CYCLES * CHAOS_PER_CYCLE {
        for _ in 0..CHAOS_STEPS_PER_SAMPLE {
            step(&mut state);
        }
        table.push(state[0]);
    }
    let mean = table.iter().sum::<f64>() / table.len() as f64;
    let peak = table.iter().fold(0.0f64, |peak, x| peak.max((x - mean).abs())).max(f64::EPSILON);
    table.iter().map(|x| ((x - mean) / peak) as f32).collect()
}
