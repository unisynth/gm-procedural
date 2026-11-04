//! `formant` family (task 7.3): a glottal source — band-limited saw, or pulse of width `pw` — with aspiration
//! `noise` and period `jitter`, through ≤ 5 parallel formant band-passes (`formants` rows [Hz, bandwidth Hz, dB]).
//! - Syllables (`syl` rows [s, dB, cents]): the voiced source sounds only in syllables, each with attack `syl_a`,
//!   decay `syl_d` and a pitch fall of `syl_glide` cents over `syl_d`; `breath_db` is noise through the formants that
//!   follows only the layer envelope (the "h" of a laugh); the syllables carry their own envelopes. No rows: the
//!   source sounds throughout, under the layer envelope.
//! - Sustain: per harmonic — the source spectrum times the formants' complex response at that harmonic — on the loop
//!   grid, vibrato snapped to whole cycles per loop and the total phase to whole cycles: exactly periodic. Voiced only
//!   (breath and aspiration: a `noise` layer).
use crate::math::{cos_cycles, db, exp, exp2, floor, fract, sin_cycles, Rng};
use crate::params::Layer;
use crate::render::{layer_env, Biquad, Ctx, Kind};
use crate::schema::Region;
use alloc::vec;
use alloc::vec::Vec;

/// PolyBLEP residual for a unit saw step at phase `t` (cycles) with increment `dt`.
fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

/// Band-limited saw at phase `t` in [0, 1).
fn saw(t: f32, dt: f32) -> f32 {
    2.0 * t - 1.0 - blep(t, dt)
}

fn formants(l: &Layer) -> Vec<(f32, f32, f32)> {
    l.table("formants").chunks(3).filter(|r| r.len() == 3).map(|r| (r[0], r[1].max(10.0), db(r[2]))).collect()
}

/// Voiced-source level at `t` from the syllable table (1 when there is none) and the pitch offset in cents.
fn syllables(syl: &[f32], a: f32, d: f32, glide: f32, t: f32) -> (f32, f32) {
    if syl.is_empty() {
        return (1.0, 0.0);
    }
    let (mut g, mut cents, mut best) = (0.0f32, 0.0f32, 0.0f32);
    for r in syl.chunks(3).filter(|r| r.len() == 3) {
        let u = t - r[0];
        if u < 0.0 {
            continue;
        }
        let e = if u < a { u / a } else { exp(-(u - a) / d) } * db(r[1]);
        g += e;
        if e > best {
            best = e;
            cents = r[2] - glide * (1.0 - exp(-u / d));
        }
    }
    (g, cents)
}

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let rate = ctx.rate;
    let f0 = if l.get("f0") > 0.0 { l.get("f0") } else { ctx.f0 };
    let pw = l.get("pw");
    let level = db(l.get("level_db"));
    let fs = formants(l);
    let (vr, vc) = (l.get("vib_rate"), l.get("vib_cents"));
    match (ctx.region, ctx.lp) {
        (Region::Sustain, Some((s, len))) => {
            // Snap: vibrato to whole cycles per loop; f0 to the loop grid (whole periods per loop).
            let vcyc = if vr > 0.0 && vc > 0.0 { ((vr * len as f32 / rate + 0.5) as u32).max(1) as f64 } else { 0.0 };
            let periods = ((f0 * len as f32 / rate + 0.5) as u32).max(1) as f64;
            let base = periods * rate as f64 / len as f64;
            // Pitch multiplier per sample over the loop, normalised so the loop holds exactly `periods` periods.
            let m: Vec<f64> = (0..len).map(|j| exp2(vc * sin_cycles((vcyc * j as f64 / len as f64) as f32) / 1200.0) as f64).collect();
            let norm = len as f64 / m.iter().sum::<f64>();
            let kmax = ((0.45 * rate) as f64 / base) as usize;
            let bq: Vec<(Biquad, f32)> = fs.iter().map(|&(f, bw, g)| (Biquad::new(Kind::Bandpass, f.min(0.45 * rate), f / bw, rate), g)).collect();
            // Complex coefficient of each harmonic: source (saw −2/(πk); pulse × (1 − e^{i2πk·pw})) × Σ formants.
            let coef: Vec<(f32, f32)> = (1..=kmax)
                .map(|k| {
                    let a = -2.0 / (core::f32::consts::PI * k as f32);
                    let (sr, si) = if pw > 0.0 {
                        let x = k as f32 * pw;
                        (a * (1.0 - cos_cycles(x)), a * -sin_cycles(x))
                    } else {
                        (a, 0.0)
                    };
                    let w = (k as f64 * base / rate as f64) as f32;
                    let (mut hr, mut hi) = (0.0f32, 0.0f32);
                    for (b, g) in &bq {
                        let (r, i) = b.response(w);
                        hr += g * r;
                        hi += g * i;
                    }
                    (sr * hr - si * hi, sr * hi + si * hr)
                })
                .collect();
            let mut pat = vec![0.0f32; len];
            let mut ph = 0.0f64;
            for (j, v) in pat.iter_mut().enumerate() {
                let mut acc = 0.0f32;
                for (k, &(cr, ci)) in coef.iter().enumerate() {
                    let x = (k + 1) as f64 * ph;
                    let th = (x - (x as u64) as f64) as f32;
                    acc += cr * sin_cycles(th) + ci * cos_cycles(th);
                }
                *v = acc;
                ph += base / rate as f64 * m[j] * norm;
                ph -= floor(ph as f32) as f64;
            }
            for (i, o) in out.iter_mut().enumerate() {
                *o += pat[(i + len - s % len) % len] * layer_env(l, ctx.t(i)) * level;
            }
        }
        _ => {
            let syl = l.table("syl");
            let (sa, sd, sg) = (l.get("syl_a").max(1e-4), l.get("syl_d").max(1e-4), l.get("syl_glide"));
            let (noise, breath, jit) = (l.get("noise"), db(l.get("breath_db")), l.get("jitter"));
            let mut bq: Vec<(Biquad, f32)> = fs.iter().map(|&(f, bw, g)| (Biquad::new(Kind::Bandpass, f.min(0.45 * rate), f / bw, rate), g)).collect();
            let mut rng = Rng::new(ctx.seed);
            let (mut ph, mut jc) = (0.0f32, 0.0f32);
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let e = layer_env(l, ctx.t(i));
                let (v, cents) = syllables(syl, sa, sd, sg, t);
                let vib = if vr > 0.0 { vc * sin_cycles(vr * t) } else { 0.0 };
                let dt = (f0 * exp2((cents + vib + jc) / 1200.0) / rate).min(0.5);
                let src = if pw > 0.0 { saw(ph, dt) - saw(fract(ph + pw), dt) } else { saw(ph, dt) };
                let w = rng.white();
                let ve = if syl.is_empty() { v * e } else { v };
                let x = ve * ((1.0 - noise) * src + noise * w) + breath * e * w;
                let y: f32 = if bq.is_empty() { x } else { bq.iter_mut().map(|(b, g)| *g * b.run(x)).sum() };
                *o += y * level;
                ph += dt;
                if ph >= 1.0 {
                    ph -= 1.0;
                    jc = jit * rng.white();
                }
            }
        }
    }
}
