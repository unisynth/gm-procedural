//! Formant analysis (task 7.3, design D4r): candidates for the `formant` family.
//! - Formants: LPC envelope (order 2 + rate/1000, pre-emphasis, averaged autocorrelation of the voiced frames),
//!   ≤ 5 peaks in 200 Hz…0.45·rate with −3 dB bandwidths; gains re-levelled against the original band by band
//!   (3 render passes: the fitter tunes scalars only).
//! - Syllables (unlooped voice: a laugh): 5 ms envelope peaks ≥ 8 dB over the valley before, ≥ 60 ms apart, within
//!   30 dB of the loudest; pitch per syllable by subharmonic summation (octave errors pulled to the median), decay =
//!   median −10 dB fall / 1.15, pitch fall from the syllable's two halves; breath calibrated on the gaps.
//! - Looped voice (choir): one sustain layer on the wave's pitch, formants from the loop.
use crate::analyze::{avg_power, calibrate, open_for_fit, rms};
use crate::audio::loop_of;
use gmgen::params::{Layer, Params};
use gmgen::render::WaveMeta;
use gmgen::schema::{family, Region};

/// Averaged LPC envelope (dB, `n` points over 0…rate/2) of the frames starting at `starts` (`len` samples each).
pub fn lpc_envelope(x: &[f32], starts: &[usize], len: usize, rate: f32, n: usize) -> Vec<f32> {
    let order = (2.0 + rate / 1000.0) as usize;
    let mut r = vec![0.0f64; order + 1];
    for &s in starts {
        let seg: Vec<f64> = (s..(s + len).min(x.len()))
            .map(|i| {
                let pre = x[i] as f64 - 0.97 * if i > 0 { x[i - 1] as f64 } else { 0.0 };
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * (i - s) as f64 / len as f64).cos();
                pre * w
            })
            .collect();
        for (k, rk) in r.iter_mut().enumerate() {
            *rk += seg.iter().zip(seg.iter().skip(k)).map(|(a, b)| a * b).sum::<f64>();
        }
    }
    r[0] *= 1.0 + 1e-6;
    // Levinson-Durbin.
    let mut a = vec![0.0f64; order + 1];
    a[0] = 1.0;
    let mut err = r[0].max(1e-20);
    for i in 1..=order {
        let acc: f64 = (1..i).map(|j| a[j] * r[i - j]).sum::<f64>() + r[i];
        let k = -acc / err;
        let prev = a.clone();
        for j in 1..i {
            a[j] = prev[j] + k * prev[i - j];
        }
        a[i] = k;
        err *= 1.0 - k * k;
    }
    (0..n)
        .map(|q| {
            let w = std::f64::consts::PI * q as f64 / n as f64;
            let (re, im) = a.iter().enumerate().fold((0.0, 0.0), |(re, im), (k, c)| (re + c * (w * k as f64).cos(), im - c * (w * k as f64).sin()));
            (-10.0 * (re * re + im * im).max(1e-20).log10()) as f32
        })
        .collect()
}

/// ≤ 5 formants [Hz, bandwidth, dB re the strongest] from an LPC envelope, the pre-emphasis (+6 dB/oct) removed.
pub fn formants(env: &[f32], rate: f32) -> Vec<f32> {
    let n = env.len();
    let hz = |q: usize| q as f32 * rate / 2.0 / n as f32;
    let de: Vec<f32> = (0..n).map(|q| env[q] - 20.0 * (2.0 * (std::f32::consts::PI * hz(q) / rate).sin()).max(1e-3).log10()).collect();
    let mut peaks: Vec<(usize, f32)> = (1..n - 1)
        .filter(|&q| env[q] > env[q - 1] && env[q] >= env[q + 1] && hz(q) >= 200.0 && hz(q) <= 0.45 * rate)
        .map(|q| (q, de[q]))
        .collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    peaks.truncate(5);
    peaks.sort_by_key(|p| p.0);
    let top = peaks.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    let mut out = Vec::new();
    for (q, d) in peaks {
        let lo = (0..q).rev().find(|&j| env[j] < env[q] - 3.0).unwrap_or(0);
        let hi = (q..n).find(|&j| env[j] < env[q] - 3.0).unwrap_or(n - 1);
        let bw = (hz(hi) - hz(lo)).clamp(40.0, 800.0);
        out.extend([hz(q), bw, (d - top).max(-40.0)]);
    }
    out
}

/// Formants of a high voice (harmonics too sparse for LPC: it locks onto them): harmonic amplitudes at k·f0 of each
/// frame (k ≤ 0.45·rate), averaged in dB on a 50 Hz grid by linear interpolation between harmonics, ≤ 5 peaks;
/// bandwidth max(150 Hz, 0.5·f0) (unresolvable below the harmonic spacing).
pub fn harmonic_formants(x: &[f32], frames: &[(usize, f32)], len: usize, rate: f32) -> Vec<f32> {
    let grid: Vec<f32> = (4..((0.45 * rate) / 50.0) as usize).map(|k| k as f32 * 50.0).collect();
    let mut acc = vec![0.0f32; grid.len()];
    let mut f0m = 0.0f32;
    for &(st, f0) in frames {
        let p = avg_power(&x[st..(st + len).min(x.len())]);
        let n = (p.len() - 1) * 2;
        let amp = |f: f32| {
            let k = (f * n as f32 / rate).round() as usize;
            10.0 * p[k.saturating_sub(1).min(p.len() - 1)..(k + 2).min(p.len())].iter().copied().fold(0.0f32, f32::max).max(1e-20).log10()
        };
        let pts: Vec<(f32, f32)> = (1..).map(|k| k as f32 * f0).take_while(|f| *f < 0.45 * rate).map(|f| (f, amp(f))).collect();
        for (g, a) in grid.iter().zip(acc.iter_mut()) {
            let v = match pts.iter().position(|q| q.0 >= *g) {
                Some(0) | None => pts.first().map_or(-200.0, |q| q.1).min(pts.last().map_or(-200.0, |q| q.1)),
                Some(j) => {
                    let (u, w) = (pts[j - 1], pts[j]);
                    u.1 + (w.1 - u.1) * (g - u.0) / (w.0 - u.0)
                }
            };
            *a += v / frames.len() as f32;
        }
        f0m += f0 / frames.len() as f32;
    }
    let mut peaks: Vec<(usize, f32)> = (1..grid.len() - 1).filter(|&q| acc[q] > acc[q - 1] && acc[q] >= acc[q + 1]).map(|q| (q, acc[q])).collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    peaks.truncate(5);
    peaks.sort_by_key(|p| p.0);
    let top = peaks.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    peaks.iter().flat_map(|&(q, d)| [grid[q], (0.5 * f0m).max(150.0), (d - top).max(-40.0)]).collect()
}

/// Pitch track (Hz per sample, amplitude-weighted ±1 ms) of the stronger of harmonics 1–2 of `f0` in `x`.
fn pitch_track(x: &[f32], rate: f32, f0: f32) -> Vec<f32> {
    let p = avg_power(x);
    let n = (p.len() - 1) * 2;
    let e = |f: f32| {
        let k = (f * n as f32 / rate) as usize;
        p[k.saturating_sub(2).min(p.len() - 1)..(k + 3).min(p.len())].iter().sum::<f32>()
    };
    // Harmonic 1 or 2, a band of ±f0/2 (wider bands take in the neighbours and flatten the track).
    let h = (1..=2).filter(|h| *h as f32 * f0 < 0.45 * rate).max_by(|a, b| e(*a as f32 * f0).total_cmp(&e(*b as f32 * f0))).unwrap_or(1) as f32;
    let z = crate::fm::analytic_pub(x, rate, h * f0 - f0 / 2.0, h * f0 + f0 / 2.0);
    let w = ((0.001 * rate) as usize).max(1);
    let nz = z.len();
    (0..nz)
        .map(|i| {
            let s: rustfft::num_complex::Complex32 = (i.saturating_sub(w)..(i + w + 1).min(nz)).filter(|&j| j > 0).map(|j| z[j] * z[j - 1].conj()).sum();
            s.im.atan2(s.re) * rate / std::f32::consts::TAU / h
        })
        .collect()
}

/// Pitch (Hz) of `x` by subharmonic summation over 80…1000 Hz.
fn shs(x: &[f32], rate: f32) -> f32 {
    let p = avg_power(x);
    let n = (p.len() - 1) * 2;
    let at = |f: f32| {
        let k = (f * n as f32 / rate) as usize;
        if k + 1 < p.len() { p[k].max(p[k + 1]).sqrt() } else { 0.0 }
    };
    (80..1000).map(|f| f as f32).max_by(|a, b| {
        let s = |f: f32| (1..=8).map(|k| 0.84f32.powi(k - 1) * at(k as f32 * f)).sum::<f32>();
        s(*a).total_cmp(&s(*b))
    }).unwrap_or(200.0)
}

/// Syllables of an unlooped voice: (sample, level dB) of each 5 ms envelope peak.
pub fn syllables(x: &[f32], rate: f32) -> Vec<(usize, f32)> {
    let fr = ((0.005 * rate) as usize).max(1);
    let e: Vec<f32> = x.chunks(fr).map(|c| 20.0 * rms(c).max(1e-9).log10()).collect();
    let top = e.iter().copied().fold(f32::MIN, f32::max);
    let gap = (0.06 * rate) as usize / fr;
    let mut out: Vec<(usize, f32)> = Vec::new();
    let mut valley = f32::MAX;
    for k in 1..e.len().saturating_sub(1) {
        valley = valley.min(e[k]);
        let local = e[k] >= e[k - 1] && e[k] >= e[k + 1] && e[k] >= *e[k.saturating_sub(gap)..(k + gap).min(e.len())].iter().max_by(|a, b| a.total_cmp(b)).unwrap();
        if local && e[k] > top - 30.0 && (out.is_empty() || e[k] - valley >= 8.0) {
            if out.last().is_none_or(|l| k * fr - l.0 >= gap * fr) {
                out.push((k * fr, e[k]));
                valley = f32::MAX;
            }
        }
    }
    out.into_iter().take(16).collect()
}

/// Re-levels each formant against the original: 3 passes of the band-energy ratio around each formant.
fn relevel(i: usize, orig: &[f32], p: &mut Params, k: usize, range: std::ops::Range<usize>, rate: f32) {
    let band = |x: &[f32], f: f32, bw: f32| {
        let pw = avg_power(&x[range.clone()]);
        let n = (pw.len() - 1) * 2;
        let (lo, hi) = (((f - bw) * n as f32 / rate).max(0.0) as usize, ((f + bw) * n as f32 / rate) as usize);
        pw[lo.min(pw.len() - 1)..hi.min(pw.len() - 1).max(lo.min(pw.len() - 1) + 1)].iter().sum::<f32>().max(1e-20)
    };
    for _ in 0..3 {
        let y = crate::render_wave(i, p);
        let t = &mut p.layers[k].tables[0];
        for r in t.chunks_mut(3) {
            let d = 10.0 * (band(orig, r[0], r[1]) / band(&y, r[0], r[1])).log10();
            r[2] = (r[2] + d.clamp(-12.0, 12.0)).clamp(-60.0, 24.0);
        }
        let top = t.chunks(3).map(|r| r[2]).fold(f32::MIN, f32::max);
        t.chunks_mut(3).for_each(|r| r[2] -= top);
        calibrate(i, p, k, rms(&orig[range.clone()]), range.clone());
    }
}

/// All `formant` candidates for wave `i`; `wave` = the current model's wave scalars (its pitch).
pub fn candidates(i: usize, orig: &[f32], m: &WaveMeta, wave: &[f32]) -> Vec<Params> {
    let rate = m.rate as f32;
    let mut out = Vec::new();
    if let Some((s, l)) = loop_of(m) {
        // Looped voice: formants from the loop, the wave's pitch.
        if l < (0.02 * rate) as usize {
            return out;
        }
        let w = ((0.03 * rate) as usize).min(l);
        let starts: Vec<usize> = (s..s + l - w + 1).step_by(w / 2 + 1).collect();
        let fs = formants(&lpc_envelope(orig, &starts, w, rate, 1024), rate);
        for (pw, vib) in [(0.0f32, 0.0f32), (0.0, 1.0), (0.3, 1.0)] {
            let mut lay = Layer::new(family("formant").unwrap(), Region::Sustain);
            lay.tables[0] = fs.clone();
            lay.set("pw", pw);
            if vib > 0.0 {
                lay.set("vib_rate", 5.5);
                lay.set("vib_cents", 15.0);
            }
            lay.set("a", 0.05);
            open_for_fit(&mut lay);
            let mut p = Params::default();
            p.wave = wave.to_vec();
            p.wave_explicit = vec![false, true];
            p.layers.push(lay);
            calibrate(i, &mut p, 0, rms(&orig[s..s + l]), s..s + l);
            relevel(i, orig, &mut p, 0, s..s + l, rate);
            if p.validate().is_ok() {
                out.push(p);
            }
        }
        return out;
    }
    // Voiced syllables only: normalised autocorrelation ≥ 0.5 at a 1–12.5 ms lag over the 25 ms after the peak.
    let voiced = |at: usize| {
        let seg = &orig[at..(at + (0.025 * rate) as usize).min(orig.len())];
        let e0: f32 = seg.iter().map(|v| v * v).sum::<f32>().max(1e-12);
        ((0.001 * rate) as usize..((0.0125 * rate) as usize).min(seg.len() / 2))
            .map(|l| seg.iter().zip(&seg[l..]).map(|(a, b)| a * b).sum::<f32>() / e0)
            .fold(0.0f32, f32::max)
            >= 0.5
    };
    let syl: Vec<(usize, f32)> = syllables(orig, rate).into_iter().filter(|s| voiced(s.0)).collect();
    if syl.len() < 2 {
        return out;
    }
    let w = (0.03 * rate) as usize;
    let starts: Vec<usize> = syl.iter().map(|s| s.0.saturating_sub(w / 3)).filter(|s| s + w <= orig.len()).collect();
    let mut f0s: Vec<f32> = starts.iter().map(|&st| shs(&orig[st..st + w], rate)).collect();
    let mut sorted = f0s.clone();
    sorted.sort_by(f32::total_cmp);
    let med = sorted[sorted.len() / 2];
    for f in &mut f0s {
        while *f > med * 1.5 {
            *f /= 2.0;
        }
        while *f < med / 1.5 {
            *f *= 2.0;
        }
    }
    let fs = if med > 250.0 {
        harmonic_formants(orig, &starts.iter().copied().zip(f0s.iter().copied()).collect::<Vec<_>>(), w, rate)
    } else {
        formants(&lpc_envelope(orig, &starts, w, rate, 1024), rate)
    };
    // Decay: median −10 dB fall after each peak.
    let fr = ((0.005 * rate) as usize).max(1);
    let e: Vec<f32> = orig.chunks(fr).map(|c| 20.0 * rms(c).max(1e-9).log10()).collect();
    let mut falls: Vec<f32> = syl.iter().filter_map(|&(at, d)| (at / fr..e.len()).find(|&k| e[k] < d - 10.0).map(|k| (k * fr - at) as f32 / rate)).collect();
    falls.sort_by(f32::total_cmp);
    let sd = falls.get(falls.len() / 2).map_or(0.04, |t| t / 1.15).clamp(0.005, 0.5);
    let top = syl.iter().map(|s| s.1).fold(f32::MIN, f32::max);
    // Pitch at each syllable's peak and its fall to the −10 dB point (the model falls 1 − e^{−1.15} = 0.68 of
    // `syl_glide` by then).
    let mut cents = Vec::new();
    let mut falls_c = Vec::new();
    for (&(at, d), f) in syl.iter().zip(&f0s) {
        let end = (at / fr..e.len()).find(|&k| e[k] < d - 10.0).map_or(orig.len(), |k| k * fr).min(orig.len());
        let seg_end = (at + (0.15 * rate) as usize).min(orig.len());
        if end <= at + 2 * fr {
            cents.push(1200.0 * (f / med).log2());
            continue;
        }
        let tr = pitch_track(&orig[at..seg_end], rate, *f);
        let near = |k: usize| {
            let a = k.saturating_sub(fr).min(tr.len() - 1);
            let b = (k + fr).min(tr.len());
            let mut v: Vec<f32> = tr[a..b.max(a + 1)].to_vec();
            v.sort_by(f32::total_cmp);
            v[v.len() / 2].max(1.0)
        };
        let (c0, c1) = (near(fr), near(end - at));
        cents.push(1200.0 * (c0 / med).log2());
        falls_c.push(1200.0 * (c0 / c1).log2() / 0.68);
    }
    falls_c.sort_by(f32::total_cmp);
    let glide = falls_c.get(falls_c.len() / 2).copied().unwrap_or(0.0).clamp(-2400.0, 2400.0);
    let tab: Vec<f32> = syl
        .iter()
        .zip(&cents)
        .flat_map(|(s, c)| [((s.0 as f32 / rate) - 0.004).max(0.0), s.1 - top, c.clamp(-2400.0, 2400.0)])
        .collect();
    // Breath: the floor between syllables (rolling minimum of the 5 ms envelope over ±40 ms) fitted with the layer
    // envelope (hold, decay, sustain; grid), its level set against the floor afterwards.
    let k40 = (0.04 * rate) as usize / fr;
    let floor_tr: Vec<f32> = (0..e.len()).map(|k| e[k.saturating_sub(k40)..(k + k40 + 1).min(e.len())].iter().copied().fold(f32::MAX, f32::min)).collect();
    let mut best_env = (f32::MAX, 0.0f32, 1.0f32, 1.0f32);
    for hi in 0..14 {
        let h = hi as f32 * 0.025;
        for d in [0.02f32, 0.05, 0.1, 0.2, 0.4, 0.8, 2.0] {
            for sv in [0.0f32, 0.1, 0.3, 0.6] {
                let model: Vec<f32> = (0..e.len()).map(|k| 20.0 * gmgen::render::env(k as f32 * fr as f32 / rate, 0.001, h, d, sv, 0.0).max(1e-5).log10()).collect();
                let off = floor_tr.iter().zip(&model).map(|(f, m)| f - m).sum::<f32>() / e.len() as f32;
                let err = floor_tr.iter().zip(&model).map(|(f, m)| (f - m - off).powi(2)).sum::<f32>();
                if err < best_env.0 {
                    best_env = (err, h, d, sv);
                }
            }
        }
    }
    let breath_rel = floor_tr.iter().copied().fold(f32::MIN, f32::max) - top;
    for (noise, pw) in [(0.0f32, 0.0f32), (0.25, 0.0), (0.5, 0.0), (0.25, 0.3)] {
        let mut lay = Layer::new(family("formant").unwrap(), Region::Whole);
        lay.tables[0] = fs.clone();
        lay.tables[1] = tab.clone();
        lay.set("f0", med);
        lay.set("pw", pw);
        lay.set("noise", noise);
        lay.set("syl_a", 0.004);
        lay.set("syl_d", sd);
        lay.set("syl_glide", glide);
        lay.set("a", 0.001);
        lay.set("h", best_env.1);
        lay.set("d", best_env.2);
        lay.set("s", best_env.3);
        lay.set("breath_db", (breath_rel + 6.0).clamp(-80.0, 0.0));
        open_for_fit(&mut lay);
        let mut p = Params::default();
        p.layers.push(lay);
        calibrate(i, &mut p, 0, rms(orig), 0..orig.len());
        relevel(i, orig, &mut p, 0, 0..orig.len(), rate);
        // Breath against the floor: 3 passes of the median floor difference.
        for _ in 0..3 {
            let y = crate::render_wave(i, &p);
            let ey: Vec<f32> = y.chunks(fr).map(|c| 20.0 * rms(c).max(1e-9).log10()).collect();
            let fy: Vec<f32> = (0..ey.len()).map(|k| ey[k.saturating_sub(k40)..(k + k40 + 1).min(ey.len())].iter().copied().fold(f32::MAX, f32::min)).collect();
            let mut d: Vec<f32> = floor_tr.iter().zip(&fy).map(|(a, b)| a - b).collect();
            d.sort_by(f32::total_cmp);
            let step = d[d.len() / 2].clamp(-12.0, 12.0);
            let l = &mut p.layers[0];
            let b = l.get("breath_db");
            l.set("breath_db", (b + step).clamp(-80.0, 24.0));
        }
        if p.validate().is_ok() {
            out.push(p);
        }
    }
    if std::env::var_os("GMFIT_TRACE").is_some() {
        eprintln!("formant: {} syllables, f0 {med:.0} Hz, decay {sd:.3}, glide {glide:.0} c, breath {breath_rel:.1} dB, formants {fs:?}, syl {tab:?}", syl.len());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LPC finds the formants of a rendered formant voice within 10 %.
    #[test]
    fn lpc_finds_formants() {
        let m = WaveMeta { rate: 22050, len: 22050, unity: 60, fine: 0, looped: None };
        let mut l = Layer::new(family("formant").unwrap(), Region::Whole);
        l.set("f0", 140.0);
        l.set("d", 30.0);
        l.tables[0] = vec![700.0, 90.0, 0.0, 1200.0, 110.0, -4.0, 2600.0, 160.0, -10.0];
        let mut p = Params::default();
        p.layers.push(l);
        let x = gmgen::render::render(&m, &p, 1);
        let starts: Vec<usize> = (2000..20000).step_by(1500).collect();
        let fs = formants(&lpc_envelope(&x, &starts, 1024, 22050.0, 1024), 22050.0);
        for want in [700.0f32, 1200.0, 2600.0] {
            assert!(fs.chunks(3).any(|r| (r[0] / want - 1.0).abs() < 0.1), "{want} Hz not found in {fs:?}");
        }
    }

    /// Syllables of a rendered laugh at their times (±10 ms).
    #[test]
    fn finds_syllables() {
        let m = WaveMeta { rate: 22050, len: 22050, unity: 60, fine: 0, looped: None };
        let mut l = Layer::new(family("formant").unwrap(), Region::Whole);
        l.set("f0", 300.0);
        l.set("d", 30.0);
        l.set("syl_d", 0.03);
        l.tables[0] = vec![800.0, 100.0, 0.0, 1800.0, 150.0, -6.0];
        l.tables[1] = vec![0.05, 0.0, 0.0, 0.25, -3.0, 0.0, 0.42, 0.0, -100.0, 0.6, -6.0, -200.0];
        let mut p = Params::default();
        p.layers.push(l);
        let x = gmgen::render::render(&m, &p, 1);
        let s = syllables(&x, 22050.0);
        let t: Vec<f32> = s.iter().map(|v| v.0 as f32 / 22050.0).collect();
        assert_eq!(t.len(), 4, "{t:?}");
        for (a, b) in t.iter().zip([0.05f32, 0.25, 0.42, 0.6]) {
            assert!((a - b).abs() < 0.015, "{t:?}");
        }
    }
}
