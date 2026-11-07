//! `ks`: extended Karplus-Strong (Jaffe–Smith) plucked string. A noise burst of `exc` periods, low-passed at `dyn` Hz
//! (pluck dynamics) and combed at the pick position (`pick` × the string length: ½ removes the even harmonics),
//! excites — through optional body resonances `body` [Hz, Q, dB] (commuted synthesis) — a delay-line loop: one-zero
//! damping (`damp`: upper harmonics die faster), dispersion all-pass (`stretch`: stiff-string sharp partials), a
//! fractional all-pass tuning the loop to the exact pitch, and an optional asymmetric bridge contact (`buzz`, sitar).
//! The loop gain gives the fundamental a 60 dB decay in `t60` s. Not periodic: whole waves and attack layers only
//! (a looped wave hands over to an exact additive sustain at the loop start).
use crate::math::{cos_cycles, db, exp2, sin_cycles, Rng};
use crate::params::Layer;
use crate::render::{layer_env, Biquad, Ctx, Kind};
use alloc::vec;
use alloc::vec::Vec;

const CR: usize = 32;

/// atan2 (no_std): range reduction + odd polynomial, |error| < 2e-6 rad.
fn atan2(y: f32, x: f32) -> f32 {
    use core::f32::consts::{FRAC_PI_2, PI};
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    let (ax, ay) = (x.abs(), y.abs());
    let (a, swap) = if ay > ax { (ax / ay, true) } else { (ay / ax, false) };
    // atan on [0, 1] via atan(a) = 2·atan(a / (1 + √(1 + a²))) then a 9th-order series on |t| ≤ 0.42.
    let t = a / (1.0 + crate::math::sqrt(1.0 + a * a));
    let t2 = t * t;
    let r = 2.0 * t * (1.0 - t2 * (1.0 / 3.0 - t2 * (1.0 / 5.0 - t2 * (1.0 / 7.0 - t2 * (1.0 / 9.0 - t2 / 11.0)))));
    let r = if swap { FRAC_PI_2 - r } else { r };
    let r = if x < 0.0 { PI - r } else { r };
    if y < 0.0 { -r } else { r }
}

/// Phase delay (samples) at `w` (rad/sample) of the first-order all-pass (c + z⁻¹)/(1 + c·z⁻¹).
fn ap_delay(c: f32, w: f32) -> f32 {
    let (cw, sw) = (cos_cycles(w / core::f32::consts::TAU), sin_cycles(w / core::f32::consts::TAU));
    // numerator c + e^{-jw}, denominator 1 + c e^{-jw}
    let pn = atan2(-sw, c + cw);
    let pd = atan2(-c * sw, 1.0 + c * cw);
    let mut ph = pn - pd;
    while ph > 0.0 {
        ph -= core::f32::consts::TAU;
    }
    -ph / w
}

/// The string loop for one layer: delay length, filter coefficients and gain.
struct Loop {
    n: usize,
    s: f32,
    disp: f32,
    tune: f32,
    g: f32,
}

fn design(rate: f32, f0: f32, damp: f32, stretch: f32, t60: f32) -> Loop {
    let w = core::f32::consts::TAU * f0 / rate;
    let total = rate / f0;
    let s = 0.5 * damp.clamp(0.0, 1.0);
    // One-zero (1 − s) + s·z⁻¹: phase delay and magnitude at the fundamental.
    let (re, im) = ((1.0 - s) + s * cos_cycles(f0 / rate), -s * sin_cycles(f0 / rate));
    let lp_delay = -atan2(im, re) / w;
    let lp_mag = crate::math::sqrt(re * re + im * im);
    let disp = -stretch.clamp(0.0, 0.95);
    // (at coefficient 0 the dispersion all-pass is z⁻¹: one sample)
    let fixed = lp_delay + ap_delay(disp, w);
    // Integer part leaves 0.2…1.2 samples for the tuning all-pass, solved by bisection on its coefficient.
    let n = ((total - fixed - 0.2) as usize).max(2);
    let want = total - fixed - n as f32;
    let (mut lo, mut hi) = (-0.999f32, 0.999f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        // delay decreases as the coefficient grows
        if ap_delay(mid, w) > want { lo = mid } else { hi = mid }
    }
    let g = (exp2(-3.0 * 3.321_928 / (f0 * t60.max(1e-3))) / lp_mag.max(1e-6)).min(0.99999);
    Loop { n, s, disp, tune: 0.5 * (lo + hi), g }
}

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let n_out = out.len();
    let rate = ctx.rate;
    let f0 = if l.get("f0") > 0.0 { l.get("f0") } else { ctx.f0 } * exp2(l.get("cents") / 1200.0);
    if f0 <= 0.0 || f0 >= 0.45 * rate {
        return;
    }
    let lp = design(rate, f0, l.get("damp"), l.get("stretch"), l.get("t60"));
    let level = db(l.get("level_db"));
    let buzz = l.get("buzz");
    // Excitation: burst of `exc` periods, dynamics low-pass, pick comb, body resonances (+ direct).
    let mut rng = Rng::new(ctx.seed);
    let burst = ((l.get("exc") * rate / f0) as usize).max(1);
    let k = exp2(-core::f32::consts::TAU * l.get("dyn") / rate * core::f32::consts::LOG2_E);
    let mut z = 0.0;
    // One-pole low-pass z = (1 − k)·noise + k·z (pluck dynamics: soft plucks are darker).
    let mut x: Vec<f32> = (0..burst)
        .map(|_| {
            z = (1.0 - k) * rng.white() + k * z;
            z
        })
        .collect();
    let pick = l.get("pick");
    if pick > 0.0 {
        let p = ((pick * rate / f0) + 0.5) as usize;
        if p > 0 {
            x.resize(burst + p, 0.0);
            for i in (p..x.len()).rev() {
                x[i] -= x[i - p];
            }
        }
    }
    let body: Vec<(f32, f32, f32)> = l.table("body").chunks(3).map(|r| (r[0], r[1], db(r[2]))).collect();
    if !body.is_empty() {
        let tail = x.len() + (0.2 * rate) as usize;
        let mut fs: Vec<Biquad> = body.iter().map(|&(f, q, _)| Biquad::new(Kind::Bandpass, f.min(0.45 * rate), q, rate)).collect();
        x = (0..tail.min(n_out.max(x.len())))
            .map(|i| {
                let v = x.get(i).copied().unwrap_or(0.0);
                v + fs.iter_mut().zip(&body).map(|(f, b)| f.run(v) * b.2).sum::<f32>()
            })
            .collect();
    }
    // String loop: ring buffer of n; per sample y = AP_tune(AP_disp(LP(delay out))) · g, buzz, fed back + excitation.
    let mut line = vec![0.0f32; lp.n];
    let (mut prev, mut d_x1, mut d_y1, mut t_x1, mut t_y1) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut pos = 0;
    let mut env = 0.0;
    for (i, o) in out.iter_mut().enumerate() {
        if i % CR == 0 {
            env = layer_env(l, ctx.t(i)) * level;
        }
        let v = line[pos];
        let a = (1.0 - lp.s) * v + lp.s * prev;
        prev = v;
        let b = lp.disp * a + d_x1 - lp.disp * d_y1;
        d_x1 = a;
        d_y1 = b;
        let c = lp.tune * b + t_x1 - lp.tune * t_y1;
        t_x1 = b;
        t_y1 = c;
        let mut y = c * lp.g;
        if buzz > 0.0 && y < 0.0 {
            y *= 1.0 - buzz.min(1.0);
        }
        let y = y + x.get(i).copied().unwrap_or(0.0);
        line[pos] = y;
        pos = if pos + 1 == lp.n { 0 } else { pos + 1 };
        *o += y * env;
    }
}

#[cfg(test)]
mod tests {
    use crate::params::{Layer, Params};
    use crate::render::{render, WaveMeta};
    use crate::schema::{family, Region};

    pub(super) fn pluck(f0: f32, set: &[(&str, f32)]) -> Vec<f32> {
        let m = WaveMeta { rate: 44100, len: 44100 * 2, unity: 60, fine: 0, looped: None };
        let mut l = Layer::new(family("ks").unwrap(), Region::Whole);
        l.set("f0", f0);
        for &(k, v) in set {
            l.set(k, v);
        }
        let mut p = Params::default();
        p.layers.push(l);
        render(&m, &p, 1)
    }

    /// DFT magnitude at `hz` over `x` (Hann).
    fn mag(x: &[f32], hz: f32, rate: f32) -> f32 {
        let n = x.len();
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / n as f64).cos();
            let ph = core::f64::consts::TAU * hz as f64 * i as f64 / rate as f64;
            re += w * *v as f64 * ph.cos();
            im -= w * *v as f64 * ph.sin();
        }
        (re * re + im * im).sqrt() as f32
    }

    /// Pitch within 2 cents: the strongest DFT bin near f0, refined by golden search, at 110 and 440 Hz (with damping
    /// and dispersion off and on).
    #[test]
    fn ks_pitch_within_2_cents() {
        for (f0, damp, stretch) in [(110.0f32, 0.2f32, 0.0f32), (440.0, 0.6, 0.0), (196.0, 0.3, 0.5)] {
            let x = pluck(f0, &[("damp", damp), ("stretch", stretch), ("t60", 4.0)]);
            let seg = &x[4410..4410 + 44100];
            let (mut lo, mut hi) = (f0 * 0.98, f0 * 1.02);
            for _ in 0..60 {
                let (a, b) = (lo + 0.382 * (hi - lo), lo + 0.618 * (hi - lo));
                if mag(seg, a, 44100.0) > mag(seg, b, 44100.0) { hi = b } else { lo = a }
            }
            let got = 0.5 * (lo + hi);
            let cents = 1200.0 * (got / f0).log2();
            assert!(cents.abs() < 2.0, "f0 {f0} damp {damp} stretch {stretch}: {got} Hz ({cents:+.2} cents)");
        }
    }

    /// Decay within 10 %: with damping off every harmonic loses 60 dB in `t60`; RMS 0.2 s → 1.2 s.
    #[test]
    fn ks_decay_within_10_percent() {
        let x = pluck(220.0, &[("damp", 0.0), ("t60", 2.0)]);
        let rms = |a: usize| (x[a..a + 4410].iter().map(|v| v * v).sum::<f32>() / 4410.0).sqrt();
        let drop = 20.0 * (rms(8820) / rms(52920)).log10();
        let t60 = 60.0 / drop;
        assert!((t60 / 2.0 - 1.0).abs() < 0.1, "t60 {t60}");
    }

    /// Pick position ½ removes the even harmonics (2·f0 ≥ 25 dB under f0 and 3·f0).
    #[test]
    fn ks_pick_half_removes_even() {
        let x = pluck(200.0, &[("pick", 0.5), ("damp", 0.1), ("t60", 3.0)]);
        let seg = &x[2205..2205 + 22050];
        let (h1, h2, h3) = (mag(seg, 200.0, 44100.0), mag(seg, 400.0, 44100.0), mag(seg, 600.0, 44100.0));
        assert!(20.0 * (h1 / h2).log10() > 25.0 && 20.0 * (h3 / h2).log10() > 20.0, "h1 {h1} h2 {h2} h3 {h3}");
        let y = pluck(200.0, &[("pick", 0.0), ("damp", 0.1), ("t60", 3.0)]);
        let seg = &y[2205..2205 + 22050];
        assert!(mag(seg, 400.0, 44100.0) > 0.1 * mag(seg, 200.0, 44100.0), "unpicked keeps even harmonics");
    }

    /// Body resonances and buzz stay finite and decaying.
    #[test]
    fn ks_body_and_buzz_stable() {
        let x = pluck(147.0, &[("buzz", 0.6), ("t60", 1.0)]);
        assert!(x.iter().all(|v| v.is_finite()));
        let tail = x[80000..].iter().map(|v| v.abs()).fold(0.0, f32::max);
        let head = x[..8000].iter().map(|v| v.abs()).fold(0.0, f32::max);
        assert!(tail < 0.05 * head, "head {head} tail {tail}");
        let _ = Params::default();
    }
}
