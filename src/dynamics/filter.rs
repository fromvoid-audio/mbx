use std::f64::consts::{LN_10, PI, SQRT_2};

/// Second-order trapezoidal state-variable cell, used both for conventional
/// Butterworth crossovers and for modulated shelves with matching poles.
#[derive(Default)]
struct Cell {
    state: [f64; 2],
}

impl Cell {
    fn tick(&mut self, input: f64, g: f64, damping: f64) -> (f64, f64) {
        let a = 1.0 / (1.0 + g * (g + damping));
        let b = g * a;
        let error = input - self.state[1];
        let band = a * self.state[0] + b * error;
        let low = self.state[1] + b * self.state[0] + g * b * error;
        self.state = [
            flush(2.0 * band - self.state[0]),
            flush(2.0 * low - self.state[1]),
        ];
        (band, low)
    }
}

fn flush(value: f64) -> f64 {
    if value.abs() < 1e-24 {
        0.0
    } else {
        value
    }
}

pub fn prewarp(hz: f64, rate: f64) -> f64 {
    (PI * (hz / rate).min(0.48)).tan()
}

/// Butterworth pole-pair damping coefficients for orders 1 through 8.
const DAMPING: [&[f64]; 8] = [
    &[],
    &[SQRT_2],
    &[1.0],
    &[0.7653668647301796, 1.8477590650225735],
    &[0.6180339887498948, 1.618033988749895],
    &[0.5176380902050415, SQRT_2, 1.9318516525781366],
    &[0.4450418679126288, 1.246979603717467, 1.8019377358048383],
    &[
        0.3901806440322565,
        1.1111404660392044,
        1.6629392246050905,
        1.961570560806461,
    ],
];

#[derive(Default)]
struct Edge {
    real: f64,
    pairs: [Cell; 4],
}

impl Edge {
    fn one_pole(&mut self, input: f64, g: f64) -> f64 {
        let step = g / (1.0 + g) * (input - self.real);
        let low = self.real + step;
        self.real = flush(low + step);
        low
    }

    fn crossover(&mut self, mut input: f64, g: f64, order: usize, high_pass: bool) -> f64 {
        if order % 2 == 1 {
            let low = self.one_pole(input, g);
            input = if high_pass { input - low } else { low };
        }
        for (cell, &k) in self.pairs.iter_mut().zip(DAMPING[order - 1]) {
            let (band, low) = cell.tick(input, g, k);
            input = if high_pass {
                input - k * band - low
            } else {
                low
            };
        }
        input
    }

    /// The high shelf scales the upper plateau. The normalized low shelf
    /// restores unity there. Their overlap provides the requested band gain.
    fn shelf(&mut self, mut input: f64, g: f64, order: usize, db: f64, lower: bool) -> f64 {
        let a = (db * LN_10 / (20.0 * order as f64)).exp();
        let root = a.sqrt();
        let w = if lower { g * root } else { g / root };
        if order % 2 == 1 {
            let low = self.one_pole(input, w);
            input = if lower {
                a * input + (1.0 - a) * low
            } else {
                (input + (a - 1.0) * low) / a
            };
        }
        let gain = a * a;
        for (cell, &k) in self.pairs.iter_mut().zip(DAMPING[order - 1]) {
            let (band, low) = cell.tick(input, w, k);
            input = if lower {
                gain * input + k * a * (1.0 - a) * band + (1.0 - gain) * low
            } else {
                (input + k * (a - 1.0) * band + (gain - 1.0) * low) / gain
            };
        }
        input
    }
}

#[derive(Default)]
pub struct Shelves {
    edges: [Edge; 2],
}

impl Shelves {
    pub fn tick(&mut self, input: f64, edges: [f64; 2], orders: [usize; 2], db: f64) -> f64 {
        let shape = |edge: &mut Edge, input, index: usize| {
            edge.shelf(input, edges[index], orders[index], db, index == 0)
        };
        let input = shape(&mut self.edges[0], input, 0);
        shape(&mut self.edges[1], input, 1)
    }
}

#[derive(Default)]
pub struct BandPass {
    edges: [Edge; 2],
}

impl BandPass {
    pub fn tick(&mut self, input: f64, edges: [f64; 2], orders: [usize; 2]) -> f64 {
        let high = self.edges[0].crossover(input, edges[0], orders[0], true);
        self.edges[1].crossover(high, edges[1], orders[1], false)
    }
}
