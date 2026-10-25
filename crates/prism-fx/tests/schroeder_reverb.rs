//! The Schroeder reverb renders what MultiGrainLive's `fx::reverb::Schroeder`
//! rendered, sample for sample, and keeps its keys.

const ID: &str = "schroeder-reverb";

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
    assert_eq!(keys, ["decay", "size", "diffusion", "wet", "dry", "spread", "predelayMs", "freeze"]);
    assert_eq!((d.label, d.code), ("Schroeder reverb", 97));
}

#[test]
fn fully_dry_it_passes_the_sound_untouched() {
    let x = tone(4_096, 330.0, 0.5);
    let (l, _) = run(&mut make(&[("wet", 0.0), ("dry", 1.0)]), &x);
    assert_eq!(l, x);
}

#[test]
fn frozen_it_sustains_without_growing() {
    let mut k = make(&[("wet", 1.0), ("dry", 0.0)]);
    run(&mut k, &tone(48_000, 220.0, 0.5));
    let i = k.descriptor().index_of("freeze").unwrap();
    k.set_param(i, 1.0);
    let peak = |k: &mut Boxed| run(k, &vec![0.0; 48_000]).0.iter().fold(0f32, |m, s| m.max(s.abs()));
    let first = peak(&mut k);
    for _ in 0..5 {
        peak(&mut k);
    }
    let last = peak(&mut k);
    assert!(first > 1e-3 && last > first * 0.25 && last < first * 4.0, "{first} then {last}");
}
