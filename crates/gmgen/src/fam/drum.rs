//! `drum`: body oscillator whose frequency falls from `freq·ratio` to `freq` (time constant `sweep_t`), shaped
//! from sine toward odd-harmonic (`shape`), with up to 3 extra membrane modes riding the same pitch glide,
//! a noise beater click and soft-clip `drive`. Attack/whole region only.
use crate::math::{db, exp, sin_cycles, Rng};
use crate::params::Layer;
use crate::render::Ctx;

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let (f, r, tau) = (l.get("freq"), l.get("ratio"), l.get("sweep_t"));
    let (decay, a, shape, drive) = (l.get("decay"), l.get("a"), l.get("shape"), l.get("drive"));
    let level = db(l.get("level_db"));
    let (click, cd) = (db(l.get("click_db")), l.get("click_decay"));
    let modes = l.table("modes");
    let mut rng = Rng::new(ctx.seed);
    let nyq = ctx.rate * 0.45;
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / ctx.rate;
        let e = exp(-t / tau);
        // Phase (cycles) of the gliding body: ∫ f·(1 + (r−1)·e^{−t/τ}) dt.
        let ph = f * (t + (r - 1.0) * tau * (1.0 - e));
        let fi = f * (1.0 + (r - 1.0) * e);
        let mut y = sin_cycles(ph);
        if shape > 0.0 && 3.0 * fi < nyq {
            y += shape * (-sin_cycles(3.0 * ph) / 9.0 + if 5.0 * fi < nyq { sin_cycles(5.0 * ph) / 25.0 } else { 0.0 });
        }
        let att = if a > 0.0 && t < a { t / a } else { 1.0 };
        let mut x = y * exp(-t / decay);
        for m in modes.chunks(3) {
            if m[0] * fi < nyq {
                x += db(m[1]) * sin_cycles(m[0] * ph) * exp(-t / m[2]);
            }
        }
        x *= att;
        if drive > 0.0 {
            let k = 1.0 + 9.0 * drive;
            x = (x * k) / (1.0 + (x * k).abs()) * (1.0 + 1.0 / k);
        }
        let c = if click > 1e-4 { click * rng.white() * exp(-t / cd) } else { 0.0 };
        *o += level * x + c;
    }
}
