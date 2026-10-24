//! LXP Hall: Lexicon's LXP Native Hall (1.2.4), on this crate's own engine.
//!
//! The original's code is encrypted by its copy protection and was not read;
//! its controls, ranges in use, text, panel and presets were. See `lxp` for the
//! engine and MultiGrainLive's `docs/effects/lexicon.md` for what is the
//! original's and what is this crate's.
//!
//! The manual describes a hall as late and thin to start, building slowly,
//! with long lows. So here: the longest lines, one all-pass in each loop to
//! even the decay, and four reflections that can cross sides.
//!
//! The defaults are the factory preset "Med Hall" (Medium Halls), with the mix
//! at 35 % rather than its 100 %, for an insert.

use super::lxp::{self, Algorithm, Lxp, Role, Voicing};
use crate::{Category, Descriptor, Display, Group, PadPair, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    lxp::predelay(14.0),
    lxp::diffusion(0.97),
    lxp::shape(17.0),
    lxp::spread(0.51),
    lxp::size(35.0),
    lxp::reverb_time(1.6859),
    lxp::bass_rt(1.5),
    lxp::bass_xov(600.0),
    lxp::rt_hi_cut(4500.0),
    lxp::spin(2.9),
    lxp::wander(15.0),
    lxp::tap_slope(0.0),
    lxp::reflection_master(1.0),
    lxp::voice_delay("llRflMs", "L-L Rfl Dly", 22.0),
    lxp::level("llRflDb", "L-L Rfl Gain", -6.0),
    lxp::voice_delay("lrRflMs", "L-R Rfl Dly", 41.0),
    lxp::level("lrRflDb", "L-R Rfl Gain", -8.0),
    lxp::voice_delay("rrRflMs", "R-R Rfl Dly", 26.0),
    lxp::level("rrRflDb", "R-R Rfl Gain", -6.0),
    lxp::voice_delay("rlRflMs", "R-L Rfl Dly", 39.0),
    lxp::level("rlRflDb", "R-L Rfl Gain", -9.0),
    lxp::level("earlyDb", "Early Level", -1.5),
    lxp::level("reverbDb", "Rvb Level", -5.5),
    lxp::rolloff(10750.0),
    lxp::mix(0.35),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "lxp-hall",
    code: 18,
    version: 1,
    label: "LXP Hall",
    category: Category::Reverb,
    params: PARAMS,
    panel: Panel {
        // The original's edit pages, six knobs to a page; its soft row first.
        groups: &[
            Group { label: "Reverb", keys: lxp::REVERB_KEYS },
            Group { label: "Modulation", keys: lxp::MODULATION_KEYS },
            Group { label: "Reflections", keys: &["rflTimeMaster", "llRflMs", "llRflDb", "lrRflMs", "lrRflDb", "rrRflMs", "rrRflDb", "rlRflMs", "rlRflDb"] },
            Group { label: "Levels, Mix and EQ", keys: lxp::LEVELS_KEYS },
        ],
        primary: lxp::SOFT_ROW,
        displays: &[Display::Meter, Display::Spectrum],
        pad: Some(PadPair { x: "sizeM", x_range: (5.0, 60.0), y: "reverbTimeS", y_range: (0.3, 6.0) }),
    },
    source: "Lexicon: LXP Native Hall 1.2.4 — its controls and presets; the engine is this crate's own, the original's code being encrypted",
    sound: None,
};

pub struct Hall;

impl Algorithm for Hall {
    fn descriptor() -> &'static Descriptor {
        &DESCRIPTOR
    }

    const ROLES: &'static [Role] = &[
    Role::Predelay,
    Role::Diffusion,
    Role::Shape,
    Role::Spread,
    Role::Size,
    Role::ReverbTime,
    Role::BassRt,
    Role::BassXov,
    Role::RtHiCut,
    Role::Spin,
    Role::Wander,
    Role::TapSlope,
    Role::ReflectionMaster,
    Role::ReflectionDelay(0),
    Role::ReflectionGain(0),
    Role::ReflectionDelay(1),
    Role::ReflectionGain(1),
    Role::ReflectionDelay(2),
    Role::ReflectionGain(2),
    Role::ReflectionDelay(3),
    Role::ReflectionGain(3),
    Role::Early,
    Role::Reverb,
    Role::Rolloff,
    Role::Mix,
    ];

    const VOICING: Voicing = Voicing {
        lines: [0.707, 0.788, 0.859, 0.937, 1.031, 1.117, 1.203, 1.311],
        loop_allpasses: 1,
        loop_gain: 0.35,
        input_allpasses: 4,
        input_gain: 0.72,
        reflections: &[(0, 0), (0, 1), (1, 1), (1, 0)],
        echoes: false,
    };
}

pub type LxpHall = Lxp<Hall>;
