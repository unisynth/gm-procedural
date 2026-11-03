//! Wave rendering: layer regions (design D3), a final soft knee above 0.9 (never full scale), shared envelope, exact integer phasors, biquads, and the dispatch to
//! the family modules in `fam/`.
use crate::fam;
use crate::math::{self, exp, sin_cycles};
use crate::params::{Layer, Params};
use crate::schema::{Region, FAMILIES};
use alloc::vec;
use alloc::vec::Vec;

/// What a wave must fit: from the bank structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveMeta {
    pub rate: u32,
    pub len: usize,
    pub unity: u16,
    pub fine: i16,
    /// `(start, length)` in samples.
    pub looped: Option<(u32, u32)>,
}

/// Render context handed to a family.
pub struct Ctx {
    pub rate: f32,
    pub len: usize,
    /// Loop `(start, length)`, if the wave loops and the loop is inside the wave.
    pub lp: Option<(usize, usize)>,
    /// Fundamental (Hz) of the wave.
    pub f0: f32,
    /// Phasor denominator: the loop length for looped waves, `rate·64` otherwise (1/64 Hz resolution).
    pub den: u32,
    pub seed: u32,
    /// The region this layer renders (sustain only when `lp` is set; whole otherwise).
    pub region: Region,
}

impl Ctx {
    /// Exact phasor numerator for `hz`: cycles per sample = `n / den`. Looped waves snap to the loop grid
    /// `rate/L`, which keeps every partial exactly periodic over the loop.
    pub fn grid(&self, hz: f32) -> u32 {
        let n = math::floor(hz * self.den as f32 / self.rate + 0.5);
        if n <= 0.0 { 0 } else { n as u32 }
    }
    /// Frequency actually produced by a numerator.
    pub fn hz(&self, n: u32) -> f32 {
        n as f32 * self.rate / self.den as f32
    }
    /// Time (s) at which envelopes freeze: the loop start for the sustain region, never otherwise.
    pub fn freeze(&self) -> Option<f32> {
        match (self.region, self.lp) {
            (Region::Sustain, Some((s, _))) => Some(s as f32 / self.rate),
            _ => None,
        }
    }
    /// `t` (s) of sample `i`, clamped at the freeze point.
    pub fn t(&self, i: usize) -> f32 {
        let t = i as f32 / self.rate;
        match self.freeze() {
            Some(f) if t > f => f,
            _ => t,
        }
    }
}

/// Silent before `dly`, then linear attack `a`, hold `h`, exponential decay with time constant `d` toward sustain `s`.
pub fn env(t: f32, a: f32, h: f32, d: f32, s: f32, dly: f32) -> f32 {
    let t = t - dly;
    if t < 0.0 {
        return 0.0;
    }
    if t < a {
        return t / a;
    }
    let u = t - a - h;
    if u <= 0.0 {
        return 1.0;
    }
    s + (1.0 - s) * exp(-u / d)
}

/// Envelope of a layer with the shared `a h d s` scalars.
pub fn layer_env(l: &Layer, t: f32) -> f32 {
    env(t, l.get("a"), l.get("h"), l.get("d"), l.get("s"), l.get("dly"))
}

/// Integer phase accumulator: `cycles = acc / den`, exact and periodic.
#[derive(Clone, Copy)]
pub struct Phasor {
    pub acc: u32,
    pub n: u32,
    pub den: u32,
}

impl Phasor {
    pub fn new(n: u32, den: u32) -> Self {
        Phasor { acc: 0, n: n % den.max(1), den: den.max(1) }
    }
    /// Current phase in cycles [0, 1), then advance one sample.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        let c = self.acc as f32 / self.den as f32;
        self.acc += self.n;
        if self.acc >= self.den {
            self.acc -= self.den;
        }
        c
    }
}

/// RBJ biquad (direct form I).
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Lowpass,
    Highpass,
    /// Constant 0 dB peak gain.
    Bandpass,
}

impl Biquad {
    pub fn new(kind: Kind, hz: f32, q: f32, rate: f32) -> Biquad {
        let mut b = Biquad::default();
        b.set(kind, hz, q, rate);
        b
    }
    /// Recomputes the coefficients, keeping the state.
    pub fn set(&mut self, kind: Kind, hz: f32, q: f32, rate: f32) {
        let w = (hz / rate).clamp(1e-5, 0.49);
        let (sn, cs) = (sin_cycles(w), sin_cycles(w + 0.25));
        let alpha = sn / (2.0 * q.max(0.05));
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = match kind {
            Kind::Lowpass => ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0),
            Kind::Highpass => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0),
            Kind::Bandpass => (alpha, 0.0, -alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = -2.0 * cs / a0;
        self.a2 = (1.0 - alpha) / a0;
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Fundamental (Hz) of a wave: `pitch` if set, else the unity note.
pub fn f0(meta: &WaveMeta, p: &Params) -> f32 {
    math::midi_hz(p.pitch().unwrap_or(meta.unity as f32))
}

/// Renders a wave's samples (f32, nominally in [-1, 1]) from its parameters.
pub fn render(meta: &WaveMeta, p: &Params, seed: u32) -> Vec<f32> {
    let n = meta.len;
    let lp = meta
        .looped
        .map(|(s, l)| (s as usize, l as usize))
        .filter(|&(s, l)| l > 0 && s + l <= n);
    let rate = meta.rate as f32;
    let mut out = vec![0.0f32; n];
    for (k, layer) in p.layers.iter().enumerate() {
        // Without a usable loop, sustain renders the whole wave; with one, `whole` behaves as attack.
        let region = match (layer.region, lp) {
            (Region::Sustain, None) | (Region::Whole, None) | (Region::Attack, None) => Region::Whole,
            (Region::Whole, Some(_)) => Region::Attack,
            (r, Some(_)) => r,
        };
        let ctx = Ctx {
            rate,
            len: n,
            lp,
            f0: f0(meta, p),
            den: match (region, lp) {
                (Region::Sustain, Some((_, l))) => l as u32,
                _ => meta.rate * 64,
            },
            seed: seed.wrapping_mul(31).wrapping_add(k as u32 + 1),
            region,
        };
        let mut buf = vec![0.0f32; n];
        match FAMILIES[layer.family].name {
            "additive" => fam::additive::render(&ctx, layer, &mut buf),
            "noise" => fam::noise::render(&ctx, layer, &mut buf),
            "drum" => fam::drum::render(&ctx, layer, &mut buf),
            "metal" => fam::metal::render(&ctx, layer, &mut buf),
            _ => {}
        }
        if let (Region::Attack, Some((s, _))) = (region, lp) {
            // Fade to silence over the last 10 ms (at most the whole attack) before the loop start.
            let f = s.min((rate * 0.01) as usize).max(1);
            for (i, x) in buf.iter_mut().enumerate().skip(s - f.min(s)) {
                *x *= if i >= s { 0.0 } else { (s - i) as f32 / f as f32 };
            }
        }
        for (o, x) in out.iter_mut().zip(&buf) {
            *o += x;
        }
    }
    let g = math::db(p.gain_db());
    out.iter_mut().for_each(|x| *x *= g);
    if !p.correction.is_empty() {
        crate::correct::apply(&mut out, lp, f0(meta, p), rate, &p.correction);
    }
    if !p.tf.is_empty() {
        crate::tf::apply(&mut out, lp, rate, &p.tf);
    }
    if !p.env.is_empty() {
        crate::tf::apply_env(&mut out, lp, rate, &p.env);
    }
    out.iter_mut().for_each(|x| *x = soft_knee(*x));
    out
}

/// Identity below 0.9; above, a smooth rational-tanh knee that approaches (never reaches) full scale.
#[inline]
pub fn soft_knee(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.9 {
        return x;
    }
    let u = (a - 0.9) * 10.0;
    // tanh(u) ≈ u(27 + u²) / (27 + 9u²), monotone and < 1 for u ≥ 0.
    let t = (u * (27.0 + u * u) / (27.0 + 9.0 * u * u)).min(0.999);
    (0.9 + 0.1 * t).copysign(x)
}
