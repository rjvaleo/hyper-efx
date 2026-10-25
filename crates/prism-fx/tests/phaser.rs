//! The Phaser renders what MultiGrainLive's `fx::phaser::Phaser` rendered,
//! sample for sample, and keeps its keys.

const ID: &str = "phaser";

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
fn it_keeps_multigrainlives_keys() {
    let d = prism_fx::find(ID).unwrap().descriptor;
    let keys: Vec<&str> = d.params.iter().map(|p| p.key).collect();
    assert_eq!(
        keys,
        ["notches", "centreHz", "spread", "resonance", "depth", "rateHz", "sweep", "stereo", "feedback", "wet", "dry"]
    );
    assert_eq!((d.label, d.code), ("Phaser", 99));
}

#[test]
fn a_still_notch_cuts_at_its_centre() {
    let level = |hz: f32| {
        let mut k = make(&[
            ("notches", 1.0),
            ("centreHz", 1_000.0),
            ("resonance", 0.99),
            ("rateHz", 0.0),
            ("sweep", 0.0),
            ("depth", 1.0),
            ("wet", 1.0),
            ("dry", 0.0),
        ]);
        let (l, _) = run(&mut k, &tone(16_384, hz, 0.5));
        l[8_192..].iter().fold(0f32, |m, s| m.max(s.abs()))
    };
    assert!(level(1_000.0) < 0.05, "{}", level(1_000.0));
    assert!(level(3_000.0) > 0.5, "{}", level(3_000.0));
}

#[test]
fn fully_dry_it_passes_the_sound_untouched() {
    let x = tone(4_096, 330.0, 0.5);
    let (l, _) = run(&mut make(&[("wet", 0.0), ("dry", 1.0)]), &x);
    assert_eq!(l, x);
}
