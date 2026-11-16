//! Place: where a reverb's sound sits. Our own design (SUITE-PLAN P4, the
//! Reverb's Network face), a stage around a mode, never inside it.
//!
//! **Before the mode** it renders the input as heard from a place in a room:
//! - the room is a floor plan in one of six Shapes, Room size metres across,
//!   with a ceiling the Shape sets;
//! - you stand near the front; the sound is where Position (left/right),
//!   Distance and Height put it;
//! - the direct sound arrives after its path at the speed of sound, panned by
//!   where it comes from, darker with distance (air) and brighter the higher
//!   it is (a height cue);
//! - one first reflection from every wall, the floor and the ceiling, found by
//!   mirroring the sound in each surface, each arriving after its own path from
//!   its own direction, a little darker (the surface absorbs the top);
//! - the levels are shared so their energy is always one: moving the sound
//!   changes the balance of direct and reflected sound, not the loudness of
//!   what the mode is fed.
//!
//! **After the mode**, Spread scales the side of its output (100 % is as it
//! was).
//!
//! **Off, it is nothing at all**: the mode's output is bit for bit what it is
//! without Place. Every move glides over 50 ms, so the sound can be dragged.

/// Metres a second.
pub const SPEED_OF_SOUND: f32 = 343.0;
/// The Shapes, in their order.
pub const SHAPES: [&str; 6] = ["Room", "Hall", "Cathedral", "Dome", "Chamber", "Plate"];
const MAX_PTS: usize = 28;
const MAX_TAPS: usize = 1 + MAX_PTS + 2;
const GLIDE_S: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaceSettings {
    pub on: bool,
    /// An index into [`SHAPES`].
    pub shape: u8,
    /// Metres across.
    pub size_m: f32,
    /// −100 (left) … 100 (right).
    pub pos: f32,
    /// 0 (close) … 100 (at the back).
    pub dist: f32,
    /// 0 (the floor) … 100 (the ceiling).
    pub height: f32,
    /// The width of the mode's output, 0 … 200 %.
    pub spread: f32,
}

impl Default for PlaceSettings {
    fn default() -> Self {
        PlaceSettings { on: true, shape: 1, size_m: 24.0, pos: -25.0, dist: 40.0, height: 35.0, spread: 100.0 }
    }
}

/// One path from the sound to you.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tap {
    pub direct: bool,
    pub metres: f32,
    /// Its share of the energy (all taps' squares add to one).
    pub gain: f32,
    /// Equal-power pan from where it arrives.
    pub left: f32,
    pub right: f32,
    /// Elevation of the direct sound, radians (0 for reflections).
    pub elevation: f32,
}

/// A Shape's floor plan, in units of its width, and its ceiling.
fn plan(shape: u8, pts: &mut [[f32; 2]; MAX_PTS]) -> (usize, f32, bool) {
    let set = |pts: &mut [[f32; 2]; MAX_PTS], src: &[[f32; 2]]| {
        pts[..src.len()].copy_from_slice(src);
        src.len()
    };
    match shape {
        0 => (set(pts, &[[-0.5, 0.0], [0.5, 0.0], [0.5, 1.15], [-0.5, 1.15]]), 0.32, false),
        1 => (set(pts, &[[-0.36, 0.0], [0.36, 0.0], [0.36, 1.7], [-0.36, 1.7]]), 0.55, false),
        2 => (
            set(pts, &[
                [-0.2, 0.0], [0.2, 0.0], [0.2, 0.75], [0.55, 0.75], [0.55, 1.05], [0.2, 1.05], [0.2, 1.75],
                [-0.2, 1.75], [-0.2, 1.05], [-0.55, 1.05], [-0.55, 0.75], [-0.2, 0.75],
            ]),
            1.0,
            false,
        ),
        3 => {
            for (i, p) in pts.iter_mut().enumerate() {
                let a = i as f32 / MAX_PTS as f32 * std::f32::consts::TAU;
                *p = [0.55 * a.cos(), 0.6 + 0.6 * a.sin()];
            }
            (MAX_PTS, 0.85, false)
        }
        4 => (set(pts, &[[-0.42, 0.0], [0.38, 0.0], [0.55, 0.45], [0.32, 1.05], [-0.3, 1.12], [-0.55, 0.5]]), 0.42, false),
        _ => (set(pts, &[[-0.75, 0.42], [0.75, 0.42], [0.75, 0.72], [-0.75, 0.72]]), 0.06, true),
    }
}

fn mirror(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let t = ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy);
    let (fx, fy) = (a[0] + t * dx, a[1] + t * dy);
    [2.0 * fx - p[0], 2.0 * fy - p[1]]
}

/// Where the segment p→q crosses the wall a→b, as a fraction along p→q.
fn crossing(p: [f32; 2], q: [f32; 2], a: [f32; 2], b: [f32; 2]) -> Option<f32> {
    let r = [q[0] - p[0], q[1] - p[1]];
    let s = [b[0] - a[0], b[1] - a[1]];
    let d = r[0] * s[1] - r[1] * s[0];
    if d.abs() < 1e-9 {
        return None;
    }
    let t = ((a[0] - p[0]) * s[1] - (a[1] - p[1]) * s[0]) / d;
    let u = ((a[0] - p[0]) * r[1] - (a[1] - p[1]) * r[0]) / d;
    (t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&u)).then_some(t)
}

fn inside(pts: &[[f32; 2]], x: f32, y: f32) -> bool {
    let mut c = false;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let ([xi, yi], [xj, yj]) = (pts[i], pts[j]);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            c = !c;
        }
        j = i;
    }
    c
}

fn pan(dx: f32, dy: f32) -> (f32, f32) {
    let p = (dx.atan2(dy) / std::f32::consts::FRAC_PI_2).clamp(-1.0, 1.0);
    let a = (p + 1.0) * std::f32::consts::FRAC_PI_4;
    (a.cos(), a.sin())
}

/// The paths for `s`, into `out` (slot 0 the direct sound, then one per wall,
/// then the floor and the ceiling; a path that does not exist has gain 0).
/// Returns how many slots it used.
fn compute(s: &PlaceSettings, out: &mut [Tap; MAX_TAPS]) -> usize {
    let mut pts = [[0.0f32; 2]; MAX_PTS];
    let (n, ceil, plate) = plan(s.shape, &mut pts);
    let pts = &pts[..n];
    let depth = pts.iter().map(|p| p[1]).fold(0.0, f32::max);
    let wide = pts.iter().map(|p| p[0].abs()).fold(0.0, f32::max) * 2.0;
    let h = if plate { 0.5 } else { 0.2 + 0.75 * ceil };
    let listen = if plate { [0.0, 0.15, 0.25] } else { [0.0, depth * 0.1, (h * 0.4).min(0.13)] };
    let d01 = 0.12 + 0.78 * s.dist.clamp(0.0, 100.0) / 100.0;
    let src = if plate {
        [s.pos / 100.0 * 0.6, 0.57, 0.05 + 0.4 * s.height.clamp(0.0, 100.0) / 100.0]
    } else {
        [s.pos.clamp(-100.0, 100.0) / 100.0 * wide * 0.36, listen[1] + d01 * (depth - listen[1]) * 0.92, 0.03 + 0.9 * h * s.height.clamp(0.0, 100.0) / 100.0]
    };
    let m = s.size_m.max(1.0) / wide;
    let len3 = |a: [f32; 3], b: [f32; 3]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
    *out = [Tap::default(); MAX_TAPS];
    let r = len3(src, listen) * m;
    let (l, rr) = pan(src[0] - listen[0], src[1] - listen[1]);
    let horiz = ((src[0] - listen[0]).powi(2) + (src[1] - listen[1]).powi(2)).sqrt();
    out[0] = Tap { direct: true, metres: r, gain: 1.0 / r.max(0.5), left: l, right: rr, elevation: (src[2] - listen[2]).atan2(horiz) };
    let mut used = 1;
    if !plate {
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let img2 = mirror([src[0], src[1]], a, b);
            let img = [img2[0], img2[1], src[2]];
            if let Some(t) = crossing([listen[0], listen[1]], img2, a, b) {
                let z = listen[2] + t * (img[2] - listen[2]);
                if (0.0..=h).contains(&z) {
                    let path = len3(img, listen) * m;
                    let hit = [listen[0] + t * (img[0] - listen[0]), listen[1] + t * (img[1] - listen[1])];
                    let (l, r) = pan(hit[0] - listen[0], hit[1] - listen[1]);
                    out[1 + i] = Tap { direct: false, metres: path, gain: 0.7 / path.max(0.5), left: l, right: r, elevation: 0.0 };
                }
            }
        }
        for (k, plane) in [0.0, h].into_iter().enumerate() {
            let img = [src[0], src[1], 2.0 * plane - src[2]];
            let t = (plane - listen[2]) / (img[2] - listen[2]);
            let hit = [listen[0] + t * (img[0] - listen[0]), listen[1] + t * (img[1] - listen[1])];
            if inside(pts, hit[0], hit[1]) {
                let path = len3(img, listen) * m;
                let (l, r) = pan(hit[0] - listen[0], hit[1] - listen[1]);
                out[1 + n + k] = Tap { direct: false, metres: path, gain: 0.7 / path.max(0.5), left: l, right: r, elevation: 0.0 };
            }
        }
        used = 1 + n + 2;
    }
    let e: f32 = out[..used].iter().map(|t| t.gain * t.gain).sum();
    let k = 1.0 / e.max(1e-12).sqrt();
    for t in &mut out[..used] {
        t.gain *= k;
    }
    used
}

/// The paths for `s`, the ones that exist: the direct sound first.
pub fn taps(s: &PlaceSettings) -> Vec<Tap> {
    let mut out = [Tap::default(); MAX_TAPS];
    let n = compute(s, &mut out);
    out[..n].iter().copied().filter(|t| t.direct || t.gain > 0.0).collect()
}

#[derive(Clone, Copy, Default)]
struct Live {
    delay: f32,
    gain: f32,
    left: f32,
    right: f32,
}

pub struct Place {
    sr: f32,
    settings: PlaceSettings,
    target: [Tap; MAX_TAPS],
    used: usize,
    live: [Live; MAX_TAPS],
    fresh: bool,
    buf: Vec<f32>,
    mask: usize,
    w: usize,
    glide: f32,
    // the direct sound's air and height cue, and the reflections' darkening
    air: f32,
    air_k: f32,
    shelf_lp: f32,
    shelf_k: f32,
    shelf_g: f32,
    wall: [f32; 2],
    wall_k: f32,
}

impl Place {
    pub fn new(sample_rate: f32) -> Place {
        let sr = sample_rate.max(1.0);
        let n = ((sr * 1.0) as usize).next_power_of_two();
        let one_pole = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / sr).exp();
        let mut p = Place {
            sr,
            settings: PlaceSettings { on: false, ..PlaceSettings::default() },
            target: [Tap::default(); MAX_TAPS],
            used: 0,
            live: [Live::default(); MAX_TAPS],
            fresh: true,
            buf: vec![0.0; n],
            mask: n - 1,
            w: 0,
            glide: 1.0 - (-1.0 / (GLIDE_S * sr)).exp(),
            air: 0.0,
            air_k: 1.0,
            shelf_lp: 0.0,
            shelf_k: one_pole(7000.0),
            shelf_g: 0.0,
            wall: [0.0; 2],
            wall_k: one_pole(6000.0),
        };
        p.set(p.settings);
        p
    }

    pub fn settings(&self) -> PlaceSettings {
        self.settings
    }

    /// Take new settings. Allocates nothing: it may be called between blocks
    /// on the audio thread, as a control moves.
    pub fn set(&mut self, s: PlaceSettings) {
        self.settings = s;
        self.used = compute(&s, &mut self.target);
        let d = self.target[0];
        let fc = (20_000.0 / (1.0 + d.metres / 10.0)).clamp(1500.0, 20_000.0);
        self.air_k = 1.0 - (-std::f32::consts::TAU * fc / self.sr).exp();
        self.shelf_g = 10f32.powf(6.0 * d.elevation.sin() / 20.0) - 1.0;
        if self.fresh {
            for (l, t) in self.live.iter_mut().zip(&self.target) {
                *l = Live { delay: t.metres / SPEED_OF_SOUND * self.sr, gain: t.gain, left: t.left, right: t.right };
            }
            self.fresh = !s.on;
        }
    }

    /// Clear the room's memory, keeping the settings.
    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.air = 0.0;
        self.shelf_lp = 0.0;
        self.wall = [0.0; 2];
        self.fresh = true;
        self.set(self.settings);
    }

    /// Before the mode: the sound, heard from its place in the room.
    pub fn before(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.settings.on {
            return;
        }
        let max = (self.buf.len() - 2) as f32;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.buf[self.w & self.mask] = 0.5 * (*l + *r);
            let (mut direct, mut wl, mut wr) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..self.used {
                let t = self.target[i];
                let v = &mut self.live[i];
                let want = (t.metres / SPEED_OF_SOUND * self.sr).min(max);
                // A path appearing out of silence starts where it is.
                if v.gain.abs() < 1e-6 && t.gain > 0.0 {
                    v.delay = want;
                }
                v.delay += (want - v.delay) * self.glide;
                v.gain += (t.gain - v.gain) * self.glide;
                v.left += (t.left - v.left) * self.glide;
                v.right += (t.right - v.right) * self.glide;
                if v.gain.abs() < 1e-7 {
                    continue;
                }
                let pos = self.w as f32 - v.delay.clamp(0.0, max);
                let i0 = pos.floor();
                let f = pos - i0;
                let a = self.buf[(i0 as i64 as usize) & self.mask];
                let b = self.buf[((i0 as i64 + 1) as usize) & self.mask];
                let x = (a + (b - a) * f) * v.gain;
                if t.direct {
                    direct = x;
                } else {
                    wl += x * v.left;
                    wr += x * v.right;
                }
            }
            // the direct sound: air, then the height cue, then panned
            self.air += (direct - self.air) * self.air_k;
            self.shelf_lp += (self.air - self.shelf_lp) * self.shelf_k;
            let d = self.air + self.shelf_g * (self.air - self.shelf_lp);
            let (dl, dr) = (d * self.live[0].left, d * self.live[0].right);
            // the reflections, a little darker
            self.wall[0] += (wl - self.wall[0]) * self.wall_k;
            self.wall[1] += (wr - self.wall[1]) * self.wall_k;
            *l = dl + self.wall[0];
            *r = dr + self.wall[1];
            self.w = self.w.wrapping_add(1);
        }
    }

    /// After the mode: Spread, the side of its output scaled.
    pub fn after(&self, left: &mut [f32], right: &mut [f32]) {
        if !self.settings.on {
            return;
        }
        let k = self.settings.spread.clamp(0.0, 200.0) / 100.0;
        if k == 1.0 {
            return;
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let m = 0.5 * (*l + *r);
            let s = 0.5 * (*l - *r) * k;
            *l = m + s;
            *r = m - s;
        }
    }
}
