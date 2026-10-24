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
