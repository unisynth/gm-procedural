//! FM analysis (task 7.6, design D4q): candidates for the `fm` family.
//! - Chirp cycle (a bird tweet train): one partial sweeping in a repeating pattern. The loop is tracked as an
//!   analytic signal (FFT band around the dominant partial, loop tiled ×3 so the ends wrap), the number of cycles per
//!   loop is the shortest shift that repeats the level track, and the cycles are averaged into a 32-point
//!   [cents, dB] `cyc` table on a sine carrier. The head before the train becomes an `attack` layer: a steady tone
//!   with its start glide.
//! - Operator presets (EP2, tubular bell): DX7-style pairs whose indices and decays come from a scored grid (the fitter
//!   tunes scalars only).
use crate::analyze::{calibrate, open_for_fit, rms};
use crate::audio::loop_of;
use gmgen::params::{Layer, Params};
use gmgen::render::WaveMeta;
use gmgen::schema::{family, Region};
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

/// Points in a `cyc` table.
const POINTS: usize = 32;

/// Analytic signal of circular `x` restricted to `lo..hi` Hz.
fn analytic(x: &[f32], rate: f32, lo: f32, hi: f32) -> Vec<Complex32> {
    let n = x.len();
    let mut planner = FftPlanner::<f32>::new();
    let mut b: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    planner.plan_fft_forward(n).process(&mut b);
    for (k, c) in b.iter_mut().enumerate() {
        let f = k as f32 * rate / n as f32;
        *c = if k <= n / 2 && f >= lo && f < hi { *c * 2.0 / n as f32 } else { Complex32::new(0.0, 0.0) };
    }
    planner.plan_fft_inverse(n).process(&mut b);
    b
}

/// `analytic` for other analyses (formant pitch tracks).
pub fn analytic_pub(x: &[f32], rate: f32, lo: f32, hi: f32) -> Vec<Complex32> {
    analytic(x, rate, lo, hi)
}

/// Frequency (Hz, amplitude-weighted over ±`w` samples) and amplitude tracks of the analytic signal `z`.
fn tracks(z: &[Complex32], rate: f32, w: usize) -> (Vec<f32>, Vec<f32>) {
    let n = z.len();
    let d: Vec<Complex32> = (0..n).map(|i| z[i] * z[(i + n - 1) % n].conj()).collect();
    let hz = (0..n)
        .map(|i| {
            let s: Complex32 = (0..=2 * w).map(|k| d[(i + n + k - w) % n]).sum();
            s.im.atan2(s.re) * rate / std::f32::consts::TAU
        })
        .collect();
    (hz, z.iter().map(|c| c.norm()).collect())
}

/// Average power spectrum peak (Hz) of `x` and the share of power within ×/÷ 1.8 of it.
fn dominant(x: &[f32], rate: f32) -> (f32, f32) {
    let n = x.len();
    let mut b: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    FftPlanner::<f32>::new().plan_fft_forward(n).process(&mut b);
    let p: Vec<f32> = b[..n / 2].iter().map(|c| c.norm_sqr()).collect();
    // Peak of the power smoothed over ±200 Hz (a sweep spreads it).
    let w = ((200.0 * n as f32 / rate) as usize).max(1);
    let sm: Vec<f32> = (0..p.len()).map(|k| p[k.saturating_sub(w)..(k + w + 1).min(p.len())].iter().sum()).collect();
    let k = (1..sm.len()).max_by(|a, b| sm[*a].total_cmp(&sm[*b])).unwrap_or(1);
    let f = k as f32 * rate / n as f32;
    let tot: f32 = p.iter().sum::<f32>().max(1e-20);
    let near: f32 = p.iter().enumerate().filter(|(j, _)| (*j as f32 * rate / n as f32 / f).log2().abs() < 1.8f32.log2()).map(|x| x.1).sum();
    (f, near / tot)
}

/// Cycle table of one loop: (cycles per loop, base Hz, [cents, dB] × `POINTS`).
pub fn chirp_cycle(lp: &[f32], rate: f32) -> Option<(usize, f32, Vec<f32>)> {
    let l = lp.len();
    let (f, share) = dominant(lp, rate);
    if share < 0.7 || f < 200.0 {
        return None;
    }
    let tiled: Vec<f32> = lp.iter().chain(lp).chain(lp).copied().collect();
    let z = analytic(&tiled, rate, f / 1.8, (f * 1.8).min(0.49 * rate));
    let (hz, amp) = tracks(&z, rate, ((0.0005 * rate) as usize).max(1));
    let (hz, amp) = (&hz[l..2 * l], &amp[l..2 * l]);
    let top = amp.iter().copied().fold(0.0f32, f32::max).max(1e-9);
    let ldb: Vec<f32> = amp.iter().map(|a| (20.0 * (a / top).log10()).max(-40.0)).collect();
    // Cycles per loop: the most whose shift L/m repeats the level track (r ≥ 0.8); 1 when none.
    let corr = |sh: usize| {
        let m = ldb.iter().sum::<f32>() / l as f32;
        let (mut num, mut den) = (0.0f32, 0.0f32);
        for i in 0..l {
            num += (ldb[i] - m) * (ldb[(i + sh) % l] - m);
            den += (ldb[i] - m).powi(2);
        }
        num / den.max(1e-9)
    };
    let cycles = (1..=8).rev().find(|&m| l / m >= (0.004 * rate) as usize && corr(l / m) >= 0.8).unwrap_or(1);
    // Movement must matter: a steady tone is the additive/modal families' job.
    let w2: Vec<f32> = amp.iter().map(|a| a * a).collect();
    let wsum = w2.iter().sum::<f32>().max(1e-12);
    let base = (hz.iter().zip(&w2).map(|(h, w)| h.max(1.0).log2() * w).sum::<f32>() / wsum).exp2();
    let spread = (hz.iter().zip(&w2).map(|(h, w)| (1200.0 * (h.max(1.0) / base).log2()).powi(2) * w).sum::<f32>() / wsum).sqrt();
    if spread < 100.0 {
        return None;
    }
    let per = l as f32 / cycles as f32;
    let mut tab = Vec::with_capacity(2 * POINTS);
    let mut last_c = 0.0f32;
    for p in 0..POINTS {
        let (mut pw, mut fw, mut n) = (0.0f32, 0.0f32, 0usize);
        for c in 0..cycles {
            let a = (c as f32 * per + p as f32 * per / POINTS as f32) as usize;
            let b = (c as f32 * per + (p + 1) as f32 * per / POINTS as f32) as usize;
            for i in a..b.min(l) {
                pw += w2[i];
                fw += w2[i] * (1200.0 * (hz[i].max(1.0) / base).log2()).clamp(-4800.0, 4800.0);
                n += 1;
            }
        }
        let gdb = (10.0 * (pw / n.max(1) as f32 / (top * top)).log10()).clamp(-60.0, 24.0);
        // Pitch of near-silent points is noise: hold the last audible one.
        let cents = if gdb > -30.0 && pw > 0.0 { fw / pw } else { last_c };
        last_c = cents;
        tab.push(cents);
        tab.push(gdb);
    }
    Some((cycles, base, tab))
}

fn fm_layer(region: Region, freq: f32) -> Layer {
    let mut l = Layer::new(family("fm").unwrap(), region);
    l.set("freq", freq);
    l.tables[0] = vec![1.0, 0.0, 1.0, 1.0];
    l
}

/// Chirp-cycle candidates (looped waves): the train on its own and with the head as an attack tone, for train starts
/// 0–4 cycles before the loop (or from the start).
pub fn chirp_candidates(i: usize, orig: &[f32], m: &WaveMeta) -> Vec<Params> {
    let rate = m.rate as f32;
    let Some((s, l)) = loop_of(m) else { return Vec::new() };
    if l < (0.01 * rate) as usize {
        return Vec::new();
    }
    let Some((cycles, base, tab)) = chirp_cycle(&orig[s..s + l], rate) else { return Vec::new() };
    let cyc_hz = cycles as f32 * rate / l as f32;
    let ts = s as f32 / rate;
    let mut train = fm_layer(Region::Sustain, base);
    train.tables[1] = tab;
    train.set("cyc_hz", cyc_hz);
    train.set("cyc_phase", (1.0 - (cyc_hz * ts).fract()).fract());
    let per = l / cycles;
    let mut out = Vec::new();
    let starts: Vec<usize> = (0..=4).filter_map(|k| s.checked_sub(k * per)).chain([0]).collect();
    for &st in &starts {
        let mut t = train.clone();
        // The envelope freezes at the loop start: be fully up by then.
        t.set("dly", (st as f32 / rate - 0.001).max(0.0));
        t.set("a", 0.001);
        open_for_fit(&mut t);
        let mut p = Params::default();
        p.layers.push(t);
        calibrate(i, &mut p, 0, rms(&orig[s..s + l]), s..s + l);
        if p.validate().is_ok() {
            out.push(p.clone());
        }
        // The head before the train: a steady tone with its start glide.
        let head = &orig[..st];
        if st >= (0.02 * rate) as usize && rms(head) > 0.05 * rms(&orig[s..s + l]) {
            let (f, _) = dominant(head, rate);
            let z = analytic(head, rate, f / 1.8, (f * 1.8).min(0.49 * rate));
            let (hz, amp) = tracks(&z, rate, ((0.001 * rate) as usize).max(1));
            let top = amp.iter().copied().fold(0.0f32, f32::max).max(1e-9);
            let loud: Vec<usize> = (0..st).filter(|&k| amp[k] > 0.3 * top).collect();
            let (Some(&k0), Some(&k1)) = (loud.first(), loud.last()) else { continue };
            let mut hs: Vec<f32> = loud.iter().map(|&k| hz[k]).collect();
            hs.sort_by(f32::total_cmp);
            let fh = hs[hs.len() / 2];
            let k5 = (k0 + (0.005 * rate) as usize).min(st - 1);
            let glide = (1200.0 * (hz[k0..=k5].iter().sum::<f32>() / (k5 - k0 + 1) as f32 / fh).log2()).clamp(-2400.0, 2400.0);
            let mut a = fm_layer(Region::Attack, fh);
            let kp = (k0..=k1).max_by(|x, y| amp[*x].total_cmp(&amp[*y])).unwrap_or(k0);
            a.set("a", (kp as f32 / rate).max(0.001));
            a.set("h", ((k1 - kp) as f32 / rate).max(0.0));
            a.set("d", 0.004);
            a.set("s", 0.0);
            a.set("glide", glide);
            a.set("glide_t", ((k1 - k0) as f32 / rate / 3.0).clamp(0.001, 2.0));
            open_for_fit(&mut a);
            let mut q = p.clone();
            q.layers.insert(0, a);
            calibrate(i, &mut q, 0, rms(&orig[k0..=k1]), k0..k1 + 1);
            if q.validate().is_ok() {
                out.push(q);
            }
        }
    }
    if std::env::var_os("GMFIT_TRACE").is_some() {
        eprintln!("fm chirp: {cycles} cycles/loop ({cyc_hz:.1} Hz), base {base:.0} Hz, {} candidates", out.len());
    }
    out
}

/// Operator presets [algo, ops rows…] (rows: ratio, amount, index decay, index sustain).
const PRESETS: &[(&str, f32, &[[f32; 4]])] = &[
    // DX7-style electric piano: body pair 1→1, tine pair 1→14 with a fast index decay.
    ("ep", 1.0, &[[1.0, 0.0, 1.0, 0.3], [1.0, 1.2, 1.0, 0.2], [1.0, -12.0, 1.0, 1.0], [14.0, 1.5, 0.05, 0.0]]),
    // STK TubeBell: two detuned pairs at 1 : 1.414.
    ("tubebell", 1.0, &[[0.995, 0.0, 1.0, 1.0], [1.407, 2.0, 2.0, 0.3], [1.005, -3.0, 1.0, 1.0], [1.414, 1.5, 1.0, 0.3]]),
    // A 1 : 3.5 bell pair.
    ("bell35", 0.0, &[[1.0, 0.0, 1.0, 1.0], [3.5, 2.0, 2.0, 0.2]]),
];

/// One-shot envelope from the 2 ms RMS curve: (attack s, decay to 1/e s).
fn shot_env(x: &[f32], rate: f32) -> (f32, f32) {
    let frame = ((0.002 * rate) as usize).max(1);
    let env: Vec<f32> = x.chunks(frame).map(rms).collect();
    let (kp, pk) = env.iter().enumerate().fold((0, 0.0f32), |b, (k, v)| if *v > b.1 { (k, *v) } else { b });
    let k1 = env[kp..].iter().position(|v| *v < pk / std::f32::consts::E).unwrap_or(env.len() - kp);
    (((kp as f32 + 0.5) * frame as f32 / rate).max(0.0005), (k1 as f32 * frame as f32 / rate).max(0.005))
}

/// Builds the preset's layers: one `whole` layer, or on a looped wave an `attack` layer (the index decays) crossfaded
/// into a `sustain` layer (indices frozen at the loop start).
fn preset_params(i: usize, orig: &[f32], m: &WaveMeta, algo: f32, ops: &[f32], c2_db: f32) -> Params {
    let rate = m.rate as f32;
    let (a, d) = shot_env(orig, rate);
    let mut ops = ops.to_vec();
    if ops.len() >= 12 {
        ops[9] = c2_db;
    }
    let mk = |region: Region| {
        let mut l = Layer::new(family("fm").unwrap(), region);
        l.tables[0] = ops.clone();
        l.set("algo", algo);
        l.set("a", a);
        l.set("h", 0.0);
        l.set("d", d);
        l.set("s", 0.0);
        open_for_fit(&mut l);
        l
    };
    let mut p = Params::default();
    match loop_of(m) {
        Some((s, _)) => {
            p.layers.push(mk(Region::Attack));
            let mut l = mk(Region::Sustain);
            let fade = 0.01f32.min(s as f32 / rate);
            l.set("dly", s as f32 / rate - fade);
            l.set("a", fade.max(0.0005));
            l.set("d", 30.0);
            l.set("s", 1.0);
            open_for_fit(&mut l);
            p.layers.push(l);
            let lp = loop_of(m).unwrap();
            calibrate(i, &mut p, 0, rms(&orig[..s.max(1)]), 0..s.max(1));
            calibrate(i, &mut p, 1, rms(&orig[lp.0..lp.0 + lp.1]), lp.0..lp.0 + lp.1);
        }
        None => {
            p.layers.push(mk(Region::Whole));
            calibrate(i, &mut p, 0, rms(orig), 0..orig.len());
        }
    }
    p
}

/// Operator-preset candidates: each preset with its modulator indices, index decays and sustains and the second
/// carrier's level set by scored coordinate descent (fit objective; the CMA-ES fitter tunes scalars only).
pub fn preset_candidates(i: usize, orig: &[f32], m: &WaveMeta) -> Vec<Params> {
    let mut out = Vec::new();
    for (name, algo, rows) in PRESETS {
        let mut ops: Vec<f32> = rows.iter().flatten().copied().collect();
        let mut c2 = if ops.len() >= 12 { ops[9] } else { 0.0 };
        let eval = |ops: &[f32], c2: f32| {
            let p = preset_params(i, orig, m, *algo, ops, c2);
            (crate::fit::score_of(orig, i, &p), p)
        };
        let mut best = eval(&ops, c2);
        for _round in 0..2 {
            // modulators: rows whose op modulates another (stack: 1..; pairs: 1 and 3)
            let mods: Vec<usize> = if *algo == 1.0 { vec![1, 3] } else { (1..rows.len()).collect() };
            for &r in mods.iter().filter(|r| **r < rows.len()) {
                for idx in [0.3f32, 0.7, 1.2, 2.0, 3.5, 6.0] {
                    for (dec, sus) in [(0.05f32, 0.0f32), (0.3, 0.1), (1.0, 0.3), (3.0, 0.6)] {
                        let mut o = ops.clone();
                        o[4 * r + 1] = idx;
                        o[4 * r + 2] = dec;
                        o[4 * r + 3] = sus;
                        let e = eval(&o, c2);
                        if e.0 > best.0 {
                            best = e;
                            ops = o;
                        }
                    }
                }
            }
            if ops.len() >= 12 {
                for g in [-24.0f32, -12.0, -6.0, 0.0] {
                    let e = eval(&ops, g);
                    if e.0 > best.0 {
                        best = e;
                        c2 = g;
                    }
                }
            }
        }
        if std::env::var_os("GMFIT_TRACE").is_some() {
            eprintln!("fm preset {name}: {:.4} ops {ops:?}", best.0);
        }
        if best.1.validate().is_ok() {
            out.push(best.1);
        }
    }
    out
}

/// All `fm` candidates for wave `i`.
pub fn candidates(i: usize, orig: &[f32], m: &WaveMeta) -> Vec<Params> {
    let mut v = chirp_candidates(i, orig, m);
    v.extend(preset_candidates(i, orig, m));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The analysis recovers a rendered tweet train: cycles per loop, base pitch within 3 %, the sweep depth within
    /// 150 cents.
    #[test]
    fn recovers_a_tweet_train() {
        let m = WaveMeta { rate: 22050, len: 6000, unity: 60, fine: 0, looped: Some((1000, 1500)) };
        let mut l = fm_layer(Region::Sustain, 6000.0);
        l.set("cyc_hz", 2.0 * 22050.0 / 1500.0);
        // down-sweep +600 → −600 cents over half the cycle, then a quiet gap
        let mut cyc = Vec::new();
        for p in 0..16 {
            let (c, g) = if p < 8 { (600.0 - 1200.0 * p as f32 / 7.0, 0.0) } else { (-600.0, -40.0) };
            cyc.extend([c, g]);
        }
        l.tables[1] = cyc;
        let mut p = Params::default();
        p.layers.push(l);
        let x = gmgen::render::render(&m, &p, 1);
        let (cycles, base, tab) = chirp_cycle(&x[1000..2500], 22050.0).expect("a chirp");
        assert_eq!(cycles, 2);
        let loud: Vec<f32> = tab.chunks(2).filter(|r| r[1] > -10.0).map(|r| r[0]).collect();
        let (hi, lo) = (loud.iter().copied().fold(f32::MIN, f32::max), loud.iter().copied().fold(f32::MAX, f32::min));
        let depth = hi - lo;
        assert!((depth - 1200.0).abs() < 150.0, "sweep depth {depth} cents ({tab:?})");
        let mid = base * ((hi + lo) / 2.0 / 1200.0).exp2();
        assert!((mid / 6000.0 - 1.0).abs() < 0.03, "centre {mid} Hz");
    }
}
