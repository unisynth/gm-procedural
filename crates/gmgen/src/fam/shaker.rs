//! `shaker`: PhISEM (Cook) — random collision events, each a short noise burst (decay `sdecay`) ringing ≤ 3
//! band-pass resonances `res` [Hz, Q, gain dB] (none: the bare burst). Events arrive as a Bernoulli process at
//! `rate` × envelope^`follow` per second; each has a level spread of ±`gjit` dB, its resonances detuned by ±`fjit`
//! cents and sweeping `chirp` octaves with time constant `chirp_t` (water drops/bubbles rise). Covers maracas,
//! cabasa, tambourine jingles, sleigh bells, applause, streams.
//! Whole/attack: free-running events, each at the envelope level of its onset. Sustain: one loop-length event pattern,
//! rings wrapped circularly, tiled, so the wave is exactly periodic over the loop.
use crate::math::{db, exp, exp2, log2, Rng};
use crate::params::Layer;
use crate::render::{layer_env, Biquad, Ctx, Kind};
use crate::schema::Region;
use alloc::vec;
use alloc::vec::Vec;

const CR: usize = 32;
/// Longest ring of one event (s).
const MAX_RING: f32 = 0.5;

/// One collision: onset sample, linear level, resonance detune (cents).
pub struct Event {
    pub at: usize,
    pub amp: f32,
    pub cents: f32,
}

/// Collision events over `n` samples. `steady`: the sustain pattern — rate from the frozen envelope, levels
/// without it (the tiled pattern is enveloped afterwards).
pub fn events(ctx: &Ctx, l: &Layer, n: usize, rng: &mut Rng, steady: bool) -> Vec<Event> {
    let (rate, follow, gj, fj) = (l.get("rate"), l.get("follow"), l.get("gjit"), l.get("fjit"));
    let pw = |e: f32| if e <= 0.0 { 0.0 } else if follow == 0.0 { 1.0 } else { exp2(follow * log2(e)) };
    let frozen = ctx.freeze().map_or(1.0, |f| layer_env(l, f));
    let mut out = Vec::new();
    let mut e = 0.0;
    for i in 0..n {
        if i % CR == 0 {
            e = if steady { frozen } else { layer_env(l, ctx.t(i)) };
        }
        let p = rate * pw(e) / ctx.rate;
        if 0.5 * (rng.white() + 1.0) < p {
            let amp = (if steady { 1.0 } else { e }) * db(gj * rng.white());
            out.push(Event { at: i, amp, cents: fj * rng.white() });
        }
    }
    out
}

/// Renders one event's burst + rings, calling `emit(offset, sample)`.
fn ring(l: &Layer, ev: &Event, rate: f32, rng: &mut Rng, mut emit: impl FnMut(usize, f32)) {
    let sd = l.get("sdecay");
    let (chirp, ct) = (l.get("chirp"), l.get("chirp_t"));
    let res: Vec<(f32, f32, f32)> = l.table("res").chunks(3).map(|r| (r[0] * exp2(ev.cents / 1200.0), r[1], db(r[2]))).collect();
    let burst = ((7.0 * sd * rate) as usize).max(1);
    let k = exp(-1.0 / (sd * rate));
    // Ring length: −60 dB of the slowest resonance (RBJ band-pass pole radius ≈ e^{−π·f/(Q·rate)}).
    let tail = res.iter().map(|&(f, q, _)| (6.9 * q * rate / (core::f32::consts::PI * f.max(20.0))) as usize).max().unwrap_or(0);
    let len = (burst + tail).min((MAX_RING * rate) as usize).max(burst);
    let mut filt: Vec<Biquad> = res.iter().map(|&(f, q, _)| Biquad::new(Kind::Bandpass, f.min(0.45 * rate), q, rate)).collect();
    let mut g = ev.amp;
    for j in 0..len {
        let x = if j < burst { g * rng.white() } else { 0.0 };
        g *= k;
        if chirp != 0.0 && j % CR == 0 && j > 0 {
            let m = exp2(chirp * (1.0 - exp(-(j as f32 / rate) / ct)));
            for (f, &(c, q, _)) in filt.iter_mut().zip(&res) {
                f.set(Kind::Bandpass, (c * m).clamp(20.0, 0.45 * rate), q, rate);
            }
        }
        let y = if res.is_empty() { x } else { filt.iter_mut().zip(&res).map(|(f, r)| f.run(x) * r.2).sum() };
        emit(j, y);
    }
}

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let n = out.len();
    let level = db(l.get("level_db"));
    let mut rng = Rng::new(ctx.seed);
    match (ctx.region, ctx.lp) {
        (Region::Sustain, Some((s, len))) => {
            let evs = events(ctx, l, len, &mut rng, true);
            let mut pat = vec![0.0f32; len];
            for ev in &evs {
                ring(l, ev, ctx.rate, &mut rng, |j, y| pat[(ev.at + j) % len] += y);
            }
            let steps = n.div_ceil(CR) + 1;
            let env: Vec<f32> = (0..steps).map(|j| layer_env(l, ctx.t(j * CR)) * level).collect();
            for (i, o) in out.iter_mut().enumerate() {
                let g = if i >= s {
                    env[steps - 1]
                } else {
                    let (j, w) = (i / CR, (i % CR) as f32 / CR as f32);
                    env[j] + (env[j + 1] - env[j]) * w
                };
                *o += pat[(i + len - s % len) % len] * g;
            }
        }
        _ => {
            let evs = events(ctx, l, n, &mut rng, false);
            for ev in &evs {
                ring(l, ev, ctx.rate, &mut rng, |j, y| {
                    if let Some(o) = out.get_mut(ev.at + j) {
                        *o += y * level;
                    }
                });
            }
        }
    }
}
