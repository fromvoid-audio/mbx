mod design;

use crate::parameters::BANDS;
use design::{Designer, KernelSet, Worker};
use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::sync::Arc;

/// One hop of convolution scheduling plus two hops of FIR group delay.
fn hop_length(rate: f64) -> usize {
    ((rate * 0.020).ceil() as usize)
        .next_power_of_two()
        .clamp(16, 8192)
}

pub fn latency(rate: f64) -> usize {
    3 * hop_length(rate)
}

#[derive(Clone, Copy, PartialEq)]
pub struct Shape {
    pub edges: [f64; 2],
    pub orders: [usize; 2],
}

pub type Shapes = [Option<Shape>; BANDS];

enum Kernels {
    Worker(Worker),
    Inline {
        designer: Box<Designer>,
        next: Box<KernelSet>,
    },
}

pub struct LinearBank {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex32>,
    ring: Vec<Complex32>,
    input: Vec<Complex32>,
    old: Vec<Complex32>,
    output: [Vec<Complex32>; BANDS],
    current: KernelSet,
    requested: Shapes,
    kernels: Kernels,
    cursor: usize,
    offset: usize,
    hop: usize,
}

impl LinearBank {
    pub fn new(rate: f64, shapes: Shapes) -> Self {
        let hop = hop_length(rate);
        let n = 8 * hop;
        let buffer = || vec![Complex32::default(); n];
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(n);
        let inverse = planner.plan_fft_inverse(n);
        let scratch = forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len());
        let mut designer = Designer::new(hop, Arc::clone(&forward), Arc::clone(&inverse));
        let mut current = KernelSet::new(n);
        designer.design(&mut current, shapes);
        let kernels = match Worker::start(designer) {
            Ok(worker) => Kernels::Worker(worker),
            Err(_) => Kernels::Inline {
                designer: Box::new(Designer::new(
                    hop,
                    Arc::clone(&forward),
                    Arc::clone(&inverse),
                )),
                next: Box::new(KernelSet::new(n)),
            },
        };
        Self {
            forward,
            inverse,
            scratch: vec![Complex32::default(); scratch],
            ring: buffer(),
            input: buffer(),
            old: buffer(),
            output: std::array::from_fn(|_| buffer()),
            current,
            requested: shapes,
            kernels,
            cursor: 0,
            offset: 0,
            hop,
        }
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
        self.offset = 0;
        self.ring.fill(Complex32::default());
        for output in &mut self.output {
            output.fill(Complex32::default());
        }
    }

    /// L and R share a complex FFT because the filters are real and
    /// use the same impulse response on both channels.
    pub fn tick(&mut self, input: [f64; 2], shapes: Shapes) -> [[f64; 2]; BANDS] {
        let n = self.ring.len();
        let index = n - self.hop + self.offset;
        let result = self.output.each_ref().map(|trace| {
            let value = trace[index] / n as f32;
            [f64::from(value.re), f64::from(value.im)]
        });
        self.ring[self.cursor] = Complex32::new(input[0] as f32, input[1] as f32);
        self.cursor = (self.cursor + 1) & (n - 1);
        self.offset += 1;
        if self.offset == self.hop {
            self.offset = 0;
            self.process(shapes);
        }
        result
    }

    fn process(&mut self, shapes: Shapes) {
        let changed = self.requested != shapes;
        self.requested = shapes;
        let ready = match &mut self.kernels {
            Kernels::Worker(worker) => {
                if changed {
                    worker.request(shapes);
                }
                worker.take()
            }
            Kernels::Inline { designer, next } => {
                if changed {
                    designer.design(next, shapes);
                    std::mem::swap(next.as_mut(), &mut self.current);
                    None
                } else {
                    None
                }
            }
        };

        let mut previous = ready.map(|next| std::mem::replace(&mut self.current, next));
        let old = match (&self.kernels, changed) {
            (Kernels::Inline { next, .. }, true) => Some(next.as_ref()),
            _ => previous.as_ref(),
        };
        let n = self.input.len();
        let start = n - self.hop;
        if self.current.shapes.iter().any(Option::is_some) {
            let right = n - self.cursor;
            self.input[..right].copy_from_slice(&self.ring[self.cursor..]);
            self.input[right..].copy_from_slice(&self.ring[..self.cursor]);
            self.forward
                .process_with_scratch(&mut self.input, &mut self.scratch);
        }

        for (band, output) in self.output.iter_mut().enumerate() {
            if self.current.shapes[band].is_none() || shapes[band].is_none() {
                output[start..].fill(Complex32::default());
                continue;
            }
            convolve(
                &self.input,
                &self.current.transfer[band],
                output,
                &*self.inverse,
                &mut self.scratch,
            );
            if let Some(old) = old.filter(|old| old.shapes[band].is_some()) {
                convolve(
                    &self.input,
                    &old.transfer[band],
                    &mut self.old,
                    &*self.inverse,
                    &mut self.scratch,
                );
                for (i, (old, new)) in self.old[start..]
                    .iter()
                    .zip(&mut output[start..])
                    .enumerate()
                {
                    let blend = (i + 1) as f32 / self.hop as f32;
                    *new = *old + (*new - *old) * blend;
                }
            }
        }
        if let (Kernels::Worker(worker), Some(packet)) = (&self.kernels, previous.take()) {
            worker.recycle(packet);
        }
    }
}

fn convolve(
    input: &[Complex32],
    kernel: &[Complex32],
    output: &mut [Complex32],
    inverse: &dyn Fft<f32>,
    scratch: &mut [Complex32],
) {
    for ((out, input), kernel) in output.iter_mut().zip(input).zip(kernel) {
        *out = input * kernel;
    }
    inverse.process_with_scratch(output, scratch);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamics::filter::prewarp;

    #[test]
    fn impulse_is_symmetric_about_the_reported_latency() {
        let rate = 48_000.0;
        let mut shapes = [None; BANDS];
        shapes[0] = Some(Shape {
            edges: [prewarp(130.0, rate), prewarp(2800.0, rate)],
            orders: [4, 8],
        });
        let mut bank = LinearBank::new(rate, shapes);
        let center = latency(rate);
        let impulse: Vec<_> = (0..=center * 2)
            .map(|n| {
                let input = if n == 0 { [1.0, -0.25] } else { [0.0; 2] };
                bank.tick(input, shapes)[0]
            })
            .collect();
        assert!(impulse[center][0] > 0.1);
        for i in 0..center {
            assert!((impulse[i][0] - impulse[2 * center - i][0]).abs() < 2e-7);
            assert!((impulse[i][0] * -0.25 - impulse[i][1]).abs() < 2e-7);
        }
    }
}
