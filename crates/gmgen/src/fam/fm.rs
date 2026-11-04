//! `fm` family (task 7.6): 2–4 sine operators with index envelopes, feedback on the top operator, a stack or two
//! parallel pairs, plus a repeating pitch/loudness cycle (`cyc`: a bird's tweet train) and a start glide.
//! - `ops` rows [ratio, amount, index decay s, index sustain 0–1]: row 0 the carrier. A carrier's amount is its
//!   output level (dB); a modulator's is its peak index (radians), decaying toward `sustain × peak` with `decay`.
//!   Frequencies: `freq` (Hz; 0 = the wave's fundamental) × ratio, all times the common pitch curve.
//! - `algo` 0: stack (op k modulates op k−1, op 0 sounds); 1: pairs 1→0 and 3→2 (ops 0 and 2 sound).
//! - `cyc` rows [cents, dB]: points evenly over one cycle at `cyc_hz` (linear, wrapping), from `cyc_phase`.
//! - Sustain: one loop is rendered and tiled. The cycle rate snaps to whole cycles per loop, the carrier's base
//!   frequency to whole cycles of total phase per loop and each modulator ratio to whole cycles too, so the loop is
//!   exact (feedback: one warm-up loop first); glide and index envelopes freeze at the loop start.
use crate::math::{db, exp, exp2, floor, sin_cycles};
use crate::params::Layer;
use crate::render::{layer_env, Ctx};
use crate::schema::Region;
use alloc::vec;
use alloc::vec::Vec;

struct Op {
    ratio: f64,
    amount: f32,
    decay: f32,
    sustain: f32,
}

/// Linear interpolation of the cycle table at phase `c` (cycles): (cents, dB).
fn cyc_at(cyc: &[f32], c: f64) -> (f32, f32) {
    let m = cyc.len() / 2;
    if m == 0 {
        return (0.0, 0.0);
    }
    let x = (c - floor(c as f32) as f64) * m as f64;
    let i = x as usize % m;
    let w = (x - floor(x as f32) as f64) as f32;
    let j = (i + 1) % m;
    (cyc[2 * i] + (cyc[2 * j] - cyc[2 * i]) * w, cyc[2 * i + 1] + (cyc[2 * j + 1] - cyc[2 * i + 1]) * w)
}

/// Common pitch multiplier and loudness gain at time `t` (s) and cycle phase `c`.
fn pitch_gain(l: &Layer, cyc: &[f32], t: f32, c: f64) -> (f64, f32) {
    let (cents, gdb) = cyc_at(cyc, c);
    let glide = l.get("glide") * exp(-t / l.get("glide_t"));
    (exp2((cents + glide) / 1200.0) as f64, db(gdb))
}

/// One operator network sample: phases (cycles), indices (radians), feedback state.
fn voice(ops: &[Op], ph: &[f64], idx: &[f32], algo: u32, fb: f32, prev: &mut f32) -> f32 {
    let k = ops.len();
    let tau = core::f32::consts::TAU;
    let car = |i: usize, m: f32| sin_cycles(ph[i] as f32 + m / tau);
    if algo == 1 {
        // pairs 1→0, 3→2
        let mut out = 0.0;
        for base in [0usize, 2] {
            if base >= k {
                break;
            }
            let m = if base + 1 < k {
                let top = base + 1 == k - 1;
                let y = sin_cycles(ph[base + 1] as f32 + if top { fb * *prev / tau } else { 0.0 });
                if top {
                    *prev = y;
                }
                idx[base + 1] * y
            } else {
                0.0
            };
            out += db(ops[base].amount) * car(base, m);
        }
        out
    } else {
        let mut m = 0.0;
        for i in (1..k).rev() {
            let y = sin_cycles(ph[i] as f32 + (m + if i == k - 1 { fb * *prev } else { 0.0 }) / tau);
            if i == k - 1 {
                *prev = y;
            }
            m = idx[i] * y;
        }
        db(ops[0].amount) * car(0, m)
    }
}

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let mut ops: Vec<Op> = l
        .table("ops")
        .chunks(4)
        .filter(|r| r.len() == 4)
        .map(|r| Op { ratio: r[0] as f64, amount: r[1], decay: r[2].max(1e-4), sustain: r[3] })
        .collect();
    if ops.is_empty() {
        ops.push(Op { ratio: 1.0, amount: 0.0, decay: 1.0, sustain: 1.0 });
    }
    let cyc = l.table("cyc");
    let rate = ctx.rate;
    let mut base = if l.get("freq") > 0.0 { l.get("freq") as f64 } else { ctx.f0 as f64 };
    let mut cyc_hz = l.get("cyc_hz") as f64;
    let (algo, fb) = (l.get("algo") as u32, l.get("feedback"));
    let level = db(l.get("level_db"));
    let index = |o: &Op, t: f32| o.amount * (o.sustain + (1.0 - o.sustain) * exp(-t / o.decay));
    let nops = ops.len();
    match (ctx.region, ctx.lp) {
        (Region::Sustain, Some((s, len))) => {
            let ts = s as f32 / rate;
            if cyc_hz > 0.0 {
                let m = (cyc_hz * len as f64 / rate as f64 + 0.5) as u64;
                cyc_hz = m.max(1) as f64 * rate as f64 / len as f64;
            }
            // Pitch and gain over one loop (frozen glide), then snap the total phase to whole cycles.
            let c0 = l.get("cyc_phase") as f64 + cyc_hz * ts as f64;
            let pg: Vec<(f64, f32)> = (0..len).map(|j| pitch_gain(l, cyc, ts, c0 + cyc_hz * j as f64 / rate as f64)).collect();
            let total: f64 = pg.iter().map(|p| p.0).sum::<f64>() * base / rate as f64;
            let whole = (total + 0.5) as u64;
            base *= whole.max(1) as f64 / total.max(1e-9);
            let cycles = whole.max(1) as f64;
            for o in ops.iter_mut() {
                o.ratio = ((o.ratio * cycles + 0.5) as u64).max(1) as f64 / cycles;
            }
            let idx: Vec<f32> = ops.iter().map(|o| index(o, ts)).collect();
            let mut ph = vec![0.0f64; nops];
            let mut prev = 0.0f32;
            let mut pat = vec![0.0f32; len];
            for pass in 0..2 {
                ph.iter_mut().for_each(|p| *p = 0.0);
                for j in 0..len {
                    let y = voice(&ops, &ph, &idx, algo, fb, &mut prev);
                    if pass == 1 || fb == 0.0 {
                        pat[j] = y * pg[j].1;
                    }
                    for (p, o) in ph.iter_mut().zip(&ops) {
                        *p += base * o.ratio * pg[j].0 / rate as f64;
                        *p -= floor(*p as f32) as f64;
                    }
                }
                if fb == 0.0 {
                    break;
                }
            }
            for (i, o) in out.iter_mut().enumerate() {
                *o += pat[(i + len - s % len) % len] * layer_env(l, ctx.t(i)) * level;
            }
        }
        _ => {
            let mut ph = vec![0.0f64; nops];
            let mut prev = 0.0f32;
            let mut c = l.get("cyc_phase") as f64;
            for (i, o) in out.iter_mut().enumerate() {
                let t = i as f32 / rate;
                let e = layer_env(l, ctx.t(i));
                let (pm, g) = pitch_gain(l, cyc, t, c);
                if e > 0.0 {
                    let idx: Vec<f32> = ops.iter().map(|op| index(op, t)).collect();
                    *o += voice(&ops, &ph, &idx, algo, fb, &mut prev) * g * e * level;
                }
                for (p, op) in ph.iter_mut().zip(&ops) {
                    *p += base * op.ratio * pm / rate as f64;
                    *p -= floor(*p as f32) as f64;
                }
                c += cyc_hz / rate as f64;
            }
        }
    }
}
