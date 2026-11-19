use super::{Shape, Shapes};
use crate::parameters::BANDS;
use crossbeam_queue::ArrayQueue;
use rustfft::{num_complex::Complex32, Fft};
use std::{
    f64::consts::PI,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub struct KernelSet {
    pub shapes: Shapes,
    pub transfer: [Vec<Complex32>; BANDS],
}

impl KernelSet {
    pub fn new(n: usize) -> Self {
        Self {
            shapes: [None; BANDS],
            transfer: std::array::from_fn(|_| vec![Complex32::default(); n]),
        }
    }
}

pub struct Designer {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex32>,
    prototype: [Vec<f64>; BANDS],
    cached: Shapes,
    warped: Vec<f64>,
    normalization: Vec<f64>,
    window: Vec<f32>,
}

impl Designer {
    pub fn new(hop: usize, forward: Arc<dyn Fft<f32>>, inverse: Arc<dyn Fft<f32>>) -> Self {
        let n = forward.len();
        Self {
            scratch: vec![
                Complex32::default();
                forward
                    .get_inplace_scratch_len()
                    .max(inverse.get_inplace_scratch_len())
            ],
            forward,
            inverse,
            prototype: std::array::from_fn(|_| vec![0.0; n / 2 + 1]),
            cached: [None; BANDS],
            warped: (0..=n / 2)
                .map(|i| (PI * (i as f64 / n as f64).min(0.499_999_9)).tan())
                .collect(),
            normalization: vec![1.0; n / 2 + 1],
            window: (1..=2 * hop)
                .map(|i| (0.5 + 0.5 * (PI * i as f64 / (2 * hop) as f64).cos()) as f32)
                .collect(),
        }
    }

    pub fn design(&mut self, kernels: &mut KernelSet, shapes: Shapes) {
        self.normalization.fill(0.0);
        for (band, shape) in shapes.into_iter().enumerate() {
            if let Some(shape) = shape {
                if self.cached[band] != Some(shape) {
                    prototype(shape, &self.warped, &mut self.prototype[band]);
                }
                for (sum, gain) in self.normalization.iter_mut().zip(&self.prototype[band]) {
                    *sum += gain;
                }
            }
        }
        self.cached = shapes;
        for sum in &mut self.normalization {
            *sum = sum.max(1.0).recip();
        }
        for (band, shape) in shapes.into_iter().enumerate() {
            if shape.is_some() {
                design_kernel(
                    &self.prototype[band],
                    &self.normalization,
                    &self.window,
                    &mut kernels.transfer[band],
                    [&*self.forward, &*self.inverse],
                    &mut self.scratch,
                );
            }
        }
        kernels.shapes = shapes;
    }
}

struct Mailbox {
    stop: AtomicBool,
    request: ArrayQueue<Shapes>,
    ready: ArrayQueue<KernelSet>,
    free: ArrayQueue<KernelSet>,
}

/// A single worker builds kernels independently of host buffer size. Three
/// reusable packets cover the active bank, one pending bank and one spare.
/// Queue operations move ownership. The audio callback never frees a packet.
pub struct Worker {
    mailbox: Arc<Mailbox>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(mut designer: Designer) -> std::io::Result<Self> {
        let n = designer.forward.len();
        let mailbox = Arc::new(Mailbox {
            stop: AtomicBool::new(false),
            request: ArrayQueue::new(1),
            ready: ArrayQueue::new(1),
            free: ArrayQueue::new(3),
        });
        for _ in 0..2 {
            let _ = mailbox.free.push(KernelSet::new(n));
        }
        let shared = Arc::clone(&mailbox);
        let thread = thread::Builder::new()
            .name("mbx-fir".into())
            .spawn(move || {
                while !shared.stop.load(Ordering::Acquire) {
                    if let Some(mut packet) = shared.free.pop() {
                        if let Some(shapes) = shared.request.pop() {
                            designer.design(&mut packet, shapes);
                            // If the audio callback is between hops, retain the
                            // newest complete partition.
                            if let Some(old) = shared.ready.force_push(packet) {
                                let _ = shared.free.push(old);
                            }
                            continue;
                        }
                        let _ = shared.free.push(packet);
                    }
                    thread::park_timeout(Duration::from_millis(3));
                }
            })?;
        Ok(Self {
            mailbox,
            thread: Some(thread),
        })
    }

    pub fn request(&self, shapes: Shapes) {
        self.mailbox.request.force_push(shapes);
    }

    pub fn take(&self) -> Option<KernelSet> {
        self.mailbox.ready.pop()
    }

    pub fn recycle(&self, kernel: KernelSet) {
        // With exactly three packets, returning one always leaves room.
        let _ = self.mailbox.free.push(kernel);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.mailbox.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}
fn prototype(shape: Shape, warped: &[f64], response: &mut [f64]) {
    let [lo, hi] = shape.edges;
    let [low, high] = shape.orders.map(|order| 2 * order as i32);
    for (result, &f) in response.iter_mut().zip(warped) {
        *result = if f == 0.0 {
            0.0
        } else {
            ((1.0 + (lo / f).powi(low)) * (1.0 + (f / hi).powi(high)))
                .sqrt()
                .recip()
        };
    }
}

fn design_kernel(
    prototype: &[f64],
    normalization: &[f64],
    window: &[f32],
    kernel: &mut [Complex32],
    [forward, inverse]: [&dyn Fft<f32>; 2],
    scratch: &mut [Complex32],
) {
    let n = kernel.len();
    for (i, (magnitude, weight)) in prototype.iter().zip(normalization).enumerate() {
        let gain = Complex32::new((magnitude * weight) as f32, 0.0);
        kernel[i] = gain;
        if i != 0 {
            kernel[n - i] = gain;
        }
    }

    // Start with zero phase, make the finite impulse exactly even, and
    // shift its center into the causal range before transforming it.
    inverse.process_with_scratch(kernel, scratch);
    kernel[0] = Complex32::new(kernel[0].re / n as f32, 0.0);
    for (i, &weight) in window.iter().enumerate() {
        let k = i + 1;
        let value = (kernel[k].re + kernel[n - k].re) * weight / (2.0 * n as f32);
        let sample = Complex32::new(value, 0.0);
        kernel[k] = sample;
        kernel[n - k] = sample;
    }
    let midpoint = window.len();
    kernel[midpoint + 1..n - midpoint].fill(Complex32::default());
    kernel.rotate_right(midpoint);
    forward.process_with_scratch(kernel, scratch);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::FftPlanner;
    use std::time::Instant;

    #[test]
    fn worker_hands_off_complete_banks_and_reuses_recycled_storage() {
        let mut planner = FftPlanner::new();
        let designer = Designer::new(
            64,
            planner.plan_fft_forward(512),
            planner.plan_fft_inverse(512),
        );
        let worker = Worker::start(designer).unwrap();
        for (band, cutoff) in [(0, 0.09), (4, 0.15), (2, 0.04), (1, 0.2)] {
            let mut shapes = [None; BANDS];
            shapes[band] = Some(Shape {
                edges: [cutoff, 0.5],
                orders: [4, 2],
            });
            worker.request(shapes);
            let start = Instant::now();
            let kernels = loop {
                if let Some(kernels) = worker.take() {
                    break kernels;
                }
                assert!(
                    start.elapsed() < Duration::from_millis(200),
                    "kernel worker timed out"
                );
                thread::sleep(Duration::from_millis(1));
            };
            assert!(kernels.shapes == shapes);
            assert!(kernels.transfer[band]
                .iter()
                .any(|value| value.norm() > 0.5));
            worker.recycle(kernels);
        }
    }
}
