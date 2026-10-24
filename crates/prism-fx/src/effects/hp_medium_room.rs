//! Hyperprism Medium Room Reverb (Hyperprism-PPC 1.2.1, EReverb process 1,
//! MediumRoomMono), as its code does it.
//!
//! The Hall's engine — one circular memory, pointers at fixed ages, allpasses
//! done in place between two ages (`t ← t − g·h; h ← h + g·t`) — laid out as
//! two halves inside one loop.
//!
//! - **Front end.** In = ½(L + R) through the one-pole
//!   `lpi = c·in + (1 − c)·lpi` (c = Brightness), written at age 0 **and**
//!   over age 6646. The loop's return, `lpf = fb·c·buf[13106] + (1 − c)·lpf`,
//!   is added in at age 1.
//! - **First half** (ages 0–6645), g as a share of Diffusion: 3→1541 (0.3)
//!   holding 4→369 (0.7) and 370→1345 (0.5), then 1742→3066 (0.5).
//! - **The join.** `buf[6646] += Feedback·buf[6645]`, with the Feedback as
//!   set, not its glide: the input goes into the second half directly and
//!   the first half's sound joins it scaled by Feedback.
//! - **Second half** (6646–13106): 6647→8356 (0.3) holding 6648→7078 (0.6).
//!   So the loop is 13104 samples (297 ms) with a gain of about Feedback².
//! - **Output.** `wetL = −0.5·t(1600) + 0.5·t(6007) − 0.5·t(8446) + 0.4·t(9318)`,
//!   `wetR = 0.5·t(1599) + 0.5·t(6000) + 0.5·t(8453) + 0.4·t(9345)`, then
//!   `out = (1 − mix)·dry + mix·wet` per side.
//! - **Smoothing.** Mix and the loop's Feedback glide once every 45 samples,
//!   the first step on the 45th, `s = a·target + (1 − a)·s` with the
//!   Smoothing setting's `a` (0.1 at the default 25 %). Diffusion and
//!   Brightness act at once, and applying either clears the two one-poles,
//!   as the settings window's apply does. A reset clears the memory and puts
//!   the glides on their targets.
//!
//! Defaults are the original's: Mix 50 %, Feedback 20 %, Diffusion 85 %,
//! Brightness 50 %. Mix and Feedback run over the pad window, 0–100 %;
//! Diffusion over its slider, 0–150 %.
//!
//! Changed from the original:
//! - **Allpass gains** are held at 0.99 or less. Diffusion's slider reaches
//!   150 %, which puts the 0.7 allpass at g = 1.05: no longer an allpass, and
//!   it grows without bound. Below 142 % nothing changes.
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
    id: "hp-medium-room",
    code: 37,
    version: 1,
    label: "Hyperprism Medium Room Reverb",
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
    source: "Hyperprism-PPC 1.2.1: Medium Room Reverb",
    sound: None,
};

pub struct HpMediumRoom(ReverbCore);

impl Kernel for HpMediumRoom {
    fn new(sample_rate: f32) -> Self {
        HpMediumRoom(ReverbCore::new(Room::Medium, sample_rate))
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
