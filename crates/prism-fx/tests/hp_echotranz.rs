//! Hyperprism EchoTranz (1.2.1's EDelay, `Delay`): an echo whose time glides
//! through a one-pole smoother applied twice a sample, read with linear
//! interpolation, so a moving time transposes.

const SR: f32 = 44_100.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn tranz_at(sr: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-echotranz", sr).expect("hp-echotranz is in the catalogue");
    for &(key, value) in settings {
        set(&mut k, key, value);
    }
    k
}

fn tranz(settings: &[(&str, f32)]) -> Boxed {
    tranz_at(SR, settings)
}

fn set(k: &mut Boxed, key: &str, value: f32) {
    let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
    k.set_param(i, value);
}

fn run(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    k.process(&mut l, &mut r);
    assert_eq!(l, r);
    l
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn samples(ms: f32, sr: f32) -> f32 {
    (0.001 * ((ms as f64 * sr as f64) as f32) as f64) as f32
}

/// The speed law: N = int(10^(4v/100)), a = 1e-8·N, or 1 at v = 100.
fn coefficient(speed: f64) -> f64 {
    if speed >= 100.0 {
        1.0
    } else {
        1e-8 * (10f64.powf(4.0 * speed / 100.0) as i32) as f64
    }
}

const SCALE: f32 = 1e-5;

/// A rising ramp: read back at any delay it gives the delay itself.
fn ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32 * SCALE).collect()
}

/// The delay each output sample read, from a ramp's output.
fn delays(y: &[f32], from: usize) -> Vec<f64> {
    y.iter().enumerate().map(|(i, v)| (from + i) as f64 - *v as f64 / SCALE as f64).collect()
}

const WET: &[(&str, f32)] = &[("mix", 100.0), ("feedback", 0.0)];

#[test]
fn a_click_returns_at_the_fractional_delay_by_linear_interpolation() {
    let mut k = tranz(&[WET, &[("delayMs", 10.01)]].concat());
    let y = run(&mut k, &click(2_000));
    let f = samples(10.01, SR) - 441.0;
    assert!((y[441] - (1.0 - f)).abs() < 1e-5, "{} against {}", y[441], 1.0 - f);
    assert!((y[442] - f).abs() < 1e-5, "{} against {f}", y[442]);
    assert_eq!(y.iter().filter(|v| v.abs() > 1e-6).count(), 2);
}

#[test]
fn feedback_comes_from_the_whole_sample_tap() {
    // The loop takes the tap below the read, not the interpolated value, so
    // the second repeat is the first one's two samples, not three.
    let mut k = tranz(&[("mix", 100.0), ("feedback", 50.0), ("delayMs", 10.01)]);
    let y = run(&mut k, &click(2_000));
    let f = samples(10.01, SR) - 441.0;
    assert!((y[882] - 0.5 * (1.0 - f)).abs() < 1e-5, "{}", y[882]);
    assert!((y[883] - 0.5 * f).abs() < 1e-5, "{}", y[883]);
    assert_eq!(y[884], 0.0);
}

#[test]
fn the_time_glides_by_the_one_pole_applied_twice_a_sample() {
    // d ← a·t + b·(a·t + b·d), b = 1 − a; at speed 75, a = 1e-5.
    let x = ramp(4 * 44_100);
    let mut k = tranz(&[WET, &[("delayMs", 1000.0), ("speed", 75.0)]].concat());
    let n0 = 48_510;
    run(&mut k, &x[..n0]);
    set(&mut k, "delayMs", 500.0);
    let measured = delays(&run(&mut k, &x[n0..]), n0);
    let (a, t) = (coefficient(75.0), 22_050.0);
    let b = 1.0 - a;
    let mut d = 44_100.0f64;
    for (i, m) in measured.iter().enumerate() {
        d = a * t + b * (a * t + b * d);
        if i % 1000 == 0 {
            assert!((m - d).abs() < 0.05, "sample {i}: {m} against {d}");
        }
    }
    // After a second, e^(−2 × 1e-5 × 44 100) of the move is left.
    let left = (measured[44_099] - t) / 22_050.0;
    assert!((left - 0.4140).abs() < 1e-3, "{left}");
}

#[test]
fn a_moving_time_transposes() {
    // Halving the time at speed 75 starts at 1 + 2a × 22 050 ≈ 1.44 × the pitch.
    let tone: Vec<f32> = (0..(2.0 * SR) as usize).map(|i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / SR).sin()).collect();
    let mut k = tranz(&[WET, &[("delayMs", 1000.0), ("speed", 75.0)]].concat());
    let n0 = 48_510;
    run(&mut k, &tone[..n0]);
    set(&mut k, "delayMs", 500.0);
    let y = run(&mut k, &tone[n0..n0 + 2_205]);
    let crossings = y.windows(2).filter(|p| (p[0] < 0.0) != (p[1] < 0.0)).count() as f32;
    let hz = crossings / 2.0 / (2_205.0 / SR);
    assert!(hz > 1_350.0 && hz < 1_480.0, "{hz} Hz");
}

#[test]
fn the_speed_law_spans_four_decades_and_jumps_at_100() {
    let x = ramp(3 * 44_100);
    for (speed, left) in [(25.0, (-2.0 * 1e-7 * 44_100.0f64).exp()), (50.0, (-2.0 * 1e-6 * 44_100.0f64).exp())] {
        let mut k = tranz(&[WET, &[("delayMs", 1000.0), ("speed", speed)]].concat());
        run(&mut k, &x[..48_510]);
        set(&mut k, "delayMs", 500.0);
        let m = delays(&run(&mut k, &x[48_510..48_510 + 44_100]), 48_510);
        let measured = (m[44_099] - 22_050.0) / 22_050.0;
        assert!((measured - left).abs() < 1e-3, "speed {speed}: {measured} against {left}");
    }
    let mut k = tranz(&[WET, &[("delayMs", 1000.0), ("speed", 100.0)]].concat());
    run(&mut k, &x[..1_000]);
    set(&mut k, "delayMs", 10.0);
    let y = run(&mut k, &click(1_000));
    assert_eq!(y[441], 1.0, "at 100 the time lands at once");
}

#[test]
fn at_48_khz_the_glide_takes_the_same_time() {
    let sr = 48_000.0;
    let x: Vec<f32> = (0..3 * 48_000).map(|i| i as f32 * SCALE).collect();
    let mut k = tranz_at(sr, &[WET, &[("delayMs", 1000.0), ("speed", 75.0)]].concat());
    run(&mut k, &x[..52_800]);
    set(&mut k, "delayMs", 500.0);
    let m = delays(&run(&mut k, &x[52_800..52_800 + 48_000]), 52_800);
    let left = (m[47_999] - 24_000.0) / 24_000.0;
    assert!((left - 0.4140).abs() < 1e-3, "{left}");
}

#[test]
fn the_shortest_delay_is_one_sample() {
    let mut k = tranz(&[WET, &[("delayMs", 0.0)]].concat());
    let y = run(&mut k, &click(100));
    assert_eq!(y[1], 1.0);
}

#[test]
fn controls_are_the_originals() {
    let d = prism_fx::find("hp-echotranz").unwrap().descriptor;
    assert_eq!((d.code, d.label, d.source), (42, "Hyperprism EchoTranz", "Hyperprism-PPC 1.2.1: EchoTranz"));
    assert_eq!(d.category, prism_fx::Category::Delay);
    let keys: Vec<&str> = d.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["delayMs", "feedback", "mix", "speed"]);
    let p = |key: &str| d.spec(key).unwrap();
    assert_eq!((p("delayMs").min, p("delayMs").max, p("delayMs").default), (0.0, 2000.0, 1000.0));
    assert_eq!((p("feedback").min, p("feedback").max, p("feedback").default), (0.0, 100.0, 0.0));
    assert_eq!((p("mix").min, p("mix").max, p("mix").default), (0.0, 100.0, 50.0));
    assert_eq!((p("speed").min, p("speed").max, p("speed").default), (1.0, 100.0, 50.0));
    assert!(d.params.iter().all(|s| !s.log));
    let pad = d.panel.pad.unwrap();
    assert_eq!((pad.x, pad.x_range, pad.y, pad.y_range), ("delayMs", (0.0, 2000.0), "feedback", (0.0, 100.0)));
}
