//! `noise`: white noise through up to 8 band-pass bands (centre, Q, gain dB) — or flat when no band is given —
//! then high-pass `hp` and low-pass `lp`, enveloped and optionally amplitude-modulated.
//! Sustain region: one loop-length noise pattern filtered circularly (two warm-up passes) and tiled, so the
//! wave is exactly periodic over the loop; `sweep` is ignored there. Transient: free noise, band centres moving
//! by `sweep` octaves at onset and relaxing with `sweep_t`.
use crate::math::{db, exp, exp2, sin_cycles, Rng};
use crate::params::Layer;
use crate::render::{layer_env, Biquad, Ctx, Kind, Phasor};
use crate::schema::Region;
use alloc::vec;
use alloc::vec::Vec;

const CR: usize = 32;

struct Chain {
    bands: Vec<(Biquad, f32, f32, f32)>, // filter, centre, q, gain
    hp: Biquad,
    lp: Biquad,
}

impl Chain {
    fn new(l: &Layer, rate: f32) -> Chain {
        let t = l.table("bands");
        Chain {
            bands: t
                .chunks(3)
                .map(|b| (Biquad::new(Kind::Bandpass, b[0], b[1], rate), b[0], b[1], db(b[2])))
                .collect(),
            hp: Biquad::new(Kind::Highpass, l.get("hp"), 0.707, rate),
            lp: Biquad::new(Kind::Lowpass, l.get("lp"), 0.707, rate),
        }
    }
    fn retune(&mut self, oct: f32, rate: f32) {
        let m = exp2(oct);
        for (f, c, q, _) in &mut self.bands {
            f.set(Kind::Bandpass, *c * m, *q, rate);
        }
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = if self.bands.is_empty() { x } else { self.bands.iter_mut().map(|(f, _, _, g)| f.run(x) * *g).sum() };
        self.lp.run(self.hp.run(y))
    }
}

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let n = out.len();
    let level = db(l.get("level_db"));
    let mut rng = Rng::new(ctx.seed);
    let mut chain = Chain::new(l, ctx.rate);
    let am_n = if l.get("am_depth") > 0.0 { ctx.grid(l.get("am_rate")) } else { 0 };
    let mut am = Phasor::new(am_n, ctx.den);
    let depth = l.get("am_depth");
    let steps = n.div_ceil(CR) + 1;
    let env: Vec<f32> = (0..steps).map(|j| layer_env(l, ctx.t(j * CR)) * level).collect();
    let hold = match (ctx.region, ctx.lp) {
        (Region::Sustain, Some((s, _))) => s,
        _ => n,
    };
    let gain = |i: usize| {
        if i >= hold {
            env[steps - 1]
        } else {
            let (j, w) = (i / CR, (i % CR) as f32 / CR as f32);
            env[j] + (env[j + 1] - env[j]) * w
        }
    };
    let am_at = |c: f32| if am_n > 0 { 1.0 - depth * 0.5 * (1.0 - sin_cycles(c)) } else { 1.0 };
    match (ctx.region, ctx.lp) {
        (Region::Sustain, Some((s, len))) => {
            let white: Vec<f32> = (0..len).map(|_| rng.white()).collect();
            let mut pat = vec![0.0f32; len];
            for _ in 0..3 {
                for (p, w) in pat.iter_mut().zip(&white) {
                    *p = chain.run(*w);
                }
            }
            for (i, o) in out.iter_mut().enumerate() {
                let k = (i + len - s % len) % len;
                *o += pat[k] * gain(i) * am_at(am.tick());
            }
        }
        _ => {
            let (sweep, st) = (l.get("sweep"), l.get("sweep_t"));
            for (i, o) in out.iter_mut().enumerate() {
                if sweep != 0.0 && i % CR == 0 {
                    chain.retune(sweep * exp(-(i as f32 / ctx.rate) / st), ctx.rate);
                }
                *o += chain.run(rng.white()) * gain(i) * am_at(am.tick());
            }
        }
    }
}
