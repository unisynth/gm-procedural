//! `additive`: harmonic table (dB per harmonic) with tail slope, stiff-string inharmonicity, unison detune,
//! per-partial extra decay, odd/even balance, onset brightness tilt, attack pitch scoop and vibrato (as phase
//! modulation, so loops stay exactly periodic). Amplitudes run at control rate (every `CR` samples, linear).
use crate::math::{db, exp, log2, sin_cycles, sqrt};
use crate::params::Layer;
use crate::render::{layer_env, Ctx, Phasor};
use alloc::vec;
use alloc::vec::Vec;

const CR: usize = 32;
const MAX_PARTIALS: usize = 256;

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let n = out.len();
    let f0 = ctx.f0 * crate::math::exp2(l.get("cents") / 1200.0);
    let harm = l.table("harm");
    let phase = l.table("phase");
    let (tail, b) = (l.get("tail"), l.get("inharm"));
    let (even, level) = (db(l.get("even_db")), db(l.get("level_db")));
    let unison = (l.get("unison") + 0.5) as usize;
    let (detune, pdecay) = (l.get("detune"), l.get("pdecay"));
    let (tilt, tilt_t) = (l.get("tilt"), l.get("tilt_t"));
    let nyq = ctx.rate * 0.45;
    let k_table = harm.len().max(1);

    // Shared per-sample modulation: scoop (cycles per Hz of partial) and vibrato phase.
    let freeze = ctx.freeze();
    let scoop_c = crate::math::exp2(l.get("scoop_cents") / 1200.0) - 1.0;
    let tau = l.get("scoop_t");
    let scoop_end = freeze.map_or(0.0, |s| exp(-s / tau));
    let scoop: Vec<f32> = if scoop_c != 0.0 {
        (0..n)
            .map(|i| {
                let t = i as f32 / ctx.rate;
                if freeze.is_some_and(|s| t >= s) { 0.0 } else { -scoop_c * tau * (exp(-t / tau) - scoop_end) }
            })
            .collect()
    } else {
        Vec::new()
    };
    let vib_n = if l.get("vib_cents") > 0.0 { ctx.grid(l.get("vib_rate")) } else { 0 };
    let vib_hz = ctx.hz(vib_n);
    let vib: Vec<f32> = if vib_n > 0 {
        let mut p = Phasor::new(vib_n, ctx.den);
        (0..n).map(|_| sin_cycles(p.tick())).collect()
    } else {
        Vec::new()
    };
    let vib_dev = crate::math::exp2(l.get("vib_cents") / 1200.0) - 1.0;

    let steps = n.div_ceil(CR) + 1;
    let env: Vec<f32> = (0..steps).map(|j| layer_env(l, ctx.t(j * CR))).collect();
    let mut amp = vec![0.0f32; steps];
    // From the loop start on (sustain region) the amplitude is the frozen value: exactly periodic.
    let hold = match ctx.freeze() {
        Some(_) => ctx.lp.map_or(n, |(s, _)| s),
        None => n,
    };
    for k in 1..=MAX_PARTIALS {
        let kf = k as f32;
        let a_db = match harm.get(k - 1) {
            Some(&d) => d,
            None if harm.is_empty() && k == 1 => 0.0,
            None => harm.last().copied().unwrap_or(0.0) + tail * log2(kf / k_table as f32),
        };
        if a_db < -100.0 {
            break;
        }
        let stretch = if b > 0.0 { sqrt((1.0 + b * kf * kf) / (1.0 + b)) } else { 1.0 };
        let fk = kf * f0 * stretch;
        if fk > nyq {
            break;
        }
        let a = db(a_db) * level * if k % 2 == 0 { even } else { 1.0 } / sqrt(unison as f32);
        for (j, v) in amp.iter_mut().enumerate() {
            let t = ctx.t(j * CR);
            let mut x = env[j] * a;
            if pdecay > 0.0 {
                x *= exp(-pdecay * (kf - 1.0) * t);
            }
            if tilt != 0.0 && k > 1 {
                x *= db(tilt * log2(kf) * exp(-t / tilt_t));
            }
            *v = x;
        }
        let ph0 = phase.get(k - 1).copied().unwrap_or(-(kf * (kf - 1.0)) / (2.0 * k_table.max(8) as f32));
        for u in 0..unison {
            // Voices spread over ±detune/2; with more than 3 voices the inner ones get a fixed pseudo-random jitter
            // so an ensemble does not beat in a regular pattern.
            let mut off = if unison == 1 { 0.0 } else { u as f32 / (unison - 1) as f32 - 0.5 };
            if unison > 3 && u > 0 && u + 1 < unison {
                off += (crate::math::fract(u as f32 * 0.618_034 + 0.3) - 0.5) * 0.6 / (unison - 1) as f32;
            }
            let fu = fk * crate::math::exp2(off * detune / 1200.0);
            let nk = ctx.grid(fu);
            if nk == 0 || ctx.hz(nk) > nyq {
                continue;
            }
            let fr = ctx.hz(nk);
            let beta = if vib_n > 0 { fr * vib_dev / (core::f32::consts::TAU * vib_hz) } else { 0.0 };
            let mut ph = Phasor::new(nk, ctx.den);
            // Unison voices start at spread phases so they do not add up coherently at onset.
            ph.acc = (((u as u64 * ctx.den as u64 * 2_654_435_761u64) >> 32) % ctx.den as u64) as u32;
            for (i, o) in out.iter_mut().enumerate() {
                let mut c = ph.tick() + ph0;
                if !scoop.is_empty() {
                    c += fr * scoop[i];
                }
                if beta != 0.0 {
                    c += beta * vib[i];
                }
                let a = if i >= hold {
                    amp[steps - 1]
                } else {
                    let (j, w) = (i / CR, (i % CR) as f32 / CR as f32);
                    amp[j] + (amp[j + 1] - amp[j]) * w
                };
                *o += a * sin_cycles(c);
            }
        }
    }
}
