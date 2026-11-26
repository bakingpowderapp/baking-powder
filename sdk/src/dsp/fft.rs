use std::f64::consts::TAU;
use std::iter::repeat;

type Complex = (f64, f64);

#[derive(Debug)]
pub struct Fft {
    len: usize,
    swaps: Vec<(usize, usize)>,
    before_reorder: Vec<Vec<Complex>>,
    after_reorder: Vec<Vec<Complex>>,
}

fn butterfly(a: Complex, b: Complex, w: Complex) -> (Complex, Complex) {
    let t = (b.0 * w.0 - b.1 * w.1, b.0 * w.1 + b.1 * w.0);
    ((a.0 + t.0, a.1 + t.1), (a.0 - t.0, a.1 - t.1))
}

fn reversed(index: usize, bits: u32) -> usize {
    index.reverse_bits().checked_shr(usize::BITS - bits).unwrap_or(0)
}

fn halves(x: &mut [f64]) -> [&mut [f64]; 2] {
    let (low, high) = x.split_at_mut(x.len() / 2);
    [low, high]
}

fn quarters(x: &mut [f64]) -> [&mut [f64]; 4] {
    let [low, high] = halves(x);
    let ([q0, q1], [q2, q3]) = (halves(low), halves(high));
    [q0, q1, q2, q3]
}

fn stage([ar, br]: [&mut [f64]; 2], [ai, bi]: [&mut [f64]; 2], twiddles: impl Iterator<Item = Complex>) {
    for (((ar, ai), (br, bi)), w) in ar.iter_mut().zip(ai).zip(br.iter_mut().zip(bi)).zip(twiddles) {
        ((*ar, *ai), (*br, *bi)) = butterfly((*ar, *ai), (*br, *bi), w);
    }
}

fn high_outputs([ar, br]: [&mut [f64]; 2], [ai, bi]: [&mut [f64]; 2], twiddles: impl Iterator<Item = Complex>) {
    for (((ar, ai), (br, bi)), w) in ar.iter().zip(ai.iter()).zip(br.iter_mut().zip(bi)).zip(twiddles) {
        (*br, *bi) = butterfly((*ar, *ai), (*br, *bi), w).1;
    }
}

fn stage_pair([r0, r1, r2, r3]: [&mut [f64]; 4], [i0, i1, i2, i3]: [&mut [f64]; 4], twiddles: impl Iterator<Item = (Complex, Complex, Complex)>) {
    let quads = r0.iter_mut().zip(i0).zip(r1.iter_mut().zip(i1)).zip(r2.iter_mut().zip(i2)).zip(r3.iter_mut().zip(i3));
    for (((((r0, i0), (r1, i1)), (r2, i2)), (r3, i3)), (first, low, high)) in quads.zip(twiddles) {
        let (x0, x1) = butterfly((*r0, *i0), (*r1, *i1), first);
        let (x2, x3) = butterfly((*r2, *i2), (*r3, *i3), first);
        ((*r0, *i0), (*r2, *i2)) = butterfly(x0, x2, low);
        ((*r1, *i1), (*r3, *i3)) = butterfly(x1, x3, high);
    }
}

impl Fft {
    pub fn new(len: usize) -> Fft {
        let bits = len.trailing_zeros();
        let turn = -TAU / len as f64;
        let cos: Vec<f64> = (0..len / 2).map(|m| (turn * m as f64).cos()).collect();
        let sin: Vec<f64> = (0..len / 2).map(|m| (turn * m as f64).sin()).collect();
        let twiddle = |half: usize, j: usize| (cos[j * (len / 2 / half)], sin[j * (len / 2 / half)]);
        let (before, after): (Vec<usize>, Vec<usize>) = (0..bits).map(|stage| 1 << stage).partition(|&half| half < 1 << (bits / 2));
        Fft {
            len,
            swaps: (0..len).map(|i| (i, reversed(i, bits))).filter(|&(i, j)| j > i).collect(),
            before_reorder: before.into_iter().map(|half| (0..half).map(|group| twiddle(half, reversed(group, half.trailing_zeros()))).collect()).collect(),
            after_reorder: after.into_iter().map(|half| (0..half).map(|j| twiddle(half, j)).collect()).collect(),
        }
    }

    pub fn forward(&self, re: &mut [f64], im: &mut [f64]) {
        self.forward_tail(re, im, self.len);
    }

    pub fn forward_tail(&self, re: &mut [f64], im: &mut [f64], tail: usize) {
        let (re, im) = (&mut re[..self.len], &mut im[..self.len]);
        for stages in self.before_reorder.chunks(2) {
            let group = self.len / stages[0].len();
            for (g, (re, im)) in re.chunks_exact_mut(group).zip(im.chunks_exact_mut(group)).enumerate() {
                match stages {
                    [first, second] => {
                        let ([r0, r1, r2, r3], [i0, i1, i2, i3]) = (quarters(re), quarters(im));
                        stage_pair([r0, r2, r1, r3], [i0, i2, i1, i3], repeat((first[g], second[2 * g], second[2 * g + 1])));
                    }
                    _ => stage(halves(re), halves(im), repeat(stages[0][g])),
                }
            }
        }
        for &(i, j) in &self.swaps {
            re.swap(i, j);
            im.swap(i, j);
        }
        let mut stages = self.after_reorder.iter().peekable();
        while let Some(first) = stages.next() {
            let half = first.len();
            if let Some(second) = stages.next_if(|second| second.len() < tail) {
                let (low, high) = second.split_at(half);
                for (re, im) in re.chunks_exact_mut(4 * half).zip(im.chunks_exact_mut(4 * half)) {
                    stage_pair(quarters(re), quarters(im), first.iter().zip(low).zip(high).map(|((&first, &low), &high)| (first, low, high)));
                }
            } else if half < tail {
                for (re, im) in re.chunks_exact_mut(2 * half).zip(im.chunks_exact_mut(2 * half)) {
                    stage(halves(re), halves(im), first.iter().copied());
                }
            } else {
                let from = half - tail;
                for (re, im) in re.chunks_exact_mut(2 * half).zip(im.chunks_exact_mut(2 * half)) {
                    let ([ar, br], [ai, bi]) = (halves(re), halves(im));
                    high_outputs([&mut ar[from..], &mut br[from..]], [&mut ai[from..], &mut bi[from..]], first[from..].iter().copied());
                }
            }
        }
    }
}
