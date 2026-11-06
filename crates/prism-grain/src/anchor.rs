//! Where an output frame reads from in the source, kept continuous when the
//! ratio moves.
//!
//! The splice and noise streams find their read point from the output frame,
//! `frame / ratio`, every hop, so nothing accumulates and nothing drifts. The
//! price is that a new ratio moves that point at once by `frame · Δ(1/ratio)`.
//! Twenty seconds into a file, a stretch moving from ×1 to ×1.01 sends the read
//! two hundred milliseconds away. The stretcher used to answer that with a seek
//! and a fade in from silence, which is right once, but a stretch that moves
//! every block (a glide, a pad) became a seek and a fade every block, which is
//! heard as chopping and clicks.
//!
//! An anchor is the other answer. When the ratio changes, the offset takes up
//! the difference at the frame where it changed, so that frame reads where it
//! did, and from there the read moves at the new rate. The stream keeps its
//! ring, its window and its splices, and there is nothing to fade. A seek puts
//! the offset back to nothing, so seeking to a moment gives what it always
//! gave.

#[derive(Clone, Copy, Debug, Default)]
pub struct Anchor {
    /// Added to `frame / ratio`, in source frames.
    off: f64,
    /// The ratio the offset was last taken up at; zero before the first.
    ratio: f64,
}

impl Anchor {
    /// From nothing, as a seek leaves it.
    pub fn reset(&mut self, ratio: f32) {
        self.off = 0.0;
        self.ratio = ratio as f64;
    }

    /// The ratio is now `ratio`, from output frame `frame` on: take up the
    /// difference there.
    pub fn retune(&mut self, frame: u64, ratio: f32) {
        let r = ratio as f64;
        if self.ratio > 0.0 && r > 0.0 && (r - self.ratio).abs() > 1e-12 {
            self.off += frame as f64 / self.ratio - frame as f64 / r;
        }
        self.ratio = r;
    }

    /// Where `frame` reads from in the source at `ratio`.
    #[inline]
    pub fn source(&self, frame: f64, ratio: f32) -> f64 {
        frame / (ratio.max(1e-6) as f64) + self.off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_ratio_reads_on_from_where_it_was() {
        let mut a = Anchor::default();
        a.reset(1.0);
        assert_eq!(a.source(48_000.0, 1.0), 48_000.0);
        a.retune(48_000, 2.0);
        // The frame where it changed reads where it did...
        assert!((a.source(48_000.0, 2.0) - 48_000.0).abs() < 1e-9);
        // ...and from there at the new rate: half a frame of source a frame.
        assert!((a.source(49_000.0, 2.0) - 48_500.0).abs() < 1e-9);
        a.reset(2.0);
        assert_eq!(a.source(48_000.0, 2.0), 24_000.0);
    }

    #[test]
    fn the_first_ratio_and_an_unchanged_one_move_nothing() {
        let mut a = Anchor::default();
        a.retune(10_000, 1.5);
        assert_eq!(a.source(3_000.0, 1.5), 2_000.0);
        a.retune(20_000, 1.5);
        assert_eq!(a.source(3_000.0, 1.5), 2_000.0);
    }
}
