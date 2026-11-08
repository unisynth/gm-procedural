//! `modal`: a resonator of up to 32 modes `[Hz, dB, decay s, phase cycles]` struck by a train of `hits` (every
//! `period` s, each `hit_db` dB louder or softer than the one before, timing `jitter`). Every hit restarts a shared
//! pitch glide (start `glide` cents off, relaxing with `glide_t`), a per-hit attack `ha` and a band-passed noise
//! click (`click_hp`…`click_lp`), and a trill (`trill_hz`, `trill_cents`: FM of every mode, restarted per hit). A `hits` table [s, dB] gives measured strike times instead (guiro, vibraslap). One hit is a struck object (conga, agogo, triangle, whistle); several are a rattle (vibraslap, guiro) or
//! repeated chirps (bird).
//!
//! Sustain region (exactly periodic over the loop):
//! - `hits` ≤ 1: steady sparse tones, each mode snapped to the loop grid with its stored phase at the loop start (a
//!   cymbal or belltree loop: a few dozen scattered bins that no harmonic series fits).
//! - `hits` > 1: the stationary hit train folded into the loop (period snapped to loop/m, first hit `offset`
//!   periods after the loop start, hit i `hit_db`·i louder), tails folded over the loop, tiled from the loop start.
use crate::math::{db, exp, exp2, sin_cycles, Rng};
use crate::params::Layer;
use crate::render::{layer_env, Ctx};
use alloc::vec;
use alloc::vec::Vec;

struct Hit {
    modes: Vec<[f32; 4]>,
    r: f32,
    gt: f32,
    ha: f32,
    click: f32,
    cd: f32,
    hp: f32,
    lp: f32,
    /// Trill: rate Hz and peak relative deviation (2^(cents/1200) − 1).
    tr: f32,
    td: f32,
}

impl Hit {
    /// Response at `tau` s after a hit (no click); `None` once every mode has died away.
    fn at(&self, tau: f32) -> Option<f32> {
        let e = exp(-tau / self.gt);
        let warp = tau + (self.r - 1.0) * self.gt * (1.0 - e);
        let att = if self.ha > 0.0 && tau < self.ha { tau / self.ha } else { 1.0 };
        // Trill: f(τ) = f·(1 + d·cos 2π r τ) → phase f·(τ + d·sin(2π r τ)/(2π r)) in cycles.
        let fm = if self.tr > 0.0 { self.td * sin_cycles(self.tr * tau) / (core::f32::consts::TAU * self.tr) } else { 0.0 };
        let mut y = 0.0;
        let mut alive = false;
        for m in &self.modes {
            let g = exp(-tau / m[2]);
            if g < 1e-5 {
                continue;
            }
            alive = true;
            y += m[1] * g * sin_cycles(m[0] * (warp + fm) + m[3]);
        }
        alive.then_some(y * att)
    }
    /// Adds `g`× this hit's response (no click) to `out[i0..end)`. Without a trill each mode is a decaying sinusoid
    /// once the glide has settled (14 glide time constants): a rotator scaled by the per-sample decay, resynced to the
    /// exact warped phase every 64 samples (design D9, drift ≲ 1e-6), instead of an `exp` and a `sin` per mode per
    /// sample; exact per sample while gliding. A trill takes `at` per sample.
    fn add_into(&self, out: &mut [f32], i0: usize, end: usize, g: f32, rate: f32) {
        if self.tr > 0.0 {
            for (i, o) in out.iter_mut().enumerate().take(end).skip(i0) {
                match self.at((i - i0) as f32 / rate) {
                    Some(v) => *o += g * v,
                    None => break,
                }
            }
            return;
        }
        let len = end.saturating_sub(i0);
        let mut y = vec![0.0f32; len];
        let warp = |tau: f32| if self.r == 1.0 { tau } else { tau + (self.r - 1.0) * self.gt * (1.0 - exp(-tau / self.gt)) };
        let settle = if self.r == 1.0 { 0 } else { ((14.0 * self.gt * rate) as usize).min(len) };
        for m in &self.modes {
            for (k, v) in y.iter_mut().enumerate().take(settle) {
                let tau = k as f32 / rate;
                let gm = exp(-tau / m[2]);
                if gm >= 1e-5 {
                    *v += m[1] * gm * sin_cycles(m[0] * warp(tau) + m[3]);
                }
            }
            let (dc, ds) = (crate::math::cos_cycles(m[0] / rate), sin_cycles(m[0] / rate));
            let d = exp(-1.0 / (m[2] * rate));
            let (mut rc, mut rs, mut gm) = (0.0f32, 0.0f32, 0.0f32);
            for (k, v) in y.iter_mut().enumerate().skip(settle) {
                if (k - settle) % 64 == 0 {
                    let tau = k as f32 / rate;
                    gm = exp(-tau / m[2]);
                    if gm < 1e-5 {
                        break;
                    }
                    let ph = m[0] * warp(tau) + m[3];
                    rc = crate::math::cos_cycles(ph);
                    rs = sin_cycles(ph);
                }
                *v += m[1] * gm * rs;
                let r = rc * dc - rs * ds;
                rs = rs * dc + rc * ds;
                rc = r;
                gm *= d;
            }
        }
        for (k, v) in y.iter().enumerate() {
            let tau = k as f32 / rate;
            let att = if self.ha > 0.0 && tau < self.ha { tau / self.ha } else { 1.0 };
            out[i0 + k] += g * v * att;
        }
    }
    /// Adds `g`× this hit's click (band-passed noise burst from `rng`) to `out` starting at sample `i0`.
    fn click_into(&self, out: &mut [f32], i0: usize, g: f32, rate: f32, rng: &mut Rng) {
        if self.click <= 1e-4 {
            return;
        }
        let a = exp(-core::f32::consts::TAU * self.hp / rate);
        let b = 1.0 - exp(-core::f32::consts::TAU * self.lp / rate);
        let (mut px, mut hy, mut ly) = (0.0f32, 0.0f32, 0.0f32);
        let end = (i0 + (self.cd * 11.6 * rate) as usize).min(out.len());
        for (k, o) in out.iter_mut().enumerate().take(end).skip(i0) {
            let x = rng.white();
            hy = a * (hy + x - px);
            px = x;
            ly += b * (hy - ly);
            *o += g * self.click * ly * exp(-((k - i0) as f32 / rate) / self.cd);
        }
    }
    /// Longest decay (s).
    fn tail(&self) -> f32 {
        self.modes.iter().map(|m| m[2]).fold(0.0, f32::max) * 11.6
    }
}

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let n = out.len();
    let level = db(l.get("level_db"));
    let nyq = ctx.rate * 0.49;
    let hits = (l.get("hits") + 0.5) as usize;
    let modes: Vec<[f32; 4]> = l
        .table("modes")
        .chunks(4)
        .filter(|m| m[0] > 0.0 && m[0] < nyq)
        .map(|m| [m[0], db(m[1]), m[2].max(1e-4), m[3]])
        .collect();
    let hit = Hit {
        modes,
        r: exp2(l.get("glide") / 1200.0),
        gt: l.get("glide_t"),
        ha: l.get("ha"),
        click: db(l.get("click_db")),
        cd: l.get("click_decay"),
        hp: l.get("click_hp"),
        lp: l.get("click_lp").min(0.49 * ctx.rate),
        tr: l.get("trill_hz"),
        td: exp2(l.get("trill_cents") / 1200.0) - 1.0,
    };
    match (ctx.freeze(), ctx.lp) {
        (Some(_), Some((s, len))) if hits <= 1 => {
            // Steady sparse tones on the loop grid: phase measured from the loop start, exact integer phase.
            let den = len as u64;
            for m in &hit.modes {
                let k = ctx.grid(m[0]) as u64;
                if k == 0 {
                    continue;
                }
                for (i, o) in out.iter_mut().enumerate() {
                    let j = (i as i64 - s as i64).rem_euclid(len as i64) as u64;
                    *o += m[1] * sin_cycles(((k * j) % den) as f32 / den as f32 + m[3]);
                }
            }
        }
        (Some(_), Some((s, len))) => {
            // One loop's hit pattern (m hits, the i-th `hit_db`·i louder, first `offset` periods in, each with its
            // own deterministic click) rendered over enough loops for the tails, then folded: y(j) = Σ_q r(j + qL).
            let m = ((len as f32 / (l.get("period") * ctx.rate)) + 0.5).max(1.0) as usize;
            let p = len as f32 / m as f32;
            let off = l.get("offset");
            let reps = ((hit.tail().max(hit.cd * 11.6) * ctx.rate / len as f32) as usize + 2).min(64);
            let mut r = vec![0.0f32; len * reps];
            let tab = l.table("hits");
            let pattern: Vec<(usize, f32)> = if tab.is_empty() {
                (0..m).map(|i| (((i as f32 + off) * p) as usize, db(l.get("hit_db") * i as f32))).collect()
            } else {
                // Nearest sample, not floor: measured times are exact sample positions, and the file's 5-digit rounding
                // would otherwise move a hit one sample (0.4 cycle at 8 kHz — up to 0.07 of score).
                tab.chunks(2).map(|r| (((r[0] * ctx.rate + 0.5) as usize).min(len - 1), db(r[1]))).collect()
            };
            for (i, &(i0, g)) in pattern.iter().enumerate() {
                let end = (i0 + (hit.tail() * ctx.rate) as usize + 1).min(r.len());
                hit.add_into(&mut r, i0, end, g, ctx.rate);
                let mut rng = Rng::new(ctx.seed ^ (i as u32 + 1).wrapping_mul(0x9E37_79B9));
                hit.click_into(&mut r, i0, g, ctx.rate, &mut rng);
            }
            let y: Vec<f32> = (0..len).map(|j| (0..reps).map(|q| r[j + q * len]).sum()).collect();
            for (i, o) in out.iter_mut().enumerate() {
                *o += y[(i as i64 - s as i64).rem_euclid(len as i64) as usize];
            }
        }
        _ => {
            // Hit train from `dly` (a wave with leading silence; the envelope is silent before it): the measured
            // `hits` table, or `hits` regular strikes every `period` (± `jitter`), each `hit_db` dB on.
            let mut rng = Rng::new(ctx.seed);
            let t_start = l.get("dly");
            let tab = l.table("hits");
            let pattern: Vec<(f32, f32)> = if tab.is_empty() {
                let (period, step, jit) = (l.get("period"), db(l.get("hit_db")), l.get("jitter"));
                let (mut t0, mut g) = (t_start, 1.0f32);
                (0..hits.max(1))
                    .map(|k| {
                        let x = (t0, g);
                        g *= step;
                        let j = if k == 0 { 0.0 } else { rng.white() };
                        t0 += period * (1.0 + jit * j);
                        x
                    })
                    .collect()
            } else {
                tab.chunks(2).map(|r| (t_start + r[0], db(r[1]))).collect()
            };
            for (t0, g) in pattern {
                let i0 = (t0 * ctx.rate + 0.5) as usize;
                if i0 >= n {
                    break;
                }
                let end = (i0 + (hit.tail() * ctx.rate) as usize + 1).min(n);
                hit.add_into(out, i0, end, g, ctx.rate);
                hit.click_into(out, i0, g, ctx.rate, &mut rng);
            }
        }
    }
    for (i, o) in out.iter_mut().enumerate() {
        *o *= level * layer_env(l, ctx.t(i));
    }
}

#[cfg(test)]
mod fast_tests {
    use super::Hit;

    /// `add_into` (rotators after the glide settles, design D9) equals `at` per sample within 1e-4 of the peak, on a
    /// gliding three-mode hit.
    #[test]
    fn hit_rotators_match_exact() {
        let rate = 22050.0;
        let hit = Hit {
            modes: vec![[523.0, 1.0, 0.3, 0.1], [1410.0, 0.5, 0.08, 0.7], [3150.0, 0.25, 0.02, 0.3]],
            r: 1.4,
            gt: 0.004,
            ha: 0.002,
            click: 0.0,
            cd: 0.001,
            hp: 20.0,
            lp: 10000.0,
            tr: 0.0,
            td: 0.0,
        };
        let n = 20000;
        let mut fast = vec![0.0f32; n];
        hit.add_into(&mut fast, 100, n, 0.8, rate);
        let exact: Vec<f32> = (0..n).map(|i| if i < 100 { 0.0 } else { 0.8 * hit.at((i - 100) as f32 / rate).unwrap_or(0.0) }).collect();
        let peak = exact.iter().map(|v| v.abs()).fold(0.0, f32::max);
        let err = fast.iter().zip(&exact).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(peak > 0.1 && err < 1e-4 * peak, "err {err} peak {peak}");
    }
}
