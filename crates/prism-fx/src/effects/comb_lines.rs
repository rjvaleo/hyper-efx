//! The delay lines MultiGrainLive's comb-and-all-pass reverbs are built from:
//! a buffer and a write head, read some way back. Shared by
//! `schroeder_reverb` and `moorer_reverb`.

/// A circular buffer with its write head. Sized once, in `new`.
pub(crate) struct Line {
    pub buf: Vec<f32>,
    pub at: usize,
}

impl Line {
    /// At least two long, as the originals' were.
    pub fn new(len: usize) -> Self {
        Line { buf: vec![0.0; len.max(2)], at: 0 }
    }

    /// A line `ms` long at `sr`, truncated to whole samples, as the originals
    /// sized theirs.
    pub fn ms(ms: f32, sr: f32) -> Self {
        Line::new(((ms * 0.001 * sr) as usize).max(2))
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// The sample `back` frames before the write head; `back` is below the
    /// length.
    #[inline]
    pub fn read(&self, back: usize) -> f32 {
        let len = self.buf.len();
        self.buf[(self.at + len - back) % len]
    }

    /// Write at the head and move it on.
    #[inline]
    pub fn write(&mut self, x: f32) {
        self.buf[self.at] = x;
        self.at = (self.at + 1) % self.buf.len();
    }

    /// A plain delay: read `back`, then write `x`.
    #[inline]
    pub fn delay(&mut self, x: f32, back: usize) -> f32 {
        let out = self.read(back);
        self.write(x);
        out
    }

    /// Schroeder's all-pass in its one-multiply form, the whole line long.
    #[inline]
    pub fn allpass(&mut self, x: f32, g: f32) -> f32 {
        let z = self.buf[self.at];
        let y = z - g * x;
        self.write(x + g * y);
        y
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.at = 0;
    }
}

/// How long a comb bank keeps sounding: its longest loop repeated until it
/// has fallen 60 dB, after the predelay. Capped at a minute, and a minute
/// when frozen.
pub(crate) fn tail(longest_s: f32, g: f32, predelay_s: f32, frozen: bool) -> f32 {
    if frozen || g >= 1.0 {
        return 60.0;
    }
    if g <= 0.0 {
        return predelay_s + longest_s;
    }
    (predelay_s + longest_s * (0.001f32.ln() / g.ln())).min(60.0)
}
