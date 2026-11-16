//! Modules: eleven menu entries, each a mode selector over kernels.
//!
//! SUITE-PLAN §2.0. A mode is a catalogue kernel **by reference**: its own
//! `Kernel`, keys, laws and code, unchanged, so an old document opens to the
//! very same sound (held by `tests/suite.rs` against the PAR golden renders).
//! Suite and mode ids are the persistence contract: never renamed, only
//! appended. Mode labels are our own names; the original is credited in
//! `source` (decision D5).
//!
//! [`ModuleKernel`] runs one mode and switches to another with a 20 ms
//! equal-power crossfade, during which both run. The incoming kernel is built
//! by the host off the audio thread; the outgoing one is handed back through
//! [`ModuleKernel::take_retired`] so it is never dropped on the audio thread.

use crate::{Descriptor, DynKernel};

/// How exact a mode is (SUITE-PLAN, "Exactness classes"). The face shows it on
/// every mode, so nothing claims to be a model unless it is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exactness {
    /// Decoded and confirmed against captures of the original.
    Measured,
    /// Decoded from the binary, every constant confirmed, never measured.
    Static,
    /// An existing port; its verdict is COMPARE §1's.
    Port(Verdict),
    /// Built from a published paper, exact to the paper.
    Paper,
    /// Our own design. Named after an original only "in the style of".
    Ours,
    /// Part decoded: the decoded part exact, the rest ours and labelled so.
    Partial,
}

/// COMPARE §1's verdict on a port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Matches,
    Differs,
    /// Not yet compared stage by stage with its original.
    Unchecked,
}

/// Which face the interface draws for a suite (SUITE-PLAN §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceKind {
    Reverb,
    Spectral,
    Cassette,
    Delay,
    Modulation,
    Pitch,
    Dynamics,
    Saturation,
    Filter,
    Stereo,
    Utility,
}

/// One mode: a kernel, how exact it is, and where it comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mode {
    /// Persistence contract, e.g. "dattorro-plate".
    pub id: &'static str,
    /// Our name for it (D5).
    pub label: &'static str,
    /// A `CATALOG` id.
    pub kernel: &'static str,
    pub class: Exactness,
    /// False: an unlisted legacy mode. It opens, and is never offered.
    pub listed: bool,
    /// The revision new slots get (SUITE-PLAN §3.4). A slot with no rev is rev 1.
    pub rev: u16,
    /// The model, cited: the original product and the decode or paper.
    pub source: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Family {
    pub label: &'static str,
    pub modes: &'static [Mode],
}

/// One module in the rack's menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Suite {
    /// Persistence contract: documents store `suite:<id>`.
    pub id: &'static str,
    pub label: &'static str,
    pub families: &'static [Family],
    pub face: FaceKind,
}

impl Suite {
    pub fn modes(&self) -> impl Iterator<Item = &'static Mode> {
        self.families.iter().flat_map(|f| f.modes.iter())
    }

    /// Whether the menu shows this module: it has a listed mode.
    pub fn offered(&self) -> bool {
        self.modes().any(|m| m.listed)
    }

    pub fn mode(&self, id: &str) -> Option<&'static Mode> {
        self.modes().find(|m| m.id == id)
    }

    /// The family a mode is filed under.
    pub fn family_of(&self, mode_id: &str) -> Option<&'static Family> {
        self.families.iter().find(|f| f.modes.iter().any(|m| m.id == mode_id))
    }
}

const fn m(id: &'static str, label: &'static str, class: Exactness, source: &'static str) -> Mode {
    Mode { id, label, kernel: id, class, listed: true, rev: 1, source }
}

/// The same mode, kept only so old documents open.
const fn unlisted(mut mode: Mode) -> Mode {
    mode.listed = false;
    mode
}

use Exactness::{Measured, Ours, Paper, Port, Static};
use Verdict::{Differs, Matches, Unchecked};

const HP: &str = "Hyperprism (Arboretum Systems), as decoded in decoded/hyperprism.md";
const SH: &str = "SoundHack ++ effects (Tom Erbe), as decoded in decoded/soundhack-*.md";
const DAT: &str = "Jon Dattorro, \"Effect Design\" parts 1–3 (JAES 1997, 2002)";

/// The eleven modules, listing only kernels that exist (P1). New kernels are
/// appended as modes in later phases.
pub static SUITES: &[Suite] = &[
    Suite {
        id: "reverb",
        label: "Reverb",
        face: FaceKind::Reverb,
        families: &[
            Family {
                label: "Network",
                modes: &[
                    m("sm-gemini", "Mesh 16", Measured, "Valhalla Supermassive 1.3.0, mode \"Gemini\": decoded/valhalla-supermassive.md §10.1; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-hydra", "Mesh 8 + Diffuser", Measured, "Valhalla Supermassive 1.3.0, mode \"Hydra\": decoded/valhalla-supermassive.md §10.2; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-centaurus", "Quad Cascade", Measured, "Valhalla Supermassive 1.3.0, mode \"Centaurus\": decoded/valhalla-supermassive.md §10.3; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-sagittarius", "Stereo Cascade", Measured, "Valhalla Supermassive 1.3.0, mode \"Sagittarius\": decoded/valhalla-supermassive.md §10.4; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-great-annihilator", "Quad Loop + Diffusers", Measured, "Valhalla Supermassive 1.3.0, mode \"Great Annihilator\": decoded/valhalla-supermassive.md §10.5; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-andromeda", "Echo Loop + Diffusers", Measured, "Valhalla Supermassive 1.3.0, mode \"Andromeda\": decoded/valhalla-supermassive.md §10.6; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-lyra", "Mesh 4", Measured, "Valhalla Supermassive 1.3.0, mode \"Lyra\": decoded/valhalla-supermassive.md §10.7; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-capricorn", "Mesh 8", Measured, "Valhalla Supermassive 1.3.0, mode \"Capricorn\": decoded/valhalla-supermassive.md §10.8; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-triangulum", "Ping-Pong Diffuse", Measured, "Valhalla Supermassive 1.3.0, mode \"Triangulum\": decoded/valhalla-supermassive.md §10.9; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-lmc", "Householder Loop", Measured, "Valhalla Supermassive 1.3.0, mode \"Large Magellanic Cloud\": decoded/valhalla-supermassive.md §10.10; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-cirrus-major", "Filtered Mesh + Diffuser", Measured, "Valhalla Supermassive 1.3.0, mode \"Cirrus Major\": decoded/valhalla-supermassive.md §10.11; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                    m("sm-cirrus-minor", "Filtered Mesh 8", Measured, "Valhalla Supermassive 1.3.0, mode \"Cirrus Minor\": decoded/valhalla-supermassive.md §10.12; measured against the original (sources/reports/001-valhalla-measured.md S3, S5)"),
                ],
            },
            Family {
                label: "Classic",
                modes: &[
                    m("schroeder-reverb", "Schroeder", Paper, "M. R. Schroeder, \"Natural sounding artificial reverberation\" (1962)"),
                    m("moorer-reverb", "Moorer", Paper, "J. A. Moorer, \"About this reverberation business\" (1979)"),
                    m("dattorro-plate", "Plate", Paper, DAT),
                    m("freeverb", "Comb Hall", Static, "Jezar's Freeverb (2000, public domain), as copied bit for bit in Black Water Reverb: sources/reports/001-black-water-reverb.md"),
                    m("jcrev", "JC Reverb", Static, "Pluggo cverb~ (Cycling '74): the Schroeder/Chowning JCRev, sources/reports/003-pluggo-externals.md. Mono in and out; the stereo pair (one per channel) is ours"),
                ],
            },
            Family {
                label: "Hall & Room",
                modes: &[
                    m("hp-hall", "Large Hall", Port(Matches), HP),
                    m("hp-medium-room", "Medium Room", Port(Differs), HP),
                ],
            },
            Family {
                label: "Studio",
                modes: &[
                    m("lxp-chamber", "Chamber", Ours, "Our own design in the style of the Lexicon LXP (not a model of it)"),
                    m("lxp-hall", "Concert Hall", Ours, "Our own design in the style of the Lexicon LXP (not a model of it)"),
                    m("lxp-plate", "Studio Plate", Ours, "Our own design in the style of the Lexicon LXP (not a model of it)"),
                    m("lxp-room", "Studio Room", Ours, "Our own design in the style of the Lexicon LXP (not a model of it)"),
                ],
            },
            Family {
                label: "Ambient",
                modes: &[m("ambient-fdn", "Ambient FDN", Static, "Stone Voices Ambient Reverb 4.3, sources/reports/001-ambientreverb.md (static decode; ramps and EQ steps on a fixed 256-frame block are ours)")],
            },
            Family { label: "Impulse", modes: &[m("convolution", "Convolution", Port(Unchecked), "SoundHack convolution (Tom Erbe)")] },
            Family { label: "Parts", modes: &[m("allpass-diffuser", "All-pass Diffuser", Paper, DAT)] },
        ],
    },
    Suite {
        id: "spectral",
        label: "Spectral Suite",
        face: FaceKind::Spectral,
        families: &[Family {
            label: "SoundHack",
            modes: &[m("spectral-dynamics", "Spectral Dynamics", Port(Differs), "SoundHack Spectral Dynamics (Tom Erbe), sources/reports/003-soundhack.md §8")],
        }],
    },
    Suite { id: "cassette", label: "Cassette", face: FaceKind::Cassette, families: &[] },
    Suite {
        id: "delay",
        label: "Delay",
        face: FaceKind::Delay,
        families: &[
            Family {
                label: "Echo",
                modes: &[
                    m("echo", "Echo", Ours, "Our own echo"),
                    m("hp-echo", "Echo Walk", Port(Differs), HP),
                    m("hp-echotranz", "Echo Transfer", Port(Matches), HP),
                    m("sh-delay", "Long Delay", Port(Differs), SH),
                    unlisted(m("hp-single-delay", "Single Delay", Port(Differs), HP)),
                ],
            },
            Family {
                label: "Multitap",
                modes: &[m("hp-multi-delay", "Multi Tap", Port(Matches), HP), unlisted(m("multi-delay", "Multi Delay", Ours, "Our own design"))],
            },
            Family {
                label: "Reverse",
                modes: &[m("boomerang", "Boomerang", Ours, "Our own design"), m("sh-flipper", "Flipper", Port(Differs), SH)],
            },
        ],
    },
    Suite {
        id: "modulation",
        label: "Modulation",
        face: FaceKind::Modulation,
        families: &[
            Family {
                label: "Chorus",
                modes: &[
                    m("hp-chorus", "Twin-Tap Chorus", Port(Differs), HP),
                    m("white-chorus", "White Chorus", Ours, DAT),
                    m("doubler", "Doubler", Ours, DAT),
                ],
            },
            Family { label: "Flanger", modes: &[m("hp-flanger", "Short Flanger", Port(Matches), HP), m("flanger", "Flanger", Paper, DAT)] },
            Family {
                label: "Phaser",
                modes: &[
                    m("phaser", "Phaser", Paper, "J. O. Smith, \"An allpass approach to digital phasing and flanging\" (1984)"),
                    m("hp-phaser", "Sweep Phaser", Port(Differs), HP),
                    m("sh-phasor", "Eight-Stage Phaser", Port(Differs), SH),
                ],
            },
            Family {
                label: "Vibrato",
                modes: &[
                    m("hp-vibrato", "Pitch Vibrato", Port(Differs), HP),
                    m("vibrato", "Vibrato", Paper, DAT),
                    m("leslie", "Rotary", Ours, "Our own design on Dattorro's modulated delay"),
                ],
            },
            Family {
                label: "Amplitude",
                modes: &[
                    m("tremolo", "Tremolo", Ours, "Our own design"),
                    m("hp-tremolo", "Smooth Tremolo", Port(Differs), HP),
                    m("hp-auto-pan", "Auto-Pan", Port(Matches), HP),
                    m("hp-ring-modulator", "Ring Blend", Port(Differs), HP),
                    m("hp-frequency-modulator", "Frequency Mod", Port(Differs), HP),
                    m("ring", "Ring Sweep", Ours, "Our own design"),
                ],
            },
        ],
    },
    Suite {
        id: "pitch",
        label: "Pitch",
        face: FaceKind::Pitch,
        families: &[
            Family {
                label: "Shift",
                modes: &[
                    m("hp-pitch-shifter", "Pitch Shifter", Port(Matches), HP),
                    m("harmonizer", "Harmonizer", Ours, DAT),
                    m("detune", "Detune", Ours, DAT),
                    m("sh-pitchdelay", "Pitch Delay", Port(Differs), SH),
                ],
            },
            Family { label: "Motion", modes: &[m("doppler", "Doppler Line", Paper, DAT), m("hp-doppler", "Doppler Plane", Port(Differs), HP)] },
            Family { label: "Grain", modes: &[m("sh-bubbler", "Bubbler", Port(Differs), SH)] },
        ],
    },
    Suite {
        id: "dynamics",
        label: "Dynamics",
        face: FaceKind::Dynamics,
        families: &[Family {
            label: "Dynamics",
            modes: &[
                m("compressor", "Compressor", Ours, "Our own compressor"),
                m("hp-compressor", "Look-ahead Compressor", Port(Differs), HP),
                m("sh-compand", "Compander", Port(Differs), SH),
                m("maximizer", "Maximizer", Ours, "Our own design"),
                m("fit", "Amp Fit", Ours, "Our own design"),
                m("gate", "Gate", Ours, "Our own design"),
                m("hp-noise-gate", "Noise Gate", Port(Matches), HP),
            ],
        }],
    },
    Suite {
        id: "saturation",
        label: "Saturation",
        face: FaceKind::Saturation,
        families: &[
            Family { label: "Harmonics", modes: &[m("sh-chebyshev", "Chebyshev", Port(Differs), SH)] },
            Family {
                label: "Crush",
                modes: &[
                    m("hp-sonic-decimator", "Decimator", Port(Differs), HP),
                    m("sh-decimate", "Power Crush", Port(Differs), SH),
                    unlisted(m("decimator", "Floor Decimator", Ours, "Our own design")),
                ],
            },
        ],
    },
    Suite {
        id: "filter",
        label: "Filter",
        face: FaceKind::Filter,
        families: &[
            Family {
                label: "EQ & Filters",
                modes: &[
                    m("eq", "Parametric EQ", Ours, "Our own EQ"),
                    m("hp-low-pass", "Low Pass", Port(Differs), HP),
                    m("hp-high-pass", "High Pass", Port(Differs), HP),
                    m("hp-band-pass", "Band Pass", Port(Differs), HP),
                    m("hp-band-reject", "Band Reject", Port(Differs), HP),
                    m("hp-comb-filter", "Comb", Port(Differs), HP),
                    m("chamberlin", "State Variable", Ours, DAT),
                    m("rappify", "Rappify", Ours, "Our own design"),
                    m("dattorro-filter-bank", "Filter Bank", Paper, DAT),
                    m("dc", "DC Block", Ours, "Our own design"),
                    unlisted(m("dattorro-notch", "Notch", Paper, DAT)),
                    unlisted(m("dattorro-resonator", "Resonator", Paper, DAT)),
                    unlisted(m("regalia", "Regalia", Paper, DAT)),
                    unlisted(m("damping-filter", "Damping", Paper, DAT)),
                ],
            },
        ],
    },
    Suite {
        id: "stereo",
        label: "Stereo",
        face: FaceKind::Stereo,
        families: &[Family {
            label: "Stereo",
            modes: &[
                m("hp-pan", "Pan", Port(Matches), HP),
                m("hp-stereo-dynamics", "Stereo Dynamics", Port(Matches), HP),
                m("hp-more-stereo", "More Stereo", Port(Differs), HP),
                m("hp-quasi-stereo", "Quasi Stereo", Port(Matches), HP),
                m("hp-ms-matrix", "M/S Pan", Port(Matches), HP),
                m("hp-hyper-stereo", "Mid/Side Gain", Port(Matches), HP),
                m("sh-matrix", "M/S Matrix", Port(Differs), SH),
                unlisted(m("stereo-matrix", "Stereo Matrix", Ours, "Our own fold of four stereo tools")),
                unlisted(m("width", "Width", Ours, "Our own design")),
                unlisted(m("swap", "Swap", Ours, "Our own design")),
                unlisted(m("invert", "Invert", Ours, "Our own design")),
            ],
        }],
    },
    Suite {
        id: "utility",
        label: "Utility",
        face: FaceKind::Utility,
        families: &[
            Family { label: "Level", modes: &[m("gain", "Gain", Ours, "Our own design"), m("utility", "Utility Strip", Ours, "Our own design")] },
            Family {
                label: "Generators",
                modes: &[
                    m("pn-noise", "PN Noise", Paper, DAT),
                    m("pn-noise-eq", "Equalized PN", Paper, DAT),
                    m("single-bit-pn", "Single-bit PN", Paper, DAT),
                ],
            },
        ],
    },
];

pub fn find_suite(id: &str) -> Option<&'static Suite> {
    SUITES.iter().find(|s| s.id == id)
}

pub fn find_mode(suite: &str, mode: &str) -> Option<&'static Mode> {
    find_suite(suite)?.mode(mode)
}

/// The suite and mode a kernel is: how an old document's effect opens.
pub fn locate(kernel: &str) -> Option<(&'static Suite, &'static Mode)> {
    SUITES.iter().find_map(|s| s.modes().find(|m| m.kernel == kernel).map(|m| (s, m)))
}

/// The length of a mode switch's crossfade: 20 ms.
pub fn fade_frames(sample_rate: f32) -> u32 {
    (0.02 * sample_rate).round().max(1.0) as u32
}

/// The scratch block the incoming kernel renders into during a switch.
const SCRATCH: usize = 256;

/// A module in the rack: one mode running, and a switch to another.
pub struct ModuleKernel {
    mode: &'static Mode,
    cur: Box<dyn DynKernel>,
    /// The outgoing kernel while a switch fades it out.
    out: Option<Box<dyn DynKernel>>,
    pos: u32,
    len: u32,
    retired: Option<Box<dyn DynKernel>>,
    sl: Box<[f32; SCRATCH]>,
    sr: Box<[f32; SCRATCH]>,
}

impl ModuleKernel {
    /// Build a module running `mode`. Allocates: call it off the audio thread.
    pub fn new(mode: &'static Mode, sample_rate: f32) -> Option<Self> {
        let cur = (crate::find(mode.kernel)?.make)(sample_rate);
        Some(Self::with_kernel(mode, cur, sample_rate))
    }

    /// Build a module around a kernel already made for `mode`.
    pub fn with_kernel(mode: &'static Mode, kernel: Box<dyn DynKernel>, sample_rate: f32) -> Self {
        ModuleKernel {
            mode,
            cur: kernel,
            out: None,
            pos: 0,
            len: fade_frames(sample_rate),
            retired: None,
            sl: Box::new([0.0; SCRATCH]),
            sr: Box::new([0.0; SCRATCH]),
        }
    }

    pub fn mode(&self) -> &'static Mode {
        self.mode
    }

    /// The running kernel (the incoming one, during a switch).
    pub fn kernel(&self) -> &dyn DynKernel {
        self.cur.as_ref()
    }

    pub fn kernel_mut(&mut self) -> &mut dyn DynKernel {
        self.cur.as_mut()
    }

    pub fn switching(&self) -> bool {
        self.out.is_some()
    }

    /// Start a switch to `mode`, with `next` made for it (and set up with the
    /// slot's remembered values) off the audio thread. From here on, controls
    /// address the incoming mode. Allocates nothing. A switch while one runs,
    /// or while the last retired kernel has not been taken, is refused and
    /// `next` handed back, so nothing is ever dropped on the audio thread.
    pub fn begin_switch(&mut self, mode: &'static Mode, next: Box<dyn DynKernel>) -> Result<(), Box<dyn DynKernel>> {
        if self.out.is_some() || self.retired.is_some() || next.descriptor().id != mode.kernel {
            return Err(next);
        }
        self.out = Some(core::mem::replace(&mut self.cur, next));
        self.mode = mode;
        self.pos = 0;
        Ok(())
    }

    /// The kernel a finished switch let go of. Take it off the audio thread.
    pub fn take_retired(&mut self) -> Option<Box<dyn DynKernel>> {
        self.retired.take()
    }

    fn land(&mut self) {
        if let Some(old) = self.out.take() {
            self.retired = Some(old);
        }
    }

    /// Equal-power gains for fade frame `p`: (outgoing, incoming).
    fn gains(&self, p: u32) -> (f32, f32) {
        let t = (p as f32 + 1.0) / self.len as f32;
        let a = core::f32::consts::FRAC_PI_2 * t;
        (a.cos(), a.sin())
    }
}

impl DynKernel for ModuleKernel {
    fn descriptor(&self) -> &'static Descriptor {
        self.cur.descriptor()
    }
    fn set_param(&mut self, index: usize, value: f32) {
        self.cur.set_param(index, value)
    }
    fn param(&self, index: usize) -> f32 {
        self.cur.param(index)
    }
    fn reset(&mut self) {
        self.land();
        self.cur.reset();
    }
    fn tick(&mut self, frame: &mut [f32; 2]) {
        let Some(out) = self.out.as_mut() else {
            return self.cur.tick(frame);
        };
        let mut a = *frame;
        out.tick(&mut a);
        self.cur.tick(frame);
        let (go, gi) = self.gains(self.pos);
        frame[0] = go * a[0] + gi * frame[0];
        frame[1] = go * a[1] + gi * frame[1];
        self.pos += 1;
        if self.pos >= self.len {
            self.land();
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mut at = 0;
        while at < left.len() && self.out.is_some() {
            let n = (left.len() - at).min(SCRATCH).min((self.len - self.pos) as usize);
            let (l, r) = (&mut left[at..at + n], &mut right[at..at + n]);
            // the outgoing kernel renders a copy; the incoming one renders in place
            self.sl[..n].copy_from_slice(l);
            self.sr[..n].copy_from_slice(r);
            if let Some(out) = self.out.as_mut() {
                out.process(&mut self.sl[..n], &mut self.sr[..n]);
            }
            self.cur.process(l, r);
            for i in 0..n {
                let (go, gi) = self.gains(self.pos + i as u32);
                l[i] = go * self.sl[i] + gi * l[i];
                r[i] = go * self.sr[i] + gi * r[i];
            }
            self.pos += n as u32;
            if self.pos >= self.len {
                self.land();
            }
            at += n;
        }
        if at < left.len() {
            self.cur.process(&mut left[at..], &mut right[at..]);
        }
    }
    fn set_sound(&mut self, samples: &[f32], channels: usize, sample_rate: f32) {
        self.cur.set_sound(samples, channels, sample_rate)
    }
    fn latency_frames(&self) -> u32 {
        self.cur.latency_frames()
    }
    fn tail_seconds(&self) -> f32 {
        self.cur.tail_seconds()
    }
    fn has_mono(&self) -> bool {
        self.cur.has_mono()
    }
    fn tick_mono(&mut self, sample: &mut f32) {
        let Some(out) = self.out.as_mut() else {
            return self.cur.tick_mono(sample);
        };
        let mut a = *sample;
        out.tick_mono(&mut a);
        self.cur.tick_mono(sample);
        let (go, gi) = self.gains(self.pos);
        *sample = go * a + gi * *sample;
        self.pos += 1;
        if self.pos >= self.len {
            self.land();
        }
    }
    fn telemetry(&self) -> f32 {
        self.cur.telemetry()
    }
}
