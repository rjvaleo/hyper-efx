//! The Moorer reverb renders what MultiGrainLive's `fx::reverb::Moorer`
//! rendered, sample for sample, and keeps its keys.

const ID: &str = "moorer-reverb";

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
        ["decay", "damping", "size", "predelayMs", "spread", "wet", "dry", "diffusion", "freeze"]
    );
    assert_eq!((d.label, d.code), ("Moorer reverb", 98));
}

#[test]
fn damping_makes_the_top_end_die_first() {
    // Moorer's reason for the low-pass in the loop.
    let late_brightness = |damping: f32| {
        let mut k = make(&[("wet", 1.0), ("dry", 0.0), ("decay", 0.85), ("damping", damping)]);
        let mut x = vec![0.0; 96_000];
        x[0] = 1.0;
        let (l, _) = run(&mut k, &x);
        let tail = &l[72_000..];
        tail.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>()
    };
    assert!(late_brightness(2.0) < late_brightness(0.0) * 0.9);
}

#[test]
fn the_two_sides_differ() {
    let mut k = make(&[("wet", 1.0), ("dry", 0.0)]);
    let x = tone(24_000, 440.0, 0.5);
    let (mut l, mut r) = (x.clone(), x);
    k.process(&mut l, &mut r);
    assert!(l.iter().zip(&r).any(|(a, b)| (a - b).abs() > 1e-3));
}
