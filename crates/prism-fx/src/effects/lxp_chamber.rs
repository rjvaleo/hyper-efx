//! LXP Chamber: Lexicon's LXP Native Chamber (1.2.4), on this crate's own
//! engine.
//!
//! The original's code is encrypted by its copy protection and was not read;
//! its controls, ranges in use, text, panel and presets were. See `lxp` for the
//! engine and MultiGrainLive's `docs/effects/lexicon.md` for what is the
//! original's and what is this crate's.
//!
//! The manual has the chamber dense quickly, with no wall slaps. So here:
//! shorter, unevenly spaced lines with two all-passes in each loop. Two
//! reflections, one per side, and two echoes that feed back into their side.
//!
//! The defaults are the factory preset "Medium Honey Vocal 2" (Medium
//! Chambers), with the mix at 35 % rather than its 100 %, for an insert.

use super::lxp::{self, Algorithm, Lxp, Role, Voicing};
use crate::{Category, Descriptor, Display, Group, PadPair, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    lxp::predelay(13.0),
    lxp::diffusion(0.12),
    lxp::shape(19.0),
    lxp::spread(0.69),
    lxp::size(50.5),
    lxp::reverb_time(1.8736),
    lxp::bass_rt(1.3),
    lxp::bass_xov(950.0),
    lxp::rt_hi_cut(4425.0),
    lxp::spin(1.6),
    lxp::wander(9.0),
    lxp::tap_slope(0.0),
    lxp::reflection_master(1.0),
    lxp::voice_delay("lRflMs", "L Rfl Dly", 62.0),
    lxp::level("lRflDb", "L Rfl Gain", 0.0),
    lxp::voice_delay("rRflMs", "R Rfl Dly", 50.0),
    lxp::level("rRflDb", "R Rfl Gain", 0.0),
    lxp::echo_master("ekoTimeMaster", "Eko Time Mast", 1.0),
    lxp::echo_master("ekoFbMaster", "Eko Fbck Mast", 0.75),
    lxp::voice_delay("lEkoMs", "L Eko Dly", 135.0),
    lxp::polarity("lEkoPolarity", "L Eko FBack Polarity", 1.0),
    lxp::level("lEkoFbDb", "L Eko FBack", -10.5),
    lxp::voice_delay("rEkoMs", "R Eko Dly", 117.0),
    lxp::polarity("rEkoPolarity", "R Eko FBack Polarity", 0.0),
    lxp::level("rEkoFbDb", "R Eko FBack", -10.5),
    lxp::level("earlyDb", "Early Level", -4.0),
    lxp::level("reverbDb", "Rvb Level", -5.5),
    lxp::rolloff(6375.0),
    lxp::mix(0.35),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "lxp-chamber",
    code: 17,
    version: 1,
    label: "LXP Chamber",
    category: Category::Reverb,
    params: PARAMS,
    panel: Panel {
        // The original's edit pages, six knobs to a page; its soft row first.
        groups: &[
            Group { label: "Reverb", keys: lxp::REVERB_KEYS },
            Group { label: "Modulation", keys: lxp::MODULATION_KEYS },
            Group { label: "Reflections", keys: &["rflTimeMaster", "lRflMs", "lRflDb", "rRflMs", "rRflDb"] },
            Group {
                label: "Echoes",
                keys: &[
                    "ekoTimeMaster",
                    "ekoFbMaster",
                    "lEkoMs",
                    "lEkoFbDb",
                    "lEkoPolarity",
                    "rEkoMs",
                    "rEkoFbDb",
                    "rEkoPolarity",
                ],
            },
            Group { label: "Levels, Mix and EQ", keys: lxp::LEVELS_KEYS },
        ],
        primary: lxp::SOFT_ROW,
        displays: &[Display::Meter, Display::Spectrum],
        pad: Some(PadPair { x: "sizeM", x_range: (5.0, 60.0), y: "reverbTimeS", y_range: (0.3, 6.0) }),
    },
    source: "Lexicon: LXP Native Chamber 1.2.4 — its controls and presets; the engine is this crate's own, the original's code being encrypted",
    sound: None,
};

pub struct Chamber;

impl Algorithm for Chamber {
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
    Role::EchoTimeMaster,
    Role::EchoFeedbackMaster,
    Role::EchoDelay(0),
    Role::EchoInvert(0),
    Role::EchoFeedback(0),
    Role::EchoDelay(1),
    Role::EchoInvert(1),
    Role::EchoFeedback(1),
    Role::Early,
    Role::Reverb,
    Role::Rolloff,
    Role::Mix,
    ];

    const VOICING: Voicing = Voicing {
        lines: [0.482, 0.526, 0.577, 0.640, 0.683, 0.748, 0.815, 0.872],
        loop_allpasses: 2,
        loop_gain: 0.5,
        input_allpasses: 4,
        input_gain: 0.7,
        reflections: &[(0, 0), (1, 1)],
        echoes: true,
    };
}

pub type LxpChamber = Lxp<Chamber>;
