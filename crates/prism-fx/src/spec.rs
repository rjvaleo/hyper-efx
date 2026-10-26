//! What an effect is, as data: its parameters and its panel.
//!
//! This is the one table both hosts read. MultiGrainLive serves it to its UI;
//! idMLab's TypeScript descriptors are generated from it. Keys and ids are the
//! persistence contract — documents and automation lanes store them — so they
//! are never renamed, and parameters are only ever appended, because a
//! parameter's index is its position in `params`.

/// How a parameter's value is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    /// Any value in range.
    Continuous,
    /// Whole numbers only.
    Int,
    /// Off (min) or on (max).
    Toggle,
    /// One of the labels, numbered from `min`. There must be exactly
    /// `max - min + 1` labels.
    Choice(&'static [&'static str]),
}

/// One control. The first seven fields are MultiGrainLive's `ParamSpec`, so a
/// host converts by copying.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamSpec {
    /// Stable across versions. This is what documents and lanes store.
    pub key: &'static str,
    /// What a person should see.
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// Sweep it logarithmically: anything in hertz or milliseconds.
    pub log: bool,
    /// What to write after the number.
    pub unit: &'static str,
    pub kind: ParamKind,
    /// How long the kernel takes to glide to a new value, so changes do not click.
    pub smooth_ms: f32,
    /// For a glide time: the control whose moves it sets the pace of. Such a
    /// control changes nothing while that one holds still, and hosts and the
    /// contract need to know that.
    pub glides: Option<&'static str>,
    /// For a control that only means something in one setting of another —
    /// a gate's reduction, when the type is Gate: that control and value.
    /// Hosts may dim it otherwise, as SoundHack swapped its labels.
    pub when: Option<(&'static str, f32)>,
    /// Kept only so the documents that store it still open: the original
    /// effect had this control and it did nothing. Hosts hide it, panels leave
    /// it out, and the contract holds it to changing nothing rather than to
    /// reaching the output.
    pub inert: bool,
}

impl ParamSpec {
    pub const fn new(key: &'static str, label: &'static str, min: f32, max: f32, default: f32) -> Self {
        ParamSpec {
            key,
            label,
            min,
            max,
            default,
            log: false,
            unit: "",
            kind: ParamKind::Continuous,
            smooth_ms: 5.0,
            glides: None,
            when: None,
            inert: false,
        }
    }

    pub const fn log(mut self) -> Self {
        self.log = true;
        self
    }

    pub const fn unit(mut self, unit: &'static str) -> Self {
        self.unit = unit;
        self
    }

    pub const fn kind(mut self, kind: ParamKind) -> Self {
        self.kind = kind;
        self
    }

    pub const fn smooth(mut self, ms: f32) -> Self {
        self.smooth_ms = ms;
        self
    }

    /// Mark this as acting only while `key` is at `value`.
    pub const fn when(mut self, key: &'static str, value: f32) -> Self {
        self.when = Some((key, value));
        self
    }

    /// Mark this as the glide time for moves of `key`.
    pub const fn glides(mut self, key: &'static str) -> Self {
        self.glides = Some(key);
        self
    }

    /// Mark this as a control the original had and never read.
    pub const fn inert(mut self) -> Self {
        self.inert = true;
        self
    }

    /// A value the effect can use: inside the range, whole if the kind says so,
    /// and the default for anything that is not a number.
    pub fn clamp(&self, value: f32) -> f32 {
        if !value.is_finite() {
            return self.default;
        }
        let v = value.clamp(self.min, self.max);
        match self.kind {
            ParamKind::Continuous => v,
            ParamKind::Int | ParamKind::Choice(_) => v.round(),
            ParamKind::Toggle => {
                if v >= (self.min + self.max) * 0.5 {
                    self.max
                } else {
                    self.min
                }
            }
        }
    }

    /// A value as 0–1 along the control's travel, logarithmic where the spec is.
    pub fn to_unit(&self, value: f32) -> f32 {
        let v = self.clamp(value);
        let u = if self.log {
            (v / self.min).ln() / (self.max / self.min).ln()
        } else {
            (v - self.min) / (self.max - self.min)
        };
        u.clamp(0.0, 1.0)
    }

    /// The inverse of `to_unit`, exact at both ends.
    pub fn from_unit(&self, unit: f32) -> f32 {
        let u = if unit.is_finite() { unit.clamp(0.0, 1.0) } else { self.to_unit(self.default) };
        if u <= 0.0 {
            return self.clamp(self.min);
        }
        if u >= 1.0 {
            return self.clamp(self.max);
        }
        let v = if self.log {
            self.min * (self.max / self.min).powf(u)
        } else {
            self.min + (self.max - self.min) * u
        };
        self.clamp(v)
    }
}

/// Where an effect is filed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Delay,
    Reverb,
    Modulation,
    Stereo,
    Filter,
    Spectral,
    Pitch,
    Distortion,
    Dynamics,
    Granular,
    Utility,
}

/// Controls that sit together on the panel, in reading order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Group {
    pub label: &'static str,
    pub keys: &'static [&'static str],
}

/// Something the panel draws that is not a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    /// Input and output level.
    Meter,
    /// A transfer or response curve.
    Curve,
    /// A waveform or Lissajous scope.
    Scope,
    /// A spectrum.
    Spectrum,
    /// A drawing of the effect's own structure.
    Diagram,
}

/// The Prism pad's default pair: which control each axis drives, over what part
/// of its range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadPair {
    pub x: &'static str,
    pub x_range: (f32, f32),
    pub y: &'static str,
    pub y_range: (f32, f32),
}

/// The shared interface: one vocabulary for every effect, with the grouping and
/// order taken from the original.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub groups: &'static [Group],
    /// The controls shown first, or alone in a compact panel.
    pub primary: &'static [&'static str],
    pub displays: &'static [Display],
    pub pad: Option<PadPair>,
}

/// An effect, as data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Descriptor {
    /// Stable across versions: documents store it.
    pub id: &'static str,
    /// idMLab's wire number. Appended, never reused.
    pub code: u16,
    pub version: u16,
    pub label: &'static str,
    pub category: Category,
    /// A parameter's index is its position here, so this list is append-only.
    pub params: &'static [ParamSpec],
    pub panel: Panel,
    /// Where the effect comes from: the original product and its process.
    pub source: &'static str,
    /// A sound the effect takes as well as its input — a convolution's
    /// impulse — handed over with `Kernel::set_sound`.
    pub sound: Option<SoundInput>,
}

/// A sound an effect takes, as the host should offer it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundInput {
    /// Stable: documents store which sound under it.
    pub key: &'static str,
    pub label: &'static str,
}

impl Descriptor {
    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.params.iter().position(|p| p.key == key)
    }

    pub fn spec(&self, key: &str) -> Option<&'static ParamSpec> {
        self.params.iter().find(|p| p.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversion_round_trips_and_is_exact_at_the_ends() {
        let hz = ParamSpec::new("hz", "Hz", 20.0, 20_000.0, 1000.0).log();
        assert_eq!(hz.from_unit(0.0), 20.0);
        assert_eq!(hz.from_unit(1.0), 20_000.0);
        for v in [20.0, 100.0, 1000.0, 9_999.0] {
            assert!((hz.from_unit(hz.to_unit(v)) - v).abs() / v < 1e-4);
        }
        let lin = ParamSpec::new("mix", "Mix", 0.0, 1.0, 0.5);
        assert_eq!(lin.to_unit(0.25), 0.25);
        assert_eq!(lin.from_unit(f32::NAN), 0.5);
    }

    #[test]
    fn kinds_round_and_snap() {
        let n = ParamSpec::new("n", "N", 1.0, 8.0, 4.0).kind(ParamKind::Int);
        assert_eq!(n.clamp(2.6), 3.0);
        let t = ParamSpec::new("t", "T", 0.0, 1.0, 0.0).kind(ParamKind::Toggle);
        assert_eq!(t.clamp(0.4), 0.0);
        assert_eq!(t.clamp(0.6), 1.0);
    }
}
