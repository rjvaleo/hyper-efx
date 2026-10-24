//! Hyperprism Flanger: a delay swept from zero to the depth on a raised
//! cosine, +0.25 feedback of the previous wet sample, half wet and half dry.

type Boxed = Box<dyn prism_fx::DynKernel>;

fn flanger(sr: f32, settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("hp-flanger", sr).expect("hp-flanger is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn run(k: &mut Boxed, x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut a, mut b) = (x.to_vec(), x.to_vec());
    k.process(&mut a, &mut b);
    (a, b)
}

fn click(n: usize, at: usize) -> Vec<f32> {
    let mut x = vec![0.0; n];
    x[at] = 1.0;
    x
}

/// The sum and the centre of what arrives in a window.
fn arrival(y: &[f32], from: usize, to: usize) -> (f32, f32) {
    let sum: f32 = y[from..to].iter().sum();
    let centre = (from..to).map(|i| i as f32 * y[i]).sum::<f32>() / sum;
    (sum, centre)
}

#[test]
fn at_the_bottom_of_the_sweep_the_delay_is_zero() {
    // The LFO starts at the bottom: the delay is 0, so the wet sound is this
    // sample and the click comes out whole (0.5 dry + 0.5 wet). Each later
    // sample is the feedback, a quarter of the previous wet, at half level.
    let mut k = flanger(44_100.0, &[("rateHz", 2.0), ("depth", 100.0)]);
    let (y, r) = run(&mut k, &click(8, 0));
    assert!((y[0] - 1.0).abs() < 1e-6, "{}", y[0]);
    assert!((y[1] - 0.125).abs() < 1e-4, "{}", y[1]);
    assert!((y[2] - 0.031_25).abs() < 1e-4, "{}", y[2]);
    assert_eq!(y, r, "both sides in phase");
}

#[test]
fn at_the_top_of_the_sweep_the_delay_is_the_depth_and_feeds_back_a_quarter() {
    // 2 Hz: the top is half a cycle in, at sample 11 025. A click 100 samples
    // before it comes back 100 samples later at half level, and once more
    // 101 samples after that (the loop is a sample longer than the delay) at
    // a quarter of that.
    let mut k = flanger(44_100.0, &[("rateHz", 2.0), ("depth", 100.0)]);
    let (y, _) = run(&mut k, &click(12_000, 10_925));
    assert!((y[10_925] - 0.5).abs() < 1e-6, "dry half: {}", y[10_925]);
    let (sum, centre) = arrival(&y, 11_000, 11_050);
    assert!((sum - 0.5).abs() < 1e-3, "echo level {sum}");
    assert!((centre - 11_025.0).abs() < 0.5, "echo at {centre}");
    let (sum, centre) = arrival(&y, 11_100, 11_150);
    assert!((sum - 0.125).abs() < 1e-3, "second echo level {sum}");
    assert!((centre - 11_126.0).abs() < 1.0, "second echo at {centre}");
}

#[test]
fn the_depth_is_in_samples_at_44_1_khz() {
    // At twice the rate the same depth is twice as many samples: the same time.
    let mut k = flanger(88_200.0, &[("rateHz", 2.0), ("depth", 100.0)]);
    let (y, _) = run(&mut k, &click(24_000, 21_850));
    let (sum, centre) = arrival(&y, 22_000, 22_100);
    assert!((sum - 0.5).abs() < 1e-3, "{sum}");
    assert!((centre - 22_050.0).abs() < 0.5, "{centre}");
}

#[test]
fn the_ranges_are_the_originals() {
    let d = prism_fx::find("hp-flanger").unwrap().descriptor;
    let rate = d.spec("rateHz").unwrap();
    let depth = d.spec("depth").unwrap();
    assert!(!rate.log && !depth.log);
    assert_eq!((rate.min, rate.max, rate.default), (0.0, 4.0, 2.0));
    assert_eq!((depth.min, depth.max, depth.default), (0.0, 100.0, 0.0));
    assert_eq!(d.label, "Hyperprism Flanger");
}
