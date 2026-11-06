//! Hyperprism Hall Reverb (Hyperprism-PPC 1.2.1, EReverb process 0,
//! LargeHallMono, "Reverb L. Hall"), as its code does it.
//!
//! One circular memory and 25 pointers into it, each a fixed number of
//! samples — an age — behind the write point. Series and nested allpasses
//! are done in place between two ages (`t ← t − g·h; h ← h + g·t`, so
//! `(z⁻ᵈ − g)/(1 − g·z⁻ᵈ)` with d the span), all inside one feedback loop.
//!
//! - **Front end.** In = ½(L + R). A one-pole `lpi = c·in + (1 − c)·lpi`
//!   (c = Brightness) is written at age 0. The loop's return is
//!   `lpf = fb·c·buf[12420] + (1 − c)·lpf` — the same one-pole, times
//!   Feedback — added in at age 1. So the loop is 12419 samples (281.6 ms).
//! - **Allpasses**, in order, g as a share of Diffusion: 3→334 (0.3), 335→846
//!   (0.3), 1768→5623 (0.5) holding 1769→4501 (0.25), and 7121→12413 (0.5)
//!   holding 7122→10455 and 10456→11779 (0.25 each).
//! - **Output.** `wetL = 0.6·t(851) − 0.2·t(7001) + 0.1·t(12415)`,
//!   `wetR = −0.6·t(1002) + 0.2·t(657) − 0.1·t(12421)`, then
//!   `out = (1 − mix)·dry + mix·wet` per side. Two more pointers the code sets
//!   up (10379, 10409) are never read, so they are left out.
//! - **Smoothing.** Mix and Feedback glide once every 45 samples, the first
//!   step on the 45th, `s = a·target + (1 − a)·s` with the Smoothing setting's
//!   `a` (0.1 at the default 25 %). Diffusion and Brightness act at once, and
//!   applying either clears the two one-poles, as the settings window's
//!   apply does. A reset clears the memory and puts the glides on their
//!   targets.
//!
//! Defaults are the original's: Mix 50 %, Feedback 20 % (the pad's start
//! point), Diffusion 85 % and Brightness 50 % (its constructor; the code's
//! later 0.6 for Brightness is overwritten before it is used). Mix and
//! Feedback run over the pad window, 0–100 %; Diffusion over its slider,
//! 0–150 %.
//!
//! Changed from the original:
//! - **Feedback** stops at 100 %. The original let a window be typed out to
//!   2047 %, where the loop grows without bound.
//! - **Rate.** The original's delays are fixed sample counts at 44.1 kHz. At
//!   another rate every age is scaled by sr/44100 and rounded, keeping the
//!   one- and two-sample joins between stages as they are; the 45-sample step
//!   is scaled the same way, and the one-pole keeps its frequency
//!   (`1 − c` raised to 44100/sr). At 44.1 kHz all of it is the original's.
//!
//! Added (not in the original):
//! - **Smoothing step**, 1–2048 samples at 44.1 kHz (scaled with the rate as
//!   the 45 is), default 45: how often Mix and Feedback glide a step. The
//!   default is the original exactly.
//! - **Size**, 25–200 %, default 100 %: every fixed age — the loop,
//!   the allpass spans and the output taps — scaled together, as the rate
//!   scales them and with the same one- and two-sample joins kept. The loop
//!   is 12419 samples (281.6 ms) at 100 %; the echo spacing and the
//!   decay time grow or shrink with it. The memory is made for 200 %, so a
//!   change never allocates. The default is the original exactly.

use super::hp_fr::{ReverbCore, Room, REVERB_PARAMS};
use crate::{Category, Descriptor, Display, Group, Kernel, PadPair, Panel};

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "hp-hall",
    code: 38,
    version: 1,
    label: "Hyperprism Hall Reverb",
    category: Category::Reverb,
    params: REVERB_PARAMS,
    panel: Panel {
        groups: &[
            Group { label: "Reverb", keys: &["mix", "feedback"] },
            Group { label: "Settings", keys: &["diffusion", "brightness", "smoothing"] },
            Group { label: "Added (not in the original)", keys: &["size", "smoothingStep"] },
        ],
        primary: &["mix", "feedback"],
        displays: &[Display::Meter],
        pad: Some(PadPair { x: "mix", x_range: (0.0, 100.0), y: "feedback", y_range: (0.0, 100.0) }),
    },
    source: "Hyperprism-PPC 1.2.1: Hall Reverb",
    sound: None,
};

pub struct HpHall(ReverbCore);

impl Kernel for HpHall {
    fn new(sample_rate: f32) -> Self {
        HpHall(ReverbCore::new(Room::Hall, sample_rate))
    }

    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    fn set_param(&mut self, index: usize, value: f32) {
        self.0.set_param(index, value)
    }

    fn param(&self, index: usize) -> f32 {
        self.0.param(index)
    }

    fn reset(&mut self) {
        self.0.reset()
    }

    fn tick(&mut self, frame: &mut [f32; 2]) {
        self.0.tick(frame)
    }

    fn tail_seconds(&self) -> f32 {
        self.0.tail_seconds()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::hp_fr::render::checksum;

    /// The renders before any control was added, at the defaults and at two
    /// other settings, one at 48 kHz: the added controls at their defaults
    /// leave every bit as it was.
    #[test]
    fn defaults_render_as_the_original() {
        let got = [
            checksum::<HpHall>(44_100.0, 3.0, &[], &[]),
            checksum::<HpHall>(
                44_100.0,
                3.0,
                &[("mix", 70.0), ("feedback", 80.0), ("diffusion", 120.0), ("brightness", 30.0)],
                &[("feedback", 40.0)],
            ),
            checksum::<HpHall>(
                48_000.0,
                3.0,
                &[("mix", 100.0), ("feedback", 60.0), ("smoothing", 60.0)],
                &[("mix", 30.0)],
            ),
        ];
        assert_eq!(got, [13951521947965946310, 14004291059576815122, 17283648987697829808]);
    }

    #[test]
    fn the_smoothing_step_paces_the_glide() {
        crate::effects::hp_fr::render::assert_step_paces_the_glide::<HpHall>(&[("mix", 20.0)], &[("mix", 90.0)]);
    }

    const LOOP: usize = 12_419;

    /// The impulse response's energy centre, in samples, at a Size.
    fn centre(size: f32) -> f64 {
        let mut k = HpHall::new(44_100.0);
        let d = HpHall::descriptor();
        for (key, v) in [("mix", 100.0), ("feedback", 70.0), ("size", size)] {
            k.set_param(d.index_of(key).unwrap(), v);
        }
        k.reset();
        let (mut energy, mut moment) = (0.0, 0.0);
        for i in 0..4 * 44_100 {
            let mut frame = if i == 0 { [1.0, 1.0] } else { [0.0, 0.0] };
            k.tick(&mut frame);
            let e = f64::from(frame[0] * frame[0] + frame[1] * frame[1]);
            energy += e;
            moment += e * i as f64;
        }
        moment / energy
    }

    #[test]
    fn size_scales_the_echo_spacing_and_the_decay() {
        let (half, whole, double) = (centre(50.0), centre(100.0), centre(200.0));
        assert!((double / whole - 2.0).abs() < 0.15, "200 %: {double} against {whole}");
        assert!((half / whole - 0.5).abs() < 0.08, "50 %: {half} against {whole}");
        // The tail it reports moves with it.
        let tail = |size: f32| {
            let mut k = HpHall::new(44_100.0);
            k.set_param(HpHall::descriptor().index_of("size").unwrap(), size);
            k.tail_seconds()
        };
        assert!((tail(200.0) / tail(100.0) - 2.0).abs() < 1e-3);
    }

    #[test]
    fn size_moves_the_loop_return() {
        // Wet only, no diffusion: the loop's first return lands one loop
        // length after the click, scaled by Size.
        let first_return = |size: f32| {
            let mut k = HpHall::new(44_100.0);
            let d = HpHall::descriptor();
            let settings = [("mix", 100.0), ("feedback", 100.0), ("diffusion", 0.0), ("brightness", 100.0), ("size", size)];
            for (key, v) in settings {
                k.set_param(d.index_of(key).unwrap(), v);
            }
            k.reset();
            let out: Vec<f32> = (0..3 * 44_100)
                .map(|i| {
                    let mut frame = if i == 0 { [1.0, 1.0] } else { [0.0, 0.0] };
                    k.tick(&mut frame);
                    frame[0].abs()
                })
                .collect();
            // The loudest output in the second pass round the loop.
            let lo = (LOOP as f32 * size / 100.0 * 0.9) as usize;
            let hi = (LOOP as f32 * size / 100.0 * 1.9) as usize;
            (lo..hi).max_by(|&a, &b| out[a].total_cmp(&out[b])).unwrap()
        };
        let (whole, double) = (first_return(100.0), first_return(200.0));
        assert!(whole > LOOP / 2 && (double as f64 / whole as f64 - 2.0).abs() < 0.01, "{whole} {double}");
    }
}
