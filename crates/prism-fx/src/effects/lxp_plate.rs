//! LXP Plate: Lexicon's LXP Native Plate (1.2.4), on this crate's own engine.
//!
//! The original's code is encrypted by its copy protection and was not read;
//! its controls, ranges in use, text, panel and presets were. See `lxp` for the
//! engine and MultiGrainLive's `docs/effects/lexicon.md` for what is the
//! original's and what is this crate's.
//!
//! The manual has the plate dense from the start, bright and coloured. So
//! here: the shortest lines, two all-passes in each loop, and six input
//! all-passes that diffuse harder. Two reflections, one per side, and two
//! echoes that feed back into their side.
//!
//! The defaults are the factory preset "Small Rich Plate 2" (Short Plates),
//! with the mix at 35 % rather than its 100 %, for an insert.

use super::lxp::{self, Algorithm, Lxp, Role, Voicing};
use crate::{Category, Descriptor, Display, Group, PadPair, Panel, ParamSpec};

static PARAMS: &[ParamSpec] = &[
    lxp::predelay(0.0),
    lxp::diffusion(0.88),
    lxp::shape(7.0),
    lxp::spread(0.13),
    lxp::size(32.0),
    lxp::reverb_time(0.9664),
    lxp::bass_rt(0.2),
    lxp::bass_xov(95.0),
    lxp::rt_hi_cut(2750.0),
    lxp::spin(1.2),
    lxp::wander(5.7),
    lxp::tap_slope(0.0),
    lxp::reflection_master(1.0),
    lxp::voice_delay("lRflMs", "L Rfl Dly", 33.0),
    lxp::level("lRflDb", "L Rfl Gain", -6.0),
    lxp::voice_delay("rRflMs", "R Rfl Dly", 22.0),
    lxp::level("rRflDb", "R Rfl Gain", -6.0),
    lxp::echo_master("ekoTimeMaster", "Eko Time Mast", 1.0),
    lxp::echo_master("ekoFbMaster", "Eko Fbck Mast", 1.0),
    lxp::voice_delay("lEkoMs", "L Eko Dly", 89.0),
    lxp::polarity("lEkoPolarity", "L Eko FBack Polarity", 1.0),
    lxp::level("lEkoFbDb", "L Eko FBack", -25.0),
    lxp::voice_delay("rEkoMs", "R Eko Dly", 105.0),
    lxp::polarity("rEkoPolarity", "R Eko FBack Polarity", 1.0),
    lxp::level("rEkoFbDb", "R Eko FBack", -23.5),
    lxp::level("earlyDb", "Early Level", -3.5),
    lxp::level("reverbDb", "Rvb Level", -3.0),
    lxp::rolloff(3725.0),
    lxp::mix(0.35),
];

pub static DESCRIPTOR: Descriptor = Descriptor {
    id: "lxp-plate",
    code: 19,
    version: 1,
    label: "LXP Plate",
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
    source: "Lexicon: LXP Native Plate 1.2.4 — its controls and presets; the engine is this crate's own, the original's code being encrypted",
    sound: None,
};

pub struct Plate;

impl Algorithm for Plate {
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
        lines: [0.362, 0.403, 0.446, 0.482, 0.524, 0.571, 0.614, 0.676],
        loop_allpasses: 2,
        loop_gain: 0.6,
        input_allpasses: 6,
        input_gain: 0.75,
        reflections: &[(0, 0), (1, 1)],
        echoes: true,
    };
}

pub type LxpPlate = Lxp<Plate>;
