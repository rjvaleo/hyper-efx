//! Hyperprism Compressor (ECompressor, process 36), as its code does it: a
//! 1024-frame look-ahead of two half-overlapped sin²-windowed blocks, a peak
//! envelope updated once a block, a hard knee at infinite ratio.

const SR: f32 = 44_100.0;
const BLOCK: usize = 1_024;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn compressor_at(sr: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-compressor", sr).expect("hp-compressor is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn compressor(settings: &[(&str, f32)]) -> Boxed {
    compressor_at(SR, settings)
}

fn run(k: &mut Boxed, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (l.to_vec(), r.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

fn sine(amp: f32, n: usize) -> Vec<f32> {
    (0..n).map(|i| amp * (std::f32::consts::TAU * 220.0 * i as f32 / SR).sin()).collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// Hyperprism's coefficient for `t` ms at the block rate SR / 1024, r = ½.
fn coefficient(t: f64) -> f32 {
    if t <= 0.0 {
        return 1.0;
    }
    let fs = (SR * 0.000_976_562_5) as f64;
    let w = (2.0 * std::f64::consts::PI / (0.001 * t * fs)).min(std::f64::consts::PI);
    let r2 = 0.25;
    let a = 1.0 - r2;
    let b = r2 * -2.0 * (1.0 + w.cos());
    let c = r2 * -2.0 * (1.0 - w.cos());
    (2.0 * c / (b - (b * b - 4.0 * a * c).sqrt())) as f32
}

fn level(db: f32) -> f32 {
    10f64.powf(0.05 * db as f64) as f32
}

#[test]
fn at_its_start_values_it_is_the_input_1024_frames_late() {
    let x = sine(0.9, 8_192);
    let y: Vec<f32> = x.iter().map(|s| -0.5 * s).collect();
    let mut k = compressor(&[]);
    assert_eq!(k.latency_frames(), BLOCK as u32);
    let (l, r) = run(&mut k, &x, &y);
    assert!(l[..BLOCK].iter().chain(&r[..BLOCK]).all(|s| *s == 0.0));
    for i in BLOCK..x.len() {
        assert!((l[i] - x[i - BLOCK]).abs() < 1e-6 && (r[i] - y[i - BLOCK]).abs() < 1e-6, "frame {i}");
    }
}

#[test]
fn the_look_ahead_scales_with_the_sample_rate() {
    assert_eq!(compressor_at(88_200.0, &[]).latency_frames(), 2_048);
    assert_eq!(compressor_at(22_050.0, &[]).latency_frames(), 512);
}

#[test]
fn below_the_threshold_the_gain_is_volume_over_compression() {
    // −30 dBFS under a −20 dB threshold, volume −6: 14 dB of makeup.
    let x = sine(level(-30.0), 44_100);
    let (l, _) = run(&mut compressor(&[("volumeDb", -6.0), ("compressionDb", -20.0)]), &x, &x);
    assert!((db(peak(&l[22_050..])) - -16.0).abs() < 0.05, "{}", db(peak(&l[22_050..])));
}

#[test]
fn above_the_threshold_the_ratio_is_infinite() {
    // The peak envelope is held at the volume, whatever the input level.
    for input in [-12.0, -3.0] {
        let x = sine(level(input), 88_200);
        let (l, _) = run(&mut compressor(&[("volumeDb", -10.0), ("compressionDb", -30.0), ("attackMs", 0.0)]), &x, &x);
        assert!((db(peak(&l[44_100..])) - -10.0).abs() < 0.3, "{input}: {}", db(peak(&l[44_100..])));
    }
}

#[test]
fn the_envelope_moves_once_a_block_by_the_block_rate_coefficient() {
    // A steady level c: every block's windowed peak is c (the window is 1 at
    // its middle). Block A updates at 1024, 2048 …, block B at 512, 1536 …;
    // the output is c × the mean of the two gains V / env.
    let c = 0.5f32;
    let (v, t) = (level(-40.0), level(-40.0));
    let attack = 200.0f32;
    let a = coefficient(attack as f64);
    let x = vec![c; 6 * BLOCK];
    let (l, _) = run(&mut compressor(&[("volumeDb", -40.0), ("compressionDb", -40.0), ("attackMs", attack)]), &x, &x);
    let mut env = 0.0f32;
    let mut envs = Vec::new();
    for _ in 0..6 {
        env = c * a + env * (1.0 - a);
        envs.push(env);
    }
    let g = |e: f32| if e > t { (v / t) * t / e } else { v / t };
    // Frames 1024–1535: A has updated once, B once. 1536–2047: B twice.
    let expect = |na: usize, nb: usize| c * 0.5 * (g(envs[na - 1]) + g(envs[nb - 1]));
    assert!((l[1_100] / expect(1, 1) - 1.0).abs() < 1e-4, "{} vs {}", l[1_100], expect(1, 1));
    assert!((l[1_600] / expect(1, 2) - 1.0).abs() < 1e-4, "{} vs {}", l[1_600], expect(1, 2));
    assert!((l[2_100] / expect(2, 2) - 1.0).abs() < 1e-4, "{} vs {}", l[2_100], expect(2, 2));
    assert!((l[5_000] / expect(4, 5) - 1.0).abs() < 1e-4, "{} vs {}", l[5_000], expect(4, 5));
    // Between updates the gain holds.
    assert!((l[1_100] / l[1_500] - 1.0).abs() < 1e-6);
}

#[test]
fn the_release_follows_its_own_coefficient() {
    // After a loud stretch at 0.5 the envelope falls toward the quiet 0.01 by
    // the release coefficient, once a block: env_k = 0.01 + 0.49 (1 − r)^k.
    let release = 500.0f32;
    let r = coefficient(release as f64);
    let mut x = vec![0.5f32; 8 * BLOCK];
    x.extend(vec![0.01f32; 8 * BLOCK]);
    let settings = [("volumeDb", -20.0), ("compressionDb", -60.0), ("attackMs", 0.0), ("releaseMs", release)];
    let (l, _) = run(&mut compressor(&settings), &x, &x);
    let g = |k: i32| level(-20.0) / (0.01 + 0.49 * (1.0 - r).powi(k));
    // Output frame 10340: A has had two quiet blocks and B one; 11364: three
    // and two.
    let expect = (g(3) + g(2)) / (g(2) + g(1));
    let got = l[11 * BLOCK + 100] / l[10 * BLOCK + 100];
    assert!((got / expect - 1.0).abs() < 1e-4, "{got} vs {expect}");
}

#[test]
fn short_times_overshoot_as_the_original_does() {
    // Under about 46 ms the coefficient is 1.1547: the envelope overshoots
    // the peak on its first block. Kept: it is the original's formula.
    assert!((coefficient(10.0) - 1.154_700_5).abs() < 1e-6);
    let c = 0.5f32;
    let v = level(-40.0);
    let x = vec![c; 3 * BLOCK];
    let (l, _) = run(&mut compressor(&[("volumeDb", -40.0), ("compressionDb", -40.0), ("attackMs", 10.0)]), &x, &x);
    let env = c * coefficient(10.0);
    assert!((l[1_100] / (c * v / env) - 1.0).abs() < 1e-4, "{}", l[1_100]);
}

#[test]
fn the_look_ahead_catches_an_onset() {
    // 30 dB over the threshold from silence: the first loud output frame is
    // already turned down, because its block's peak was known in advance.
    let mut x = vec![0.0f32; 4 * BLOCK + 300];
    x.extend(sine(level(-6.0), 4 * BLOCK));
    let (l, _) = run(&mut compressor(&[("volumeDb", -36.0), ("compressionDb", -36.0), ("attackMs", 0.0)]), &x, &x);
    let onset = 4 * BLOCK + 300 + BLOCK;
    // Without the look-ahead it would come out at −6 dB.
    assert!(db(peak(&l[onset..onset + 64])) < -26.0, "{}", db(peak(&l[onset..onset + 64])));
}

#[test]
fn a_mono_signal_has_no_dip_at_the_start() {
    // The original's mono path started ring B out of step and lost half the
    // window sum for its first 1536 samples. Equal sides here take the
    // stereo path: from the first output frame the level is the input's.
    let x = vec![0.25f32; 4 * BLOCK];
    let (l, r) = run(&mut compressor(&[]), &x, &x);
    for i in BLOCK..x.len() {
        assert!((l[i] - 0.25).abs() < 1e-6 && (r[i] - 0.25).abs() < 1e-6, "frame {i}: {}", l[i]);
    }
}
