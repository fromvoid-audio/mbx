use super::*;
use std::f64::consts::TAU;

const RATE: f64 = 48_000.0;

fn set(values: &mut Values, id: usize, plain: f64) {
    values[id] = parameters::spec(id).normalized(plain);
}

fn band() -> Values {
    let mut values = parameters::empty();
    for (field, value) in [
        (P::Enabled, 1.0),
        (P::Low, 180.0),
        (P::High, 5500.0),
        (P::Threshold, -25.0),
        (P::Attack, 2.0),
        (P::Knee, 0.0),
        (P::Ratio, 8.0),
        (P::Range, 9.0),
    ] {
        set(&mut values, field.id(0), value);
    }
    values
}

fn measure(values: &Values, hz: f64, level: f64, right: f64, key: f64) -> ([f64; 2], f64) {
    let mut engine = Engine::new(RATE, values);
    let mut power = [0.0; 2];
    let mut input_power = 0.0;
    let mut activity = 0.0;
    for n in 0..12_000 {
        let tone = (TAU * hz * n as f64 / RATE).sin();
        let frame = engine.tick([tone * level, tone * level * right], [tone * key; 2]);
        if n >= 9000 {
            input_power += (tone * level).powi(2);
            for (sum, sample) in power.iter_mut().zip(frame.output) {
                *sum += sample * sample;
            }
            activity += frame.activity[0] / 3000.0;
        }
    }
    (
        power.map(|level| 10.0 * (level / input_power).log10()),
        activity,
    )
}

#[test]
fn dry_and_neutral_bands_are_exact_unity() {
    let mut values = band();
    set(&mut values, P::Ratio.id(0), 1.0);
    for id in 1..BANDS {
        set(&mut values, P::Enabled.id(id), 1.0);
        set(&mut values, P::Ratio.id(id), 1.0);
    }
    let mut engine = Engine::new(RATE, &values);
    for n in 0..1000 {
        let input = [(n as f64 * 0.31).sin(), (n as f64 * 0.13).cos()];
        assert_eq!(engine.tick(input, [0.0; 2]).output, input);
    }
}

#[test]
fn gain_curve_honors_direction_soft_knee_ratio_and_range() {
    assert_eq!(Mode::CompressDown.gain(8.0, 2.0, 0.0, 12.0), -4.0);
    assert_eq!(Mode::CompressUp.gain(-8.0, 2.0, 0.0, 12.0), 4.0);
    assert_eq!(Mode::ExpandDown.gain(-8.0, 2.0, 0.0, 5.0), -5.0);
    assert_eq!(Mode::ExpandUp.gain(8.0, 2.0, 0.0, 5.0), 5.0);
    assert_eq!(Mode::CompressDown.gain(0.0, 2.0, 8.0, 12.0), -0.5);
}

#[test]
fn gain_is_bounded_and_local_to_the_band() {
    let mut values = band();
    // Trigger from a broadband external key for all three probe frequencies.
    set(&mut values, P::Key.id(0), 2.0);
    let mut levels = [0.0; 3];
    for (probe, level) in [24.0, 1200.0, 17_000.0].into_iter().zip(&mut levels) {
        let mut engine = Engine::new(RATE, &values);
        let mut input_power = 0.0;
        let mut output_power = 0.0;
        for n in 0..12_000 {
            let input = (TAU * probe * n as f64 / RATE).sin() * 0.7;
            let key = (TAU * 1200.0 * n as f64 / RATE).sin() * 0.9;
            let frame = engine.tick([input; 2], [key; 2]);
            if n >= 9000 {
                assert!((frame.activity[0] + 9.0).abs() < 0.001);
                input_power += input * input;
                output_power += frame.output[0] * frame.output[0];
            }
        }
        *level = 10.0 * (output_power / input_power).log10();
    }
    assert!(levels[0].abs() < 0.1, "{levels:?}");
    assert!((levels[1] + 9.0).abs() < 0.1, "{levels:?}");
    assert!(levels[2].abs() < 0.2, "{levels:?}");
}

#[test]
fn external_key_and_mid_side_routing_affect_only_the_intended_signal() {
    let mut values = band();
    set(&mut values, P::Key.id(0), 2.0);
    assert_eq!(measure(&values, 1000.0, 0.8, 1.0, 0.0).1, 0.0);
    assert!(measure(&values, 1000.0, 0.1, 1.0, 0.8).1 < -8.9);
    set(&mut values, P::Key.id(0), 0.0);
    set(&mut values, P::Route.id(0), 1.0);
    assert!(measure(&values, 1000.0, 0.8, -1.0, 0.0).0[0].abs() < 1e-10);
    assert!(measure(&values, 1000.0, 0.8, 1.0, 0.0).0[0] < -8.0);
    set(&mut values, P::Route.id(0), 2.0);
    assert!(measure(&values, 1000.0, 0.8, 1.0, 0.0).0[0].abs() < 1e-10);
    assert!(measure(&values, 1000.0, 0.8, -1.0, 0.0).0[0] < -8.0);
}

#[test]
fn automation_with_reversed_edges_and_extreme_gains_stays_finite() {
    let mut values = band();
    let mut engine = Engine::new(44_100.0, &values);
    for n in 0..8192 {
        if n % 16 == 0 {
            let phase = (n as f64 * 0.002).sin() * 0.5 + 0.5;
            for band in 0..BANDS {
                set(&mut values, P::Enabled.id(band), 1.0);
                values[P::Low.id(band)] = phase;
                values[P::High.id(band)] = 1.0 - phase;
                set(&mut values, P::Mode.id(band), f64::from(n % 96 != 0) * 3.0);
                set(&mut values, P::Range.id(band), 36.0);
            }
            engine.update(&values);
        }
        let input = (n as f64 * 0.14).sin() * 0.2;
        let frame = engine.tick([input, -input], [0.0; 2]);
        assert!(frame.output.iter().all(|value| value.is_finite()));
    }
}

#[test]
fn lookahead_aligns_unity_and_applies_preview_before_a_transient() {
    let mut values = band();
    set(&mut values, parameters::LOOKAHEAD, 1.0);
    set(&mut values, P::Attack.id(0), 2.0);
    set(&mut values, P::Release.id(0), 100.0);
    set(&mut values, P::Enabled.id(0), 0.0);
    let mut unity = Engine::new(RATE, &values);
    let latency = unity.latency();
    assert_eq!(latency, 960);
    for n in 0..latency + 4 {
        let input = if n == 3 { [0.75, -0.2] } else { [0.0; 2] };
        let expected = if n == latency + 3 {
            [0.75, -0.2]
        } else {
            [0.0; 2]
        };
        assert_eq!(unity.tick(input, [0.0; 2]).output, expected);
    }
    set(&mut values, P::Enabled.id(0), 1.0);
    let render_peak = |lookahead_ms| {
        let mut values = values;
        set(&mut values, P::Lookahead.id(0), lookahead_ms);
        let mut engine = Engine::new(RATE, &values);
        (0..2400)
            .map(|n| {
                let sample = if (500..700).contains(&n) {
                    (TAU * 1400.0 * (n - 500) as f64 / RATE).sin() * 0.9
                } else {
                    0.0
                };
                engine.tick([sample; 2], [0.0; 2]).output[0].abs()
            })
            .fold(0.0_f64, f64::max)
    };
    let normal = render_peak(0.0);
    let preview = render_peak(8.0);
    assert!(
        preview < normal * 0.70,
        "normal={normal}, preview={preview}"
    );
}

#[test]
fn crossover_orders_control_both_edges_independently() {
    let rate = 48_000.0;
    let rms = |hz: f64, orders| {
        let mut filter = BandPass::default();
        let edges = [prewarp(600.0, rate), prewarp(2400.0, rate)];
        (0..8192)
            .map(|n| filter.tick((TAU * hz * n as f64 / rate).sin(), edges, orders))
            .skip(4096)
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt()
    };
    assert!(rms(150.0, [8, 1]) < rms(150.0, [1, 1]) * 0.001);
    assert!(rms(9600.0, [1, 8]) < rms(9600.0, [1, 1]) * 0.001);
    let mid = rms(1200.0, [8, 8]);
    assert!(mid > 44.0 && mid < 46.0, "mid={mid}");
}

#[test]
fn linear_phase_bypass_has_exact_combined_delay() {
    let mut values = band();
    set(&mut values, parameters::PHASE, 1.0);
    set(&mut values, parameters::LOOKAHEAD, 1.0);
    set(&mut values, parameters::BYPASS, 1.0);
    let mut engine = Engine::new(RATE, &values);
    let latency = engine.latency();
    assert_eq!(latency, 4032);
    for n in 0..latency + 5 {
        let input = if n == 4 { [0.75, -0.5] } else { [0.0; 2] };
        let expected = if n == latency + 4 {
            [0.75, -0.5]
        } else {
            [0.0; 2]
        };
        assert_eq!(engine.tick(input, [0.0; 2]).output, expected);
    }
}

#[test]
fn legacy_overlapping_linear_bands_do_not_invert_a_deep_cut() {
    let mut values = parameters::empty();
    set(&mut values, parameters::PHASE, 1.0);
    for i in 0..2 {
        for (param, value) in [
            (P::Enabled, 1.0),
            (P::Low, 150.0),
            (P::High, 8000.0),
            (P::LowSlope, 7.0),
            (P::HighSlope, 7.0),
            (P::Ratio, 1.0),
            (P::Makeup, -18.0),
        ] {
            set(&mut values, param.id(i), value);
        }
    }
    let mut engine = Engine::new(RATE, &values);
    let latency = engine.latency();
    let mut power = 0.0;
    let mut dot = 0.0;
    for n in 0..16_000 {
        let input = (TAU * 700.0 * n as f64 / RATE).sin();
        let result = engine.tick([input; 2], [0.0; 2]).output[0];
        if n > 10_000 {
            let dry = (TAU * 700.0 * (n - latency) as f64 / RATE).sin();
            power += dry * dry;
            dot += dry * result;
        }
    }
    assert!((dot / power - 10_f64.powf(-18.0 / 20.0)).abs() < 0.002);
}

#[test]
fn mute_attenuates_the_band_and_audition_plays_the_filtered_key() {
    let mut values = band();
    set(&mut values, parameters::PHASE, 1.0);
    set(&mut values, P::Low.id(0), 180.0);
    set(&mut values, P::High.id(0), 8000.0);
    set(&mut values, P::LowSlope.id(0), 7.0);
    set(&mut values, P::HighSlope.id(0), 7.0);
    set(&mut values, P::Mute.id(0), 1.0);
    let tone = |hz: f64, n| (TAU * hz * n as f64 / RATE).sin() * 0.7;
    let mut engine = Engine::new(RATE, &values);
    let mut peak: f64 = 0.0;
    for n in 0..12_000 {
        let result = engine.tick([tone(1100.0, n); 2], [0.0; 2]);
        if n > 9000 {
            peak = peak.max(result.output[0].abs());
        }
    }
    assert!(peak < 0.01, "{peak}");

    set(&mut values, P::Key.id(0), 2.0);
    set(&mut values, P::KeyFilter.id(0), 1.0);
    set(&mut values, P::Audition.id(0), 1.0);
    let mut engine = Engine::new(RATE, &values);
    let mut peak: f64 = 0.0;
    for n in 0..12_000 {
        let result = engine.tick([0.0; 2], [tone(1100.0, n); 2]);
        if n > 9000 {
            peak = peak.max(result.output[0].abs());
            assert!(result.detector_db[0] > -12.0);
        }
    }
    assert!(peak > 0.65, "{peak}");
}

#[test]
fn factory_bands_pass_ordinary_mix_levels_at_unity() {
    let mut engine = Engine::new(RATE, &parameters::defaults());
    for n in 0..4800 {
        let input = [
            0.55 * (TAU * 100.0 * n as f64 / RATE).sin(),
            0.6 * (TAU * 5093.0 * n as f64 / RATE).cos(),
        ];
        assert_eq!(engine.tick(input, [0.0; 2]).output, input);
    }
}
