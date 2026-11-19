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

#[test]
fn length_step_glides_without_a_click_and_lands_within_bound() {
    let imp = burst(1.0, 21);
    let tone = |n: usize| -> Vec<f32> {
        (0..n).map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / SR).sin()).collect()
    };
    let mut k = convolve(&imp, SR, &[("lengthMs", 100.0)]);
    let len_idx = k.descriptor().index_of("lengthMs").unwrap();

    // Warm up for 0.5 s and measure the steady signal's largest sample-to-sample step.
    let total = (SR * 1.5) as usize;
    let x = tone(total);
    let mut y = vec![0.0f32; total];
    let step_at = (SR * 0.5) as usize;

    let mut pos = 0usize;
    while pos < total {
        let end = (pos + 64).min(total);
        if pos == step_at {
            k.set_param(len_idx, 500.0);
        }
        let mut l = x[pos..end].to_vec();
        let mut r = x[pos..end].to_vec();
        k.process(&mut l, &mut r);
        y[pos..end].copy_from_slice(&l);
        pos = end;
    }

    let steady_jump = (12_000..step_at)
        .map(|i| (y[i] - y[i - 1]).abs())
        .fold(0.0f32, f32::max);
    let transition_end = step_at + (SR * 0.35) as usize;
    let max_jump = (step_at..transition_end)
        .map(|i| (y[i] - y[i - 1]).abs())
        .fold(0.0f32, f32::max);
    let margin = 0.05;
    assert!(
        max_jump <= steady_jump * 1.5 + margin,
        "stepped Length clicked: max jump {max_jump:.5} vs steady {steady_jump:.5} (+ {margin})"
    );

    // After 150 ms (500 ms impulse rebuild ~60 ms + <=10.7 ms block boundary + 15 ms crossfade),
    // the output must match a steady 500 ms reference convolution.
    let mut ref_k = convolve(&imp, SR, &[("lengthMs", 500.0)]);
    let ref_y = mono(&mut ref_k, &x);
    let check_from = step_at + (SR * 0.65) as usize; // past 150 ms rebuild + 500 ms tail settle
    let worst_err = (check_from..total)
        .map(|i| (y[i] - ref_y[i]).abs())
        .fold(0.0f32, f32::max);
    assert!(
        worst_err < 1e-4,
        "500 ms Length step did not land on the target impulse: worst err {worst_err}"
    );
}

#[test]
fn four_second_brighten_rebuild_lands_within_stated_bound() {
    let imp = burst(4.0, 29);
    let mut k = convolve(&imp, SR, &[("lengthMs", 100.0)]);
    let len_idx = k.descriptor().index_of("lengthMs").unwrap();
    let win_idx = k.descriptor().index_of("window").unwrap();
    let bri_idx = k.descriptor().index_of("brighten").unwrap();

    // Warm up, then step to the worst case: 4 s + Kaiser + Brighten.
    let mut l = vec![0.1f32; 64];
    let mut r = vec![-0.1f32; 64];
    for _ in 0..16 {
        k.process(&mut l, &mut r);
    }

    k.set_param(len_idx, 4000.0);
    k.set_param(win_idx, window("Kaiser"));
    k.set_param(bri_idx, 1.0);

    // Worst-case 4 s + Brighten + Kaiser build: ~1072 quanta @ 7/512 frames = 78,409 frames (~1.63 s)
    // + <=512 frames boundary + 720 frames crossfade <= 80,000 frames (~1.67 s).
    let mut settled_at = None;
    let mut blocks = 0usize;
    let mut pcts = Vec::new();
    let slot_s = 64.0 / SR as f64;
    let limit = (SR * 2.0) as usize;
    while blocks * 64 < limit {
        l.fill(0.1);
        r.fill(-0.1);
        let t0 = std::time::Instant::now();
        k.process(&mut l, &mut r);
        let dt = t0.elapsed().as_secs_f64();
        pcts.push(100.0 * dt / slot_s);
        blocks += 1;
        // Once the new 4 s brightened bank is active, latency_frames() switches from 512 to 1024.
        if settled_at.is_none() && k.latency_frames() == 1024 {
            settled_at = Some(blocks * 64);
            // Run 16 more blocks (2 partition boundaries) to include active 4 s convolution.
            for _ in 0..16 {
                l.fill(0.1);
                r.fill(-0.1);
                let t0 = std::time::Instant::now();
                k.process(&mut l, &mut r);
                pcts.push(100.0 * t0.elapsed().as_secs_f64() / slot_s);
            }
            break;
        }
    }
    let frames = settled_at.expect("4 s + Brighten + Kaiser rebuild did not activate within 2.0 s");
    let ms = frames as f64 / SR as f64 * 1000.0;
    pcts.sort_by(f64::total_cmp);
    let n = pcts.len();
    let med = pcts[(n - 1) / 2];
    let p99 = pcts[((n - 1) as f64 * 0.99) as usize];
    let worst = pcts[n - 1];
    println!(
        "4 s + Kaiser + Brighten worst-case landing: {frames} frames ({ms:.1} ms); 64-frame slot med={med:.2}% p99={p99:.2}% worst={worst:.2}%"
    );
    assert!(ms < 1800.0, "took too long: {ms:.1} ms");
}
