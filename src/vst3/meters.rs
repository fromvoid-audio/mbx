use crate::parameters::{Configuration, BANDS};
use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::{
    f32::consts::TAU,
    sync::{
        atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
};

const FFT: usize = 8192;
const RING: usize = FFT * 2;
const PUBLISH_INTERVAL: usize = 64;
pub const POINTS: usize = 160;
pub const TRACES: usize = 3;

pub struct Meters {
    requested: AtomicU8,
    pub latency: AtomicU32,
    readers: AtomicUsize,
    head: AtomicUsize,
    pub rate: AtomicU64,
    pub sidechain: AtomicU32,
    pub gain: [AtomicU32; BANDS],
    pub detector: [AtomicU32; BANDS],
    pub peak: [AtomicU32; 2],
    pub output_channels: AtomicU8,
    signals: [Box<[AtomicU64]>; TRACES],
}

impl Meters {
    pub fn new() -> Self {
        Self {
            requested: AtomicU8::new(0),
            latency: AtomicU32::new(0),
            readers: AtomicUsize::new(0),
            head: AtomicUsize::new(0),
            rate: AtomicU64::new(48_000.0_f64.to_bits()),
            sidechain: AtomicU32::new(0),
            gain: std::array::from_fn(|_| AtomicU32::new(0)),
            detector: std::array::from_fn(|_| AtomicU32::new((-90.0_f32).to_bits())),
            peak: std::array::from_fn(|_| AtomicU32::new(0)),
            output_channels: AtomicU8::new(2),
            signals: std::array::from_fn(|_| (0..RING).map(|_| AtomicU64::new(0)).collect()),
        }
    }

    pub fn configuration(&self) -> Configuration {
        Configuration::from_bits(self.requested.load(Ordering::Acquire))
    }

    pub fn replace_configuration(&self, value: Configuration) {
        self.requested.store(value.bits(), Ordering::Release);
    }

    pub fn request(&self, id: usize, value: f64) -> bool {
        self.requested
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                let next = Configuration::from_bits(old).with_param(id, value).bits();
                (old != next).then_some(next)
            })
            .is_ok()
    }

    pub fn wanted(&self) -> bool {
        self.readers.load(Ordering::Relaxed) > 0
    }

    pub fn reset(&self, rate: f64) {
        self.rate.store(rate.to_bits(), Ordering::Relaxed);
        self.head.store(0, Ordering::Release);
        for meter in self.gain.iter().chain(&self.peak) {
            meter.store(0, Ordering::Relaxed);
        }
        for meter in &self.detector {
            meter.store((-90.0_f32).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn sample(&self, cursor: &mut usize, signals: [[f64; 2]; TRACES]) {
        let index = *cursor % RING;
        for (buffer, signal) in self.signals.iter().zip(signals) {
            buffer[index].store(pack(signal), Ordering::Relaxed);
        }
        *cursor += 1;
        if (*cursor).is_multiple_of(PUBLISH_INTERVAL) {
            self.head.store(*cursor, Ordering::Release);
        }
    }
}

fn pack([left, right]: [f64; 2]) -> u64 {
    u64::from((left as f32).to_bits()) | (u64::from((right as f32).to_bits()) << 32)
}

fn unpack(value: u64) -> Complex32 {
    Complex32::new(
        f32::from_bits(value as u32),
        f32::from_bits((value >> 32) as u32),
    )
}

pub struct Analyzer {
    source: Arc<Meters>,
    fft: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex32>,
    data: [Vec<Complex32>; TRACES],
    window: Vec<f32>,
    head: usize,
}

impl Analyzer {
    pub fn new(source: Arc<Meters>) -> Self {
        source.readers.fetch_add(1, Ordering::Relaxed);
        let fft = FftPlanner::new().plan_fft_forward(FFT);
        Self {
            source,
            scratch: vec![Complex32::default(); fft.get_inplace_scratch_len()],
            data: std::array::from_fn(|_| vec![Complex32::default(); FFT]),
            window: (0..FFT)
                .map(|i| 0.5 - 0.5 * (TAU * i as f32 / FFT as f32).cos())
                .collect(),
            fft,
            head: 0,
        }
    }

    pub fn invalidate(&mut self) {
        self.head = usize::MAX;
    }

    pub fn update(
        &mut self,
        spectra: &mut [[f32; POINTS]; TRACES],
        shown: [bool; TRACES],
        fall: f32,
    ) -> bool {
        let head = self.source.head.load(Ordering::Acquire);
        if head == self.head {
            return false;
        }
        self.head = head;
        if head < FFT {
            if fall > 0.0 {
                *spectra = [[-100.0; POINTS]; TRACES];
            }
            return true;
        }
        for ((data, ring), shown) in self.data.iter_mut().zip(&self.source.signals).zip(shown) {
            if !shown {
                continue;
            }
            for (offset, (output, window)) in data.iter_mut().zip(&self.window).enumerate() {
                let input = ring[(head - FFT + offset) % RING].load(Ordering::Relaxed);
                *output = unpack(input) * window;
            }
        }
        let current = self.source.head.load(Ordering::Acquire);
        // Leave room for the writer's unpublished chunk and reject windows
        // overwritten while the editor copied them.
        if current < head || current - head >= RING - FFT - PUBLISH_INTERVAL {
            return false;
        }

        let rate = f64::from_bits(self.source.rate.load(Ordering::Relaxed)) as f32;
        let denominator = (FFT as f32 * 0.25).powi(2);
        let frequencies = |index: f32| 20.0 * 1000_f32.powf(index / (POINTS - 1) as f32);
        for ((data, trace), shown) in self.data.iter_mut().zip(spectra).zip(shown) {
            if !shown {
                continue;
            }
            self.fft.process_with_scratch(data, &mut self.scratch);
            let power = |bin: usize| {
                (data[bin].norm_sqr() + data[(FFT - bin) % FFT].norm_sqr()) * 0.25 / denominator
            };
            for (index, peak) in trace.iter_mut().enumerate() {
                let bin = frequencies(index as f32) * FFT as f32 / rate;
                let low = frequencies(index as f32 - 0.5) * FFT as f32 / rate;
                let high = frequencies(index as f32 + 0.5) * FFT as f32 / rate;
                let start = low.ceil() as usize;
                let end = (high as usize).min(FFT / 2);
                let amplitude = if bin > FFT as f32 * 0.5 {
                    0.0
                } else if start <= end {
                    (start..=end).map(power).fold(0.0, f32::max)
                } else {
                    let lower = bin as usize;
                    let left = power(lower);
                    let right = power((lower + 1).min(FFT / 2));
                    left + bin.fract() * (right - left)
                };
                let db = 10.0 * amplitude.max(1e-10).log10();
                *peak = db.max(*peak - fall);
            }
        }
        true
    }
}

impl Drop for Analyzer {
    fn drop(&mut self) {
        self.source.readers.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectrum_preserves_antiphase_stereo_energy_and_distinguishes_input_output() {
        let meters = Arc::new(Meters::new());
        let mut analyzer = Analyzer::new(Arc::clone(&meters));
        let mut cursor = 0;
        for n in 0..FFT {
            let tone = f64::from((TAU * 128.0 * n as f32 / FFT as f32).sin());
            meters.sample(
                &mut cursor,
                [
                    [tone, -tone],
                    [tone * 0.25, -tone * 0.25],
                    [tone * 0.5, tone * 0.5],
                ],
            );
        }
        let mut spectra = [[-100.0_f32; POINTS]; TRACES];
        assert!(analyzer.update(&mut spectra, [true; TRACES], 3.0));
        let peak = spectra.map(|trace| trace.into_iter().fold(f32::NEG_INFINITY, f32::max));
        assert!(peak[0].abs() < 1.0, "{peak:?}");
        assert!((peak[1] - peak[0] + 12.04).abs() < 0.05, "{peak:?}");
        assert!((peak[2] - peak[0] + 6.02).abs() < 0.05, "{peak:?}");
        for _ in 0..FFT {
            meters.sample(&mut cursor, [[0.0; 2]; TRACES]);
        }
        assert!(analyzer.update(&mut spectra, [true; TRACES], 0.0));
        assert_eq!(
            spectra[0].iter().copied().fold(f32::NEG_INFINITY, f32::max),
            peak[0]
        );
    }
}
