//! The Compressor renders what MultiGrainLive's rack compressor
//! (`fx::comp::Compressor`) rendered, sample for sample, and keeps the rack's
//! keys.

const ID: &str = "compressor";

#[path = "support/mgl_golden.rs"]
mod golden;
#[path = "support/mgl_parity.rs"]
mod parity;

type Boxed = Box<dyn prism_fx::DynKernel>;

fn make(settings: &[(&str, f32)]) -> Boxed {
    let mut k = prism_fx::make(ID, 48_000.0).expect("in the catalogue");
    for &(key, value) in settings {
        let i = k.descriptor().index_of(key).unwrap_or_else(|| panic!("no control {key}"));
        k.set_param(i, value);
    }
    k
}

fn run(k: &mut Boxed, x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (x.to_vec(), x.iter().map(|s| -0.5 * s).collect::<Vec<_>>());
    k.process(&mut l, &mut r);
    (l, r)
}

fn tone(n: usize, hz: f32, amp: f32) -> Vec<f32> {
    (0..n).map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin()).collect()
}

#[test]
fn it_renders_what_multigrainlive_rendered() {
    // Bit for bit in practice; 1e-5 is the bound the port promises.
    let worst = parity::check(ID, 1e-5);
    println!("{ID}: largest difference {worst:e}");
}

#[test]
fn it_keeps_the_racks_keys() {
    let d = prism_fx::find(ID).unwrap().descriptor;
    let keys: Vec<&str> = d.params.iter().map(|p| p.key).collect();
    assert_eq!(keys, ["thresholdDb", "ratio", "attackMs", "releaseMs", "kneeDb", "makeupDb"]);
    assert_eq!((d.label, d.code), ("Compressor", 101));
}

#[test]
fn below_the_threshold_it_passes_the_sound_untouched() {
    // Above it, even 1:1 rounds the level through the curve, as the original
    // did, so it is a millionth or so away from untouched there.
    let x = tone(4_096, 330.0, 0.9);
    let (l, _) = run(&mut make(&[("thresholdDb", 0.0), ("kneeDb", 0.0)]), &x);
    assert_eq!(l, x);
}

#[test]
fn four_to_one_quarters_the_level_over_the_threshold() {
    // 12 dB over a −18 dB threshold settles 3 dB over, hard-kneed.
    let (l, _) = run(
        &mut make(&[("ratio", 4.0), ("kneeDb", 0.0), ("attackMs", 1.0), ("releaseMs", 3_000.0)]),
        &tone(48_000, 220.0, 10f32.powf(-6.0 / 20.0)),
    );
    let peak = 20.0 * l[24_000..].iter().fold(0f32, |m, s| m.max(s.abs())).log10();
    assert!((peak - -15.0).abs() < 0.5, "{peak}");
}

#[test]
fn both_sides_take_the_louder_sides_gain() {
    let mut k = make(&[("ratio", 20.0), ("thresholdDb", -40.0)]);
    let loud = tone(24_000, 220.0, 0.5);
    let mut quiet: Vec<f32> = loud.iter().map(|s| s * 0.01).collect();
    let mut l = loud.clone();
    k.process(&mut l, &mut quiet);
    for i in 12_000..24_000 {
        if loud[i].abs() > 0.1 {
            assert!(((l[i] / loud[i]) - (quiet[i] / (loud[i] * 0.01))).abs() < 1e-4);
        }
    }
}
