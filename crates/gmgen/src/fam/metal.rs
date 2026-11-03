//! `metal`: TR-808-style metallic noise — up to 6 band-limited (PolyBLEP) square oscillators at `base × ratio`
//! (default the 808 set 205.3/304.4/369.6/522.7/540/800 Hz), optional white noise, band-pass + high-pass, and a
//! one- or two-stage exponential envelope (`d` until the level falls to `split`, then `d2`). Attack/whole only.
use crate::math::{db, exp, Rng};
use crate::params::Layer;
use crate::render::{Biquad, Ctx, Kind};

const R808: [f32; 6] = [1.0, 1.482_7, 1.800_3, 2.546_0, 2.630_3, 3.896_7];

/// PolyBLEP correction for a unit step at phase 0 (phase `p` in [0,1), increment `dt`).
#[inline]
fn blep(p: f32, dt: f32) -> f32 {
    if p < dt {
        let x = p / dt;
        x + x - x * x - 1.0
    } else if p > 1.0 - dt {
        let x = (p - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let t = l.table("ratios");
    let ratios: &[f32] = if t.is_empty() { &R808 } else { t };
    let base = l.get("base");
    let nyq = ctx.rate * 0.45;
    let mut osc: [(f32, f32); 6] = [(0.0, 0.0); 6];
    let mut used = 0;
    for (k, r) in ratios.iter().enumerate().take(6) {
        let hz = base * r;
        if hz < nyq {
            // Spread start phases deterministically.
            osc[used] = (crate::math::fract(k as f32 * 0.618_034), hz / ctx.rate);
            used += 1;
        }
    }
    let mut bp = Biquad::new(Kind::Bandpass, l.get("bp"), l.get("q"), ctx.rate);
    let mut hp = Biquad::new(Kind::Highpass, l.get("hp"), 0.707, ctx.rate);
    let (a, d, d2, split) = (l.get("a"), l.get("d"), l.get("d2"), l.get("split"));
    let level = db(l.get("level_db"));
    let noise = l.get("noise");
    let mut rng = Rng::new(ctx.seed);
    // Time at which the first stage reaches `split`.
    let ts = if split > 0.0 && split < 1.0 { -d * crate::math::log2(split) / core::f32::consts::LOG2_E } else { f32::MAX };
    for (i, o) in out.iter_mut().enumerate() {
        let mut sq = 0.0;
        for (p, dt) in osc.iter_mut().take(used) {
            let mut y = if *p < 0.5 { 1.0 } else { -1.0 };
            y += blep(*p, *dt);
            let q = if *p < 0.5 { *p + 0.5 } else { *p - 0.5 };
            y -= blep(q, *dt);
            sq += y;
            *p += *dt;
            if *p >= 1.0 {
                *p -= 1.0;
            }
        }
        let x = sq / used.max(1) as f32 * (1.0 - noise) + rng.white() * noise;
        let y = hp.run(bp.run(x));
        let tt = i as f32 / ctx.rate;
        let e = if tt < ts { exp(-tt / d) } else { split * exp(-(tt - ts) / d2) };
        let att = if a > 0.0 && tt < a { tt / a } else { 1.0 };
        *o += level * y * e * att;
    }
}
