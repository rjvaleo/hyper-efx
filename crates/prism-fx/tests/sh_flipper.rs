//! ++flipper: SoundHack Delay Trio's reverser, as its code does it.

const SR: f32 = 48_000.0;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn flipper(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make("sh-flipper", SR).expect("sh-flipper is in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k.reset();
    k
}

fn run(k: &mut Boxed, x: &[f32]) -> Vec<f32> {
    let (mut l, mut r) = (x.to_vec(), x.to_vec());
    k.process(&mut l, &mut r);
    l
}

fn ramp(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32 / n as f32).collect()
}

fn jump(s: &[f32]) -> f32 {
    s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max)
}

#[test]
fn each_segment_plays_the_last_one_backwards() {
    // A flip comes once the count reaches the segment, a sample after it:
    // the first at 4800, and from then on the newest 4800 samples, reversed.
    let x = ramp(20_000);
    let y = run(&mut flipper(&[("flipS", 0.1), ("overlap", 0.0)]), &x);
    for k in 1..=4_800 {
        assert_eq!(y[4_800 + k], x[4_801 - k], "sample {}", 4_800 + k);
    }
}

#[test]
fn the_overlap_is_a_share_of_the_segment() {
    // The original faded over overlap × segment with overlap 0–100, so at its
    // default of 15 the new head never got past a fifteenth. Here 15 % of a
    // segment in, only the new head is heard.
    let x = ramp(20_000);
    let y = run(&mut flipper(&[("flipS", 0.1), ("overlap", 0.15)]), &x);
    for k in 800..4_800 {
        assert_eq!(y[4_800 + k], x[4_801 - k], "sample {}", 4_800 + k);
    }
}

#[test]
fn an_overlap_takes_the_click_out_of_the_seams() {
    // A seam joins samples two segments apart; 233 Hz is not whole cycles there.
    let x: Vec<f32> = (0..48_000).map(|i| 0.5 * (std::f32::consts::TAU * 233.0 * i as f32 / SR).sin()).collect();
    let hard = run(&mut flipper(&[("flipS", 0.1), ("overlap", 0.0)]), &x);
    let soft = run(&mut flipper(&[("flipS", 0.1), ("overlap", 0.15)]), &x);
    assert!(jump(&soft) < 0.5 * jump(&hard), "{} against {}", jump(&soft), jump(&hard));
    assert!(jump(&soft) < 3.0 * jump(&x), "{} against the input's {}", jump(&soft), jump(&x));
}

#[test]
fn nothing_passes_one_and_a_half() {
    let x = vec![1.0; 9_600];
    let y = run(&mut flipper(&[("inDb", 20.0), ("outDb", 20.0)]), &x);
    assert!(y.iter().all(|s| s.abs() <= 1.5));
}
