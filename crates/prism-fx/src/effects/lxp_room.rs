//! LXP Room: Lexicon's LXP Native Room (1.2.4), on this crate's own engine.
//!
//! The original's code is encrypted by its copy protection and was not read;
//! its controls, ranges in use, text, panel and presets were. See `lxp` for the
//! engine and MultiGrainLive's `docs/effects/lexicon.md` for what is the
//! original's and what is this crate's.
//!
//! The manual has the Room as the Hall cut down for small spaces, with Size
//! setting how grainy it is. So here: the Hall's lines a little shorter, and no
//! all-passes in the loop, so a large room lets its reflections through and a
//! small one runs them together. Four reflections that can cross sides.
//!
//! The defaults are the factory preset "Medium Tiled Room" (Medium Spaces),
//! with the mix at 35 % rather than its 100 %, for an insert.

use super::lxp::{self, Algorithm, Lxp, Role, Voicing};
use crate::{Category, Descriptor, Display, Group, PadPair, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    lxp::predelay(14.0),
    lxp::diffusion(0.86),
    lxp::shape(29.0),
    lxp::spread(0.49),
    lxp::size(18.5),
    lxp::reverb_time(0.8688),
    lxp::bass_rt(1.5),
    lxp::bass_xov(385.0),
    lxp::rt_hi_cut(3450.0),
    lxp::spin(2.5),
    lxp::wander(7.0),
    lxp::tap_slope(0.0),
    lxp::reflection_master(1.0),
    lxp::voice_delay("llRflMs", "L-L Rfl Dly", 14.0),
    lxp::level("llRflDb", "L-L Rfl Gain", -2.5),
    lxp::voice_delay("lrRflMs", "L-R Rfl Dly", 33.0),
    lxp::level("lrRflDb", "L-R Rfl Gain", -7.0),
    lxp::voice_delay("rrRflMs", "R-R Rfl Dly", 17.0),
    lxp::level("rrRflDb", "R-R Rfl Gain", -1.5),
    lxp::voice_delay("rlRflMs", "R-L Rfl Dly", 37.0),
    lxp::level("rlRflDb", "R-L Rfl Gain", -6.0),
    lxp::level("earlyDb", "Early Level", -8.0),
    lxp::level("reverbDb", "Rvb Level", -4.5),
    lxp::rolloff(7250.0),
    lxp::mix(0.35),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "lxp-room",
    code: 20,
    version: 1,
    label: "LXP Room",
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
        pad: Some(PadPair { x: "sizeM", x_range: (3.0, 30.0), y: "reverbTimeS", y_range: (0.2, 2.0) }),
    },
    source: "Lexicon: LXP Native Room 1.2.4 — its controls and presets; the engine is this crate's own, the original's code being encrypted",
    sound: None,
};

pub struct Room;

impl Algorithm for Room {
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
        lines: [0.601, 0.670, 0.730, 0.796, 0.876, 0.949, 1.023, 1.114],
        loop_allpasses: 0,
        loop_gain: 0.0,
        input_allpasses: 4,
        input_gain: 0.7,
        reflections: &[(0, 0), (0, 1), (1, 1), (1, 0)],
        echoes: false,
    };
}

pub type LxpRoom = Lxp<Room>;
