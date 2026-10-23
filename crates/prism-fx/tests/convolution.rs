//! Convolution: SoundHack's "Convolve with Impulse Response", as its manual
//! describes it (docs/effects/soundhack-app.md in MultiGrainLive), in real time.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

/// The `window` choice by name.
fn window(name: &str) -> f32 {
    let d = prism_fx::find("convolution").unwrap().descriptor;
    let prism_fx::ParamKind::Choice(labels) = d.spec("window").unwrap().kind else { panic!() };
    labels.iter().position(|l| *l == name).unwrap_or_else(|| panic!("no window {name}")) as f32
}

/// A convolution with this impulse (mono) and these settings.
fn convolve(impulse: &[f32], rate: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("convolution", SR).expect("convolution is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.set_sound(impulse, 1, rate);
    k.reset();
    k
}

fn run(k: &mut Boxed, left: &[f32], right: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (left.to_vec(), right.to_vec());
    k.process(&mut l, &mut r);
    (l, r)
}

fn mono(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    run(k, x, x).0
}

fn click(n: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[0] = 1.0;
    x
}

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}

/// A decaying noise burst, `seconds` long, the shape of a short room.
fn burst(seconds: f32, seed: u32) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    noise(n, seed).iter().enumerate().map(|(i, s)| s * (-5.0 * i as f32 / n as f32).exp()).collect()
}

fn energy(x: &[f32]) -> f32 {
    x.iter().map(|s| s * s).sum()
}

fn latency(k: &Boxed) -> usize {
    k.latency_frames() as usize
}

#[test]
fn a_single_click_as_the_impulse_gives_the_sound_back_delayed_by_one_partition() {
    let mut imp = vec![0.0; 4_800];
    imp[0] = 1.0;
    let mut k = convolve(&imp, SR, &[("lengthMs", 100.0)]);
    assert_eq!(latency(&k), 512);
    let x = noise(24_000, 7);
    let y = mono(&mut k, &x);
    let worst = (512..x.len()).map(|i| (y[i] - x[i - 512]).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-4, "{worst}");
}

#[test]
fn a_delayed_click_as_the_impulse_delays_the_sound() {
    let mut imp = vec![0.0; 4_800];
    imp[300] = 1.0;
    let mut k = convolve(&imp, SR, &[("lengthMs", 100.0)]);
    let x = noise(24_000, 9);
    let y = mono(&mut k, &x);
    let worst = (812..x.len()).map(|i| (y[i] - x[i - 812]).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-4, "{worst}");
}

#[test]
fn a_click_through_it_plays_the_impulse_scaled_to_unit_energy() {
    let imp = burst(0.08, 3);
    let mut k = convolve(&imp, SR, &[("lengthMs", 100.0)]);
    let y = mono(&mut k, &click(12_000));
    let scale = 1.0 / energy(&imp).sqrt();
    for (i, s) in imp.iter().enumerate() {
        assert!((y[512 + i] - s * scale).abs() < 1e-4, "sample {i}");
    }
    assert!((energy(&y) - 1.0).abs() < 1e-3, "unit energy: {}", energy(&y));
}

#[test]
fn length_used_takes_only_that_much_of_the_impulse() {
    let imp = burst(0.5, 4);
    let mut k = convolve(&imp, SR, &[("lengthMs", 100.0)]);
    let y = mono(&mut k, &click(48_000));
    let end = 512 + 4_800;
    assert!(energy(&y[512..end]) > 0.99, "{}", energy(&y[512..end]));
    assert!(y[end..].iter().all(|s| s.abs() < 1e-6), "the impulse ran past the length used");
}

#[test]
fn brighten_lifts_the_impulse_by_six_db_an_octave() {
    // A flat impulse, a click: brightened, it becomes a first difference,
    // whose response rises with frequency.
    let flat = burst(0.05, 5);
    let tone = |hz: f32| -> Vec<f32> { (0..24_000).map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect() };
    let gain = |k: &mut Boxed, hz: f32| {
        k.reset();
        let y = mono(k, &tone(hz));
        energy(&y[12_000..]).sqrt()
    };
    let mut plain = convolve(&flat, SR, &[("lengthMs", 50.0)]);
    let mut bright = convolve(&flat, SR, &[("lengthMs", 50.0), ("brighten", 1.0)]);
    let tilt = |k: &mut Boxed| 20.0 * (gain(k, 8_000.0) / gain(k, 250.0)).log10();
    let lift = tilt(&mut bright) - tilt(&mut plain);
    // Five octaves at +6 dB each.
    assert!((lift - 30.0).abs() < 4.0, "{lift} dB");
}

#[test]
fn a_triangle_window_fades_the_impulse_in_and_out() {
    let imp = vec![0.5; 4_800];
    let shape = |name: &str| mono(&mut convolve(&imp, SR, &[("lengthMs", 100.0), ("window", window(name))]), &click(8_000));
    let rect = shape("Rectangle");
    let tri = shape("Triangle");
    assert!((rect[512] - rect[512 + 2_400]).abs() < 1e-5, "a rectangle is no window");
    assert!(tri[512].abs() < tri[512 + 2_400].abs() * 0.01, "the triangle starts at nothing");
    assert!(tri[512 + 4_799].abs() < tri[512 + 2_400].abs() * 0.01, "and ends at nothing");
}

#[test]
fn ring_modulate_multiplies_by_the_impulse_and_starts_it_again_each_length() {
    // The input held at one: the output is the impulse itself, over and over.
    let imp: Vec<f32> = (0..4_800).map(|i| (i as f32 / 4_800.0) * 2.0 - 1.0).collect();
    let mut k = convolve(&imp, SR, &[("lengthMs", 100.0), ("ringModulate", 1.0)]);
    let y = mono(&mut k, &vec![1.0; 20_000]);
    let peak = imp.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    for i in 512..20_000 {
        let want = imp[(i - 512) % 4_800] / peak;
        assert!((y[i] - want).abs() < 1e-5, "sample {i}: {} against {want}", y[i]);
    }
}

#[test]
fn gain_is_in_decibels_on_top_of_the_scaling() {
    let imp = burst(0.05, 6);
    let x = noise(12_000, 11);
    let flat = mono(&mut convolve(&imp, SR, &[("lengthMs", 50.0)]), &x);
    let up = mono(&mut convolve(&imp, SR, &[("lengthMs", 50.0), ("gainDb", 24.0)]), &x);
    for i in 0..x.len() {
        assert!((up[i] - flat[i] * 10f32.powf(1.2)).abs() < 1e-3, "sample {i}");
    }
}

#[test]
fn mix_blends_with_the_dry_sound_in_step_with_it() {
    let imp = burst(0.05, 6);
    let x = noise(12_000, 12);
    let dry = mono(&mut convolve(&imp, SR, &[("lengthMs", 50.0), ("mix", 0.0)]), &x);
    for i in 512..x.len() {
        assert!((dry[i] - x[i - 512]).abs() < 1e-6, "dry is the input, delayed with the wet");
    }
}

#[test]
fn a_stereo_impulse_convolves_each_side_with_its_own() {
    let mut imp = vec![0.0; 2 * 4_800];
    imp[0] = 1.0; // left, frame 0
    imp[2 * 50 + 1] = 1.0; // right, frame 50
    let mut k = prism_fx::make("convolution", SR).unwrap();
    let i = k.descriptor().index_of("lengthMs").unwrap();
    k.set_param(i, 100.0);
    k.set_sound(&imp, 2, SR);
    k.reset();
    let (l, r) = run(&mut k, &click(4_000), &click(4_000));
    assert!(l[512] > 0.99 && r[512].abs() < 1e-6);
    assert!(r[512 + 50] > 0.99 && l[512 + 50].abs() < 1e-6);
}

#[test]
fn an_impulse_at_another_rate_is_heard_at_this_one() {
    let mut imp = vec![0.0; 2_400];
    imp[100] = 1.0;
    let mut k = convolve(&imp, 24_000.0, &[("lengthMs", 100.0)]);
    let y = mono(&mut k, &click(4_000));
    let loudest = (0..y.len()).max_by(|&a, &b| y[a].abs().total_cmp(&y[b].abs())).unwrap();
    assert!((loudest as i64 - (512 + 200)).abs() <= 1, "at {loudest}");
}

#[test]
fn with_no_impulse_it_passes_the_sound_through_in_step() {
    let mut k = prism_fx::make("convolution", SR).unwrap();
    let x = noise(8_000, 13);
    let y = mono(&mut k, &x);
    for i in 512..x.len() {
        assert!((y[i] - x[i - 512]).abs() < 1e-6);
    }
}

#[test]
fn soundhacks_kaiser_window_sets_both_ends_to_nothing() {
    let imp = vec![0.5; 4_800];
    let y = mono(&mut convolve(&imp, SR, &[("lengthMs", 100.0), ("window", window("Kaiser"))]), &click(8_000));
    assert_eq!(y[512], 0.0, "the first sample");
    assert_eq!(y[512 + 4_799], 0.0, "the last");
    assert!(y[512 + 2_400] > 0.0);
}

#[test]
fn brighten_is_a_ramp_in_frequency_up_to_nyquist() {
    // SoundHack's ramp keeps rising to Nyquist, where a first difference
    // would flatten: an octave below Nyquist is 6 dB down, not 2.
    let flat = burst(0.05, 5);
    let tone = |hz: f32| -> Vec<f32> { (0..24_000).map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect() };
    let gain = |k: &mut Boxed, hz: f32| {
        k.reset();
        let y = mono(k, &tone(hz));
        energy(&y[12_000..]).sqrt()
    };
    let mut plain = convolve(&flat, SR, &[("lengthMs", 50.0)]);
    let mut bright = convolve(&flat, SR, &[("lengthMs", 50.0), ("brighten", 1.0)]);
    let tilt = |k: &mut Boxed| 20.0 * (gain(k, 22_000.0) / gain(k, 11_000.0)).log10();
    let lift = tilt(&mut bright) - tilt(&mut plain);
    assert!((lift - 6.0).abs() < 1.5, "{lift} dB an octave below Nyquist");
}
