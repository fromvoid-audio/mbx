//! Independent multiband dynamics.

mod filter;
mod history;
mod linear;

use crate::parameters::{
    self,
    detector::{self, Source},
    layout::Layout,
    BandParam as P, Configuration, Values, BANDS,
};
use filter::{prewarp, BandPass, Shelves};
use history::History;
use linear::{LinearBank, Shape};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    CompressDown,
    CompressUp,
    ExpandDown,
    ExpandUp,
}

impl Mode {
    fn gain(self, over: f64, ratio: f64, knee: f64, range: f64) -> f64 {
        let (over, slope) = match self {
            Self::CompressDown => (over, 1.0 / ratio - 1.0),
            Self::CompressUp => (-over, 1.0 - 1.0 / ratio),
            Self::ExpandDown => (-over, 1.0 - ratio),
            Self::ExpandUp => (over, ratio - 1.0),
        };
        let half = knee * 0.5;
        let distance = if over <= -half {
            0.0
        } else if over >= half {
            over
        } else {
            (over + half).powi(2) / (2.0 * knee)
        };
        (slope * distance).clamp(-range, range)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    Stereo,
    Mid,
    Side,
}

impl Route {
    fn encode(self, [left, right]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Stereo => [left, right],
            _ => [(left + right) * 0.5, (left - right) * 0.5],
        }
    }

    fn decode(self, [first, second]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Stereo => [first, second],
            _ => [first + second, first - second],
        }
    }

    fn active(self, channel: usize) -> bool {
        match self {
            Self::Stereo => true,
            Self::Mid => channel == 0,
            Self::Side => channel == 1,
        }
    }
}

#[derive(Clone, Copy)]
struct Settings {
    enabled: bool,
    solo: bool,
    mute: bool,
    audition: bool,
    band_key: bool,
    edges: [f64; 2],
    key_edges: [f64; 2],
    orders: [usize; 2],
    preview: f64,
    mode: Mode,
    route: Route,
    key: Source,
    threshold: f64,
    ratio: f64,
    range: f64,
    knee: f64,
    attack: f64,
    release: f64,
    makeup: f64,
    link: f64,
}

impl Settings {
    fn shape(self) -> Shape {
        Shape {
            edges: self.edges,
            orders: self.orders,
        }
    }
}

impl Settings {
    fn read(values: &Values, band: usize, rate: f64, position: [f64; 2]) -> Self {
        let read = |field: P| parameters::spec(field.id(band)).plain(values[field.id(band)]);
        let edge = parameters::spec(P::Low.id(band));
        let band_edges = position.map(|normalized| prewarp(edge.plain(normalized), rate));
        let mut orders = [
            read(P::LowSlope) as usize + 1,
            read(P::HighSlope) as usize + 1,
        ];
        if values[P::Low.id(band)] > values[P::High.id(band)] {
            orders.swap(0, 1);
        }
        let key = Source::read(values, band);
        let band_key = key.follows_band(values, band);
        Self {
            enabled: read(P::Enabled) > 0.0,
            solo: read(P::Solo) > 0.0,
            mute: read(P::Mute) > 0.0,
            audition: read(P::Audition) > 0.0,
            band_key,
            edges: band_edges,
            orders,
            key_edges: if band_key {
                band_edges
            } else {
                detector::custom_range(values, band)
                    .map(|edge| prewarp(parameters::spec(P::KeyLow.id(band)).plain(edge), rate))
            },
            preview: read(P::Lookahead) * rate * 0.001,
            mode: match read(P::Mode) as u8 {
                1 => Mode::CompressUp,
                2 => Mode::ExpandDown,
                3 => Mode::ExpandUp,
                _ => Mode::CompressDown,
            },
            route: match read(P::Route) as u8 {
                1 => Route::Mid,
                2 => Route::Side,
                _ => Route::Stereo,
            },
            key,
            threshold: read(P::Threshold),
            ratio: read(P::Ratio),
            range: read(P::Range),
            knee: read(P::Knee),
            attack: time_coefficient(read(P::Attack), rate),
            release: time_coefficient(read(P::Release), rate),
            makeup: read(P::Makeup),
            link: read(P::Link) * 0.01,
        }
    }
}

fn time_coefficient(ms: f64, rate: f64) -> f64 {
    1.0 - (-1000.0 / (ms * rate)).exp()
}

fn approach(value: &mut f64, target: f64, coefficient: f64) {
    *value += (target - *value) * coefficient;
    if (*value - target).abs() < 1e-10 {
        *value = target;
    }
}

#[derive(Default)]
struct Channel {
    detector: BandPass,
    listen: BandPass,
    dynamics: Shelves,
    power: f64,
    dynamic_db: f64,
    applied_db: f64,
}

struct Band {
    settings: Settings,
    channels: [Channel; 2],
    edges: [f64; 2],
    key_edges: [f64; 2],
    solo: f64,
    audition: f64,
    route: Route,
    orders: [usize; 2],
    route_mix: f64,
    running: bool,
    preview: f64,
}

impl Band {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            channels: std::array::from_fn(|_| Channel::default()),
            edges: settings.edges,
            key_edges: settings.key_edges,
            solo: 0.0,
            audition: 0.0,
            route: settings.route,
            orders: settings.orders,
            route_mix: 1.0,
            running: false,
            preview: settings.preview,
        }
    }

    fn update(&mut self, settings: Settings) {
        if self.settings.key != settings.key || self.settings.band_key != settings.band_key {
            for channel in &mut self.channels {
                channel.detector = BandPass::default();
                channel.power = 0.0;
            }
        }
        self.settings = settings;
    }

    fn tick(
        &mut self,
        audio: [f64; 2],
        detector: [f64; 2],
        linear: Option<[f64; 2]>,
        smooth: f64,
        detect: f64,
    ) -> BandFrame {
        let s = self.settings;
        approach(&mut self.solo, f64::from(s.solo && s.enabled), smooth);
        approach(
            &mut self.audition,
            f64::from(s.audition && s.enabled),
            smooth,
        );
        if !s.enabled
            && self
                .channels
                .iter()
                .all(|channel| channel.applied_db == 0.0)
            && self.solo == 0.0
            && self.audition == 0.0
        {
            self.edges = s.edges;
            self.key_edges = s.key_edges;
            self.route = s.route;
            self.orders = s.orders;
            self.route_mix = 1.0;
            if self.running {
                self.channels = std::array::from_fn(|_| Channel::default());
                self.running = false;
            }
            return BandFrame {
                audio,
                solo: [0.0; 2],
                activity: 0.0,
                weight: 0.0,
                key: [0.0; 2],
                key_weight: 0.0,
                detector_db: -90.0,
            };
        }

        for channel in 0..2 {
            approach(&mut self.edges[channel], s.edges[channel], smooth);
            approach(&mut self.key_edges[channel], s.key_edges[channel], smooth);
        }

        self.running = true;
        // Fade through unity before changing the stereo basis or filter
        // order so integration state never crosses incompatible topologies.
        if self.route != s.route || self.orders != s.orders {
            self.route_mix = (self.route_mix - smooth).max(0.0);
            if self.route_mix == 0.0 {
                self.route = s.route;
                self.orders = s.orders;
                self.channels = std::array::from_fn(|_| Channel::default());
            }
        } else {
            self.route_mix = (self.route_mix + smooth).min(1.0);
        }
        let route = self.route;
        let mut signal = route.encode(audio);
        let original = signal;
        let linear = linear.map(|sample| route.encode(sample));
        let source = route.encode(detector);
        let detector_orders = if s.band_key { self.orders } else { [2, 2] };
        let mut filtered = [0.0; 2];
        for ((channel, input), filtered) in self.channels.iter_mut().zip(source).zip(&mut filtered)
        {
            *filtered = channel
                .detector
                .tick(input, self.key_edges, detector_orders);
            approach(&mut channel.power, *filtered * *filtered, detect);
        }
        let maximum = self.channels[0].power.max(self.channels[1].power);
        let mut activity: f64 = 0.0;
        let mut solo = [0.0; 2];
        let mut key = [0.0; 2];
        let mut detector_db = -90.0_f64;
        for (index, channel) in self.channels.iter_mut().enumerate() {
            let enabled = s.enabled && route.active(index);
            let power = channel.power
                + s.link * (maximum - channel.power) * f64::from(route == Route::Stereo);
            let level = 10.0 * power.max(1e-14).log10();
            let target = s.mode.gain(level - s.threshold, s.ratio, s.knee, s.range);
            let speed = if target.abs() > channel.dynamic_db.abs() {
                s.attack
            } else {
                s.release
            };
            approach(&mut channel.dynamic_db, target, speed);

            let target = if enabled && s.mute && !s.solo {
                -96.0
            } else if enabled {
                channel.dynamic_db + s.makeup
            } else {
                0.0
            };
            approach(&mut channel.applied_db, target, smooth);
            if let Some(linear) = linear {
                let gain = (channel.applied_db * std::f64::consts::LN_10 * 0.05).exp();
                signal[index] = original[index] + linear[index] * (gain - 1.0);
                if self.solo > 0.0 && route.active(index) {
                    solo[index] = linear[index] * gain * self.solo;
                }
            } else {
                signal[index] = channel.dynamics.tick(
                    original[index],
                    self.edges,
                    self.orders,
                    channel.applied_db,
                );
                if self.solo > 0.0 && route.active(index) {
                    solo[index] =
                        channel.listen.tick(signal[index], self.edges, self.orders) * self.solo;
                }
            }
            if enabled {
                detector_db = detector_db.max(level);
                if channel.dynamic_db.abs() > activity.abs() {
                    activity = channel.dynamic_db;
                }
            }
            if route.active(index) {
                key[index] = filtered[index] * self.audition;
            }
        }
        let signal = route.decode(signal);
        BandFrame {
            audio: std::array::from_fn(|channel| {
                audio[channel] + self.route_mix * (signal[channel] - audio[channel])
            }),
            solo: route.decode(solo).map(|value| value * self.route_mix),
            activity,
            weight: self.solo * self.route_mix,
            key: route.decode(key).map(|value| value * self.route_mix),
            key_weight: self.audition * self.route_mix,
            detector_db,
        }
    }
}

struct BandFrame {
    audio: [f64; 2],
    solo: [f64; 2],
    activity: f64,
    weight: f64,
    key: [f64; 2],
    key_weight: f64,
    detector_db: f64,
}

pub struct Frame {
    pub output: [f64; 2],
    pub activity: [f64; BANDS],
    pub detector_db: [f64; BANDS],
}

pub struct Engine {
    rate: f64,
    latency: usize,
    preview_budget: usize,
    history: History,
    linear: Option<Box<LinearBank>>,
    bands: [Band; BANDS],
    smooth: f64,
    detect: f64,
    target_mix: f64,
    target_output: f64,
    target_bypass: f64,
    mix: f64,
    output: f64,
    bypass: f64,
}

impl Engine {
    pub fn new(rate: f64, values: &Values) -> Self {
        let config = Configuration::read(values);
        let preview_budget = config.lookahead_samples(rate);
        let latency = preview_budget
            + if config.linear() {
                linear::latency(rate)
            } else {
                0
            };
        let layout = Layout::new(values);
        let bands: [Band; BANDS] = std::array::from_fn(|band| {
            Band::new(Settings::read(values, band, rate, layout.edges[band]))
        });
        let initial_shapes = bands
            .each_ref()
            .map(|band| band.settings.enabled.then_some(band.settings.shape()));
        let mut engine = Self {
            rate,
            latency,
            preview_budget,
            history: History::new(latency),
            linear: config
                .linear()
                .then(|| Box::new(LinearBank::new(rate, initial_shapes))),
            bands,
            smooth: time_coefficient(4.0, rate),
            detect: time_coefficient(2.0, rate),
            target_mix: 1.0,
            target_output: 1.0,
            target_bypass: 0.0,
            mix: 1.0,
            output: 1.0,
            bypass: 0.0,
        };
        engine.update(values);
        engine.mix = engine.target_mix;
        engine.output = engine.target_output;
        engine.bypass = engine.target_bypass;
        engine
    }

    pub fn latency(&self) -> usize {
        self.latency
    }

    pub fn reset(&mut self, values: &Values) {
        self.history.reset();
        if let Some(bank) = &mut self.linear {
            bank.reset();
        }
        let layout = Layout::new(values);
        self.bands = std::array::from_fn(|band| {
            Band::new(Settings::read(values, band, self.rate, layout.edges[band]))
        });
        self.update(values);
        self.mix = self.target_mix;
        self.output = self.target_output;
        self.bypass = self.target_bypass;
    }

    pub fn update(&mut self, values: &Values) {
        let layout = Layout::new(values);
        for (band, state) in self.bands.iter_mut().enumerate() {
            state.update(Settings::read(values, band, self.rate, layout.edges[band]));
        }
        self.target_mix = parameters::spec(parameters::MIX).plain(values[parameters::MIX]) * 0.01;
        self.target_output = 10_f64
            .powf(parameters::spec(parameters::OUTPUT).plain(values[parameters::OUTPUT]) * 0.05);
        self.target_bypass = parameters::spec(parameters::BYPASS).plain(values[parameters::BYPASS]);
    }

    pub fn tick(&mut self, input: [f64; 2], external: [f64; 2]) -> Frame {
        self.history.push(input, external);
        let [left, right, _, _] = self.history.at(self.latency);
        let dry = [left, right];
        let linear = self.linear.as_mut().map(|bank| {
            let [left, right, _, _] = self.history.at(self.preview_budget);
            let shapes = self.bands.each_ref().map(|band| {
                (band.settings.enabled || band.running).then_some(Shape {
                    edges: band.settings.edges,
                    orders: band.settings.orders,
                })
            });
            bank.tick([left, right], shapes)
        });
        let mut processed = dry;
        let mut solo = [0.0; 2];
        let mut solo_weight: f64 = 0.0;
        let mut activity = [0.0; BANDS];
        let mut detector_db = [-90.0; BANDS];
        let mut key = [0.0; 2];
        let mut key_weight: f64 = 0.0;
        for (index, (band, meter)) in self.bands.iter_mut().zip(&mut activity).enumerate() {
            approach(&mut band.preview, band.settings.preview, self.smooth);
            let preview = band.preview.min(self.preview_budget as f64);
            let detector = self.history.pair(
                self.latency as f64 - preview,
                band.settings.key == Source::External,
            );
            let frame = band.tick(
                processed,
                detector,
                linear.map(|region| region[index]),
                self.smooth,
                self.detect,
            );
            processed = frame.audio;
            solo[0] += frame.solo[0];
            solo[1] += frame.solo[1];
            solo_weight = solo_weight.max(frame.weight);
            *meter = frame.activity;
            detector_db[index] = frame.detector_db;
            key[0] += frame.key[0];
            key[1] += frame.key[1];
            key_weight = key_weight.max(frame.key_weight);
        }
        approach(&mut self.mix, self.target_mix, self.smooth);
        approach(&mut self.output, self.target_output, self.smooth);
        approach(&mut self.bypass, self.target_bypass, self.smooth);
        let output = std::array::from_fn(|channel| {
            let dry = dry[channel];
            if self.bypass == 1.0 {
                return dry;
            }
            let mixed = dry + self.mix * (processed[channel] - dry);
            let processed = solo[channel] + mixed * (1.0 - solo_weight);
            let wet = self.output * (key[channel] + processed * (1.0 - key_weight));
            wet + self.bypass * (dry - wet)
        });
        Frame {
            output,
            activity,
            detector_db,
        }
    }
}

#[cfg(test)]
mod tests;
