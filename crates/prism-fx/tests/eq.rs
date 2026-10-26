//! The EQ renders what MultiGrainLive's rack EQ (`fx::eq::Eq`) rendered,
//! sample for sample, and keeps the rack's keys.

const ID: &str = "eq";

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
    let mut want = vec!["band.0.freq".to_string(), "band.0.enabled".into(), "band.0.mode".into()];
    for b in 1..=6 {
        for f in ["freq", "q", "gainDb", "enabled", "mode"] {
            want.push(format!("band.{b}.{f}"));
        }
    }
    want.extend(["band.7.freq".to_string(), "band.7.enabled".into(), "band.7.mode".into()]);
    assert_eq!(keys, want);
    assert_eq!((d.label, d.code), ("EQ", 100));
}

#[test]
fn a_fresh_eq_is_exactly_transparent() {
    let x = tone(4_096, 330.0, 0.5);
    let (l, _) = run(&mut make(&[]), &x);
    assert_eq!(l, x);
}

#[test]
fn a_bell_lifts_its_band_by_its_gain() {
    let level = |gain: f32| {
        let (l, _) = run(&mut make(&[("band.2.gainDb", gain)]), &tone(24_000, 1_000.0, 0.25));
        20.0 * l[12_000..].iter().fold(0f32, |m, s| m.max(s.abs())).log10()
    };
    assert!((level(12.0) - level(0.0) - 12.0).abs() < 0.1, "{} {}", level(12.0), level(0.0));
}

#[test]
fn a_notch_cuts_as_deep_as_its_gain() {
    // The original's notch is the bell's formula.
    let level = |mode: f32| {
        let (l, _) = run(&mut make(&[("band.2.mode", mode), ("band.2.gainDb", -18.0)]), &tone(24_000, 1_000.0, 0.25));
        l[12_000..].iter().fold(0f32, |m, s| m.max(s.abs()))
    };
    assert_eq!(level(3.0), level(2.0));
}
