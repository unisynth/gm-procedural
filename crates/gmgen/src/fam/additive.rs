//! `additive`: harmonic table (dB per harmonic) with tail slope, stiff-string inharmonicity, unison detune,
//! per-partial extra decay, odd/even balance, onset brightness tilt, attack pitch scoop and vibrato (as phase
//! modulation, so loops stay exactly periodic; pitch curve as a Fourier series `vib` table), amplitude modulation
//! `am_db` and per-harmonic `am` table (1×…4× rate) locked to the vibrato phasor, and
//! ensemble `spread`: each partial becomes a Gaussian cluster of
//! components (sd `spread` cents, spacing ≥ one grid step, ≤ 25 components, power preserved, fixed pseudo-random
//! phases) on the loop grid, so a chorus of players loops exactly. Amplitudes run at control rate (every `CR`
//! samples, linear).
use crate::math::{db, exp, log2, sin_cycles, sqrt};
use crate::params::Layer;
use crate::render::{layer_env, Ctx, Phasor};
use crate::schema::Region;
use alloc::vec;
use alloc::vec::Vec;

const CR: usize = 32;
const MAX_PARTIALS: usize = 256;

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let n = out.len();
    let f0 = ctx.f0 * crate::math::exp2(l.get("cents") / 1200.0);
    let harm = l.table("harm");
    let phase = l.table("phase");
    let phase_seed = (l.get("phase_seed") + 0.5) as u32;
    let (tail, b) = (l.get("tail"), l.get("inharm"));
    let (even, level) = (db(l.get("even_db")), db(l.get("level_db")));
    let unison = (l.get("unison") + 0.5) as usize;
    let (detune, pdecay) = (l.get("detune"), l.get("pdecay"));
    let (tilt, tilt_t) = (l.get("tilt"), l.get("tilt_t"));
    let spread = l.get("spread");
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
    let am_db = l.get("am_db");
    let (am_tab, vib_tab, side_tab) = (l.table("am"), l.table("vib"), l.table("side"));
    let modulated = l.get("vib_cents") > 0.0 || am_db > 0.0 || !am_tab.is_empty() || !vib_tab.is_empty();
    let vib_n = if modulated { ctx.grid(l.get("vib_rate")) } else { 0 };
    let vib_hz = ctx.hz(vib_n);
    // Modulation phasor (cycles of the vibrato rate, exact on the loop grid).
    let modu: Vec<f32> = if vib_n > 0 {
        let mut p = Phasor::new(vib_n, ctx.den);
        (0..n).map(|_| p.tick()).collect()
    } else {
        Vec::new()
    };
    // Pitch curve Σ a_n·cos(2π(φ_n + n·u)) (cents → ratio a); as phase modulation the partial at fr Hz gains
    // fr·pm cycles, pm = Σ a_n/(2π·n·rate)·sin(2π(φ_n + n·u)).
    let terms: Vec<(f32, f32)> = if vib_tab.is_empty() {
        alloc::vec![(l.get("vib_cents"), l.get("vib_phase"))]
    } else {
        vib_tab.chunks(2).map(|r| (r[0], r[1])).collect()
    };
    let vib: Vec<f32> = if vib_n > 0 && terms.iter().any(|t| t.0 != 0.0) {
        modu.iter()
            .map(|u| {
                terms
                    .iter()
                    .enumerate()
                    .map(|(j, &(c, ph))| {
                        let m = (j + 1) as f32;
                        (crate::math::exp2(c / 1200.0) - 1.0) / (core::f32::consts::TAU * m * vib_hz)
                            * sin_cycles(ph + m * u)
                    })
                    .sum()
            })
            .collect()
    } else {
        Vec::new()
    };
    // Amplitude modulation locked to the vibrato phasor (periodic on the loop like the vibrato).
    let am: Vec<f32> = if am_db > 0.0 && vib_n > 0 {
        let ph = l.get("am_phase");
        modu.iter().map(|u| db(am_db * crate::math::cos_cycles(u + ph))).collect()
    } else {
        Vec::new()
    };

    let steps = n.div_ceil(CR) + 1;
    let env: Vec<f32> = (0..steps).map(|j| layer_env(l, ctx.t(j * CR))).collect();
    let mut amp = vec![0.0f32; steps];
    // From the loop start on (sustain region) the amplitude is the frozen value: exactly periodic.
    let hold = match ctx.freeze() {
        Some(_) => ctx.lp.map_or(n, |(s, _)| s),
        None => n,
    };
    // Frozen sustain (amplitude held from the loop start, no phase or loudness modulation): every component sits on
    // bin n of the L-point loop grid, so samples ≥ the loop start are one inverse DFT of the components, repeated —
    // one FFT instead of a sine per component per sample (design D9), periodic by construction.
    let frozen_fft = match (ctx.region, ctx.lp) {
        (Region::Sustain, Some((_, l))) => ctx.den as usize == l && scoop.is_empty() && vib.is_empty() && am.is_empty() && l >= 16,
        _ => false,
    };
    let (mut fre, mut fim) = if frozen_fft { (vec![0.0f32; ctx.den as usize], vec![0.0f32; ctx.den as usize]) } else { (Vec::new(), Vec::new()) };
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
        // With an `am` table, `spread` covers only the harmonics past it (their irregular ensemble flutter); the
        // tabled ones keep their locked modulation.
        let spread = if k <= am_tab.len() / 8 { 0.0 } else { spread };
        let voices = if spread > 0.0 { 1 } else { unison };
        let a = db(a_db) * level * if k % 2 == 0 { even } else { 1.0 } / sqrt(voices as f32);
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
        // Harmonics without a stored phase: Schroeder phases (low crest for a steady tone), or with `phase_seed`
        // hashed random phases — a section's upper partials are scattered, and Schroeder phases on a steady upper
        // spectrum line up into one sharp pulse per period (audible buzz).
        let ph0 = phase.get(k - 1).copied().unwrap_or_else(|| {
            if phase_seed > 0 {
                let h = (k as u32).wrapping_mul(2_654_435_761) ^ phase_seed.wrapping_mul(0x9E37_79B9);
                let h = (h ^ (h >> 15)).wrapping_mul(0x85EB_CA6B);
                (h >> 8) as f32 / (1u32 << 24) as f32
            } else {
                -(kf * (kf - 1.0)) / (2.0 * k_table.max(8) as f32)
            }
        });
        // Components: (grid frequency, amplitude factor, phase offset in cycles).
        let mut comps: Vec<(u32, f32, f32)> = Vec::new();
        let sides: Vec<&[f32]> = side_tab.chunks(4).filter(|r| (r[0] + 0.5) as usize == k).collect();
        if !sides.is_empty() {
            let c = ctx.grid(fk) as i64;
            for r in &sides {
                let n = c + if r[1] < 0.0 { (r[1] - 0.5) as i64 } else { (r[1] + 0.5) as i64 };
                if n > 0 {
                    comps.push((n as u32, db(r[2]), r[3]));
                }
            }
        } else if spread > 0.0 {
            let c = ctx.grid(fk);
            let sd = fk * (crate::math::exp2(spread / 1200.0) - 1.0);
            let step = ctx.hz(1).max(1e-6);
            let dn = ((sd / 2.0 / step + 0.5) as u32).max(1);
            let jmax = ((2.5 * sd / (dn as f32 * step)) as i32 + 1).min(12);
            let mut sum = 0.0;
            for j in -jmax..=jmax {
                let n = c as i64 + j as i64 * dn as i64;
                if n <= 0 {
                    continue;
                }
                let x = j as f32 * dn as f32 * step / sd.max(1e-6);
                let w = exp(-0.5 * x * x);
                let h = (k as u32).wrapping_mul(2_654_435_761) ^ ((j + 64) as u32).wrapping_mul(40_503) ^ ctx.seed;
                let ph = (h.wrapping_mul(2_246_822_519) >> 8) as f32 / (1u32 << 24) as f32;
                comps.push((n as u32, w, ph));
                sum += w * w;
            }
            let g = 1.0 / sqrt(sum.max(1e-12));
            comps.iter_mut().for_each(|c| c.1 *= g);
        } else {
            for u in 0..unison {
                // Voices spread over ±detune/2; with more than 3 voices the inner ones get a fixed pseudo-random
                // jitter so an ensemble does not beat in a regular pattern.
                let mut off = if unison == 1 { 0.0 } else { u as f32 / (unison - 1) as f32 - 0.5 };
                if unison > 3 && u > 0 && u + 1 < unison {
                    off += (crate::math::fract(u as f32 * 0.618_034 + 0.3) - 0.5) * 0.6 / (unison - 1) as f32;
                }
                let fu = fk * crate::math::exp2(off * detune / 1200.0);
                // Unison voices start at spread phases so they do not add up coherently at onset.
                let ph = (((u as u64 * 2_654_435_761u64) >> 8) & 0xff_ffff) as f32 / (1u32 << 24) as f32;
                comps.push((ctx.grid(fu), 1.0, if u == 0 { 0.0 } else { ph }));
            }
        }
        // This harmonic's own loudness modulation (body resonances swept by the vibrato).
        // Row: [dB, phase] at 1×…4× rate.
        let hm: Vec<f32> = match am_tab.chunks(8).nth(k - 1) {
            Some(r) if vib_n > 0 && sides.is_empty() && r.chunks(2).any(|t| t[0] != 0.0) => modu
                .iter()
                .map(|u| db(r.chunks(2).enumerate().map(|(j, t)| t[0] * crate::math::cos_cycles(t[1] + (j + 1) as f32 * u)).sum()))
                .collect(),
            _ => Vec::new(),
        };
        for &(nk, wamp, wph) in &comps {
            if nk == 0 || ctx.hz(nk) > nyq {
                continue;
            }
            let fr = ctx.hz(nk);

            let mut ph = Phasor::new(nk, ctx.den);
            // Frozen part by FFT: a·sin(2π(n·i/L + φ)) = Im(a·e^{2πiφ}·e^{2πi·n·i/L}) → coefficient at bin n.
            let direct_end = if frozen_fft && hm.is_empty() {
                let phi = (if sides.is_empty() { ph0 } else { 0.0 }) + wph;
                let a = wamp * amp[steps - 1];
                fre[nk as usize] += a * crate::math::cos_cycles(phi);
                fim[nk as usize] += a * crate::math::sin_cycles(phi);
                hold.min(n)
            } else {
                n
            };
            // No phase modulation: a complex rotator, resynced to the exact phasor phase every 64 samples (drift
            // ≲ 1e-6; the loop region is the FFT's), instead of a sine per sample.
            if scoop.is_empty() && (vib.is_empty() || !sides.is_empty()) {
                let off = (if sides.is_empty() { ph0 } else { 0.0 }) + wph;
                let step = nk as f32 / ctx.den as f32;
                let (dc, ds) = (crate::math::cos_cycles(step), sin_cycles(step));
                let (mut rc, mut rs) = (0.0f32, 0.0f32);
                for (i, o) in out.iter_mut().enumerate().take(direct_end) {
                    let p = ph.tick();
                    if i % 64 == 0 {
                        rc = crate::math::cos_cycles(p + off);
                        rs = sin_cycles(p + off);
                    }
                    let a = if i >= hold {
                        amp[steps - 1]
                    } else {
                        let (j, w) = (i / CR, (i % CR) as f32 / CR as f32);
                        amp[j] + (amp[j + 1] - amp[j]) * w
                    };
                    let a = if am.is_empty() || !sides.is_empty() { a } else { a * am[i] };
                    let a = if hm.is_empty() { a } else { a * hm[i] };
                    *o += wamp * a * rs;
                    let r = rc * dc - rs * ds;
                    rs = rs * dc + rc * ds;
                    rc = r;
                }
                continue;
            }
            for (i, o) in out.iter_mut().enumerate().take(direct_end) {
                let mut c = ph.tick() + if sides.is_empty() { ph0 } else { 0.0 } + wph;
                if !scoop.is_empty() {
                    c += fr * scoop[i];
                }
                if !vib.is_empty() && sides.is_empty() {
                    c += fr * vib[i];
                }
                let a = if i >= hold {
                    amp[steps - 1]
                } else {
                    let (j, w) = (i / CR, (i % CR) as f32 / CR as f32);
                    amp[j] + (amp[j + 1] - amp[j]) * w
                };
                let a = if am.is_empty() || !sides.is_empty() { a } else { a * am[i] };
                let a = if hm.is_empty() { a } else { a * hm[i] };
                *o += wamp * a * sin_cycles(c);
            }
        }
    }
    if frozen_fft && fre.iter().chain(&fim).any(|v| *v != 0.0) {
        let l = fre.len();
        crate::correct::fft::fft(&mut fre, &mut fim, true);
        for (i, o) in out.iter_mut().enumerate().skip(hold.min(n)) {
            *o += fim[i % l];
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::params::{Layer, Params};
    use crate::render::{render, WaveMeta};
    use crate::schema::{family, Region};

    /// The frozen sustain by FFT (design D9) equals the direct per-sample sum: the same layer with a negligible
    /// loudness modulation (≈ 1e-7, forces the direct path) differs by < 1e-4 of the peak, and the FFT render loops
    /// exactly.
    #[test]
    fn frozen_sustain_fft_matches_direct() {
        let m = WaveMeta { rate: 22050, len: 9000, unity: 48, fine: 0, looped: Some((2000, 7000)) };
        let mk = |am: f32| {
            let mut l = Layer::new(family("additive").unwrap(), Region::Sustain);
            l.set("level_db", -12.0);
            l.set("spread", 30.0);
            l.set("a", 0.05);
            l.set("d", 0.2);
            l.set("s", 0.6);
            l.tables[0] = vec![0.0, -3.0, -6.0, -4.0, -12.0, -9.0, -15.0, -20.0];
            if am > 0.0 {
                l.set("vib_rate", 5.0);
                l.set("am_db", am);
            }
            let mut p = Params::default();
            p.layers.push(l);
            render(&m, &p, 3)
        };
        let (fast, direct) = (mk(0.0), mk(1e-5));
        let peak = direct.iter().map(|v| v.abs()).fold(0.0, f32::max);
        let err = fast.iter().zip(&direct).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(peak > 0.01 && err < 1e-4 * peak, "err {err} peak {peak}");
        // Loop exact: the last loop sample is followed by the loop's first one.
        let (first, last) = (fast[2000], fast[8999]);
        let step = (first - last).abs();
        let typical = (2001..8999).map(|i| (fast[i] - fast[i - 1]).abs()).fold(0.0, f32::max);
        assert!(step <= typical, "wrap step {step} vs largest step {typical}");
    }
}
