//! Morlet wavelet analysis (design D4m): one frequency band at a time with a window that scales with the
//! frequency (`cycles` periods), where a fixed STFT window either smears fast modulation or loses resolution.
//! - [`band`]: analytic band signal → amplitude and instantaneous frequency per sample.
//! - [`trill`]: frequency modulation of a partial (rate, depth) vs its amplitude modulation — a whistle's pea trill
//!   reads as FM; fixed modes at its sidebands would beat as AM.
//! - [`band_onsets`]: strikes detected inside the resonance band (a rattle's hits blur in the broadband envelope).
use rustfft::num_complex::Complex32;

/// Analytic Morlet band of `x` at `f` Hz with `cycles` periods of time width: `(amplitude, instantaneous Hz)` per
/// sample. Computed in the frequency domain: a Gaussian around `f` (sd `f / cycles`) on positive frequencies only.
pub fn band(x: &[f32], rate: f32, f: f32, cycles: f32) -> (Vec<f32>, Vec<f32>) {
    let n = x.len();
    if n < 4 {
        return (vec![0.0; n], vec![f; n]);
    }
    let nfft = (n + (4.0 * cycles * rate / f) as usize).next_power_of_two();
    let mut buf: Vec<Complex32> = (0..nfft).map(|k| Complex32::new(if k < n { x[k] } else { 0.0 }, 0.0)).collect();
    crate::plan::forward(nfft).process(&mut buf);
    let sd = f / cycles;
    for (k, c) in buf.iter_mut().enumerate() {
        let hz = k as f32 * rate / nfft as f32;
        *c *= if k > 0 && k < nfft / 2 { 2.0 * (-0.5 * ((hz - f) / sd).powi(2)).exp() } else { 0.0 };
    }
    crate::plan::inverse(nfft).process(&mut buf);
    let y: Vec<Complex32> = buf[..n].iter().map(|c| c / nfft as f32).collect();
    let amp: Vec<f32> = y.iter().map(|c| c.norm()).collect();
    let mut inst = vec![f; n];
    for k in 1..n {
        // Phase advance between samples, wrapped to (−π, π].
        let d = (y[k] * y[k - 1].conj()).arg();
        inst[k] = d * rate / std::f32::consts::TAU;
    }
    inst[0] = inst.get(1).copied().unwrap_or(f);
    (amp, inst)
}

/// Strongest periodicity (Hz) of `v` between `lo` and `hi`, with its share of the variance in that range.
pub(crate) fn mod_rate(v: &[f32], rate: f32, lo: f32, hi: f32) -> (f32, f32) {
    let n = v.len();
    let mean = v.iter().sum::<f32>() / n as f32;
    let nfft = n.next_power_of_two() * 2;
    let mut buf: Vec<Complex32> = (0..nfft)
        .map(|k| {
            let w = if k < n { 0.5 - 0.5 * (std::f32::consts::TAU * (k as f32 + 0.5) / n as f32).cos() } else { 0.0 };
            Complex32::new(if k < n { (v[k] - mean) * w } else { 0.0 }, 0.0)
        })
        .collect();
    crate::plan::forward(nfft).process(&mut buf);
    let hz = |k: usize| k as f32 * rate / nfft as f32;
    let ks: Vec<usize> = (1..nfft / 2).filter(|&k| hz(k) >= lo && hz(k) <= hi).collect();
    let tot: f32 = ks.iter().map(|&k| buf[k].norm_sqr()).sum();
    let Some(&kb) = ks.iter().max_by(|a, b| buf[**a].norm_sqr().total_cmp(&buf[**b].norm_sqr())) else { return (0.0, 0.0) };
    let near: f32 = ks.iter().filter(|&&k| k.abs_diff(kb) <= 2).map(|&k| buf[k].norm_sqr()).sum();
    (hz(kb), near / tot.max(1e-20))
}

/// Trill of the partial at `f` in `x`: `(rate Hz, depth cents, AM sd dB, mean Hz)` when the frequency moves periodically
/// (15–400 Hz, ≥ 20 cents sd, a clear peak) over the loud part; `None` otherwise.
pub fn trill(x: &[f32], rate: f32, f: f32) -> Option<(f32, f32, f32, f32)> {
    // ~4 cycles: about 2 ms at 2 kHz, short enough to follow a 100 Hz trill.
    let (amp, inst) = band(x, rate, f, (f / 250.0).max(4.0));
    let top = amp.iter().copied().fold(0.0f32, f32::max);
    // Loud part: from the first to the last sample within 12 dB of the peak, edges trimmed.
    let loud: Vec<usize> = (0..amp.len()).filter(|&k| amp[k] > top * 0.25).collect();
    let (a, b) = (*loud.first()?, *loud.last()?);
    let trim = ((b - a) / 10).max((0.005 * rate) as usize);
    if b < a + 2 * trim + (0.03 * rate) as usize {
        return None;
    }
    let (a, b) = (a + trim, b - trim);
    let cents: Vec<f32> = inst[a..b].iter().map(|v| 1200.0 * (v.max(1.0) / f).log2()).collect();
    let mean = cents.iter().sum::<f32>() / cents.len() as f32;
    let sd = (cents.iter().map(|c| (c - mean).powi(2)).sum::<f32>() / cents.len() as f32).sqrt();
    let (r, share) = mod_rate(&cents, rate, 15.0, 400.0);
    let am: Vec<f32> = amp[a..b].iter().map(|v| 20.0 * v.max(1e-9).log10()).collect();
    let am_mean = am.iter().sum::<f32>() / am.len() as f32;
    let am_sd = (am.iter().map(|v| (v - am_mean).powi(2)).sum::<f32>() / am.len() as f32).sqrt();
    // A sinusoidal deviation of peak d has sd d/√2.
    (sd >= 20.0 && share >= 0.3).then_some((r, sd * std::f32::consts::SQRT_2, am_sd, f * 2f32.powf(mean / 1200.0)))
}

/// Amplitude decay time constant (s) of the band around `f`: wavelet amplitude smoothed over `smooth` s,
/// least-squares dB slope from its peak down to −40 dB or the end; ≥ 0.05 s, 30 s when flat.
pub fn decay(x: &[f32], rate: f32, f: f32, smooth: f32) -> f32 {
    let (amp, _) = band(x, rate, f, 4.0);
    let w = ((smooth * rate) as usize).max(1);
    let db: Vec<f32> = amp.chunks(w).map(|c| 10.0 * (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).max(1e-18).log10()).collect();
    let Some(kp) = (0..db.len()).max_by(|a, b| db[*a].total_cmp(&db[*b])) else { return 30.0 };
    let pts: Vec<(f32, f32)> = db[kp..].iter().enumerate().take_while(|(_, v)| **v > db[kp] - 40.0).map(|(k, v)| (k as f32 * smooth, *v)).collect();
    if pts.len() < 3 {
        return 0.05;
    }
    let n = pts.len() as f32;
    let (mx, my) = (pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n);
    let slope = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f32>() / pts.iter().map(|p| (p.0 - mx).powi(2)).sum::<f32>().max(1e-12);
    if slope >= -0.3 { 30.0 } else { (-8.686 / slope).clamp(0.05, 30.0) }
}

/// Strikes inside the band around `f` (3-cycle wavelet): rises ≥ 6 dB within 2 ms, above peak − 30 dB, ≥ 8 ms apart.
/// Returns `(sample, band level dB at the strike peak)`.
pub fn band_onsets(x: &[f32], rate: f32, f: f32) -> Vec<(usize, f32)> {
    let (amp, _) = band(x, rate, f, 3.0);
    let a: Vec<f32> = amp.iter().map(|v| 20.0 * v.max(1e-9).log10()).collect();
    let top = a.iter().copied().fold(f32::MIN, f32::max);
    let h = ((0.002 * rate) as usize).max(1);
    let gap = (0.008 * rate) as usize;
    let mut out: Vec<(usize, f32)> = Vec::new();
    let mut k = h;
    while k < a.len() {
        if a[k] > top - 30.0 && a[k] - a[k - h] >= 6.0 && out.last().is_none_or(|o| k >= o.0 + gap) {
            // Strike peak within the next 4 ms.
            let end = (k + 2 * h).min(a.len());
            let kp = (k..end).max_by(|p, q| a[*p].total_cmp(&a[*q])).unwrap_or(k);
            out.push((k - h, a[kp]));
            k += gap;
        } else {
            k += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    const SR: f32 = 22050.0;

    #[test]
    fn trill_is_read_as_fm_not_am() {
        // 2 kHz, ±120 Hz at 100 Hz (≈ ±104 cents).
        let x: Vec<f32> = (0..(0.3 * SR) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                (std::f32::consts::TAU * (2000.0 * t + 120.0 / 100.0 * (std::f32::consts::TAU * 100.0 * t).sin() / std::f32::consts::TAU)).sin()
            })
            .collect();
        let (r, c, am, mean) = trill(&x, SR, 2000.0).expect("trill");
        assert!((mean - 2000.0).abs() < 10.0, "mean {mean}");
        assert!((r - 100.0).abs() < 5.0, "rate {r}");
        assert!((c - 104.0).abs() < 20.0, "cents {c}");
        assert!(am < 2.0, "am {am}");
        // Two steady tones beating at 100 Hz: AM, no trill.
        let y: Vec<f32> = (0..(0.3 * SR) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                (std::f32::consts::TAU * 2000.0 * t).sin() + 0.3 * (std::f32::consts::TAU * 2100.0 * t).sin()
            })
            .collect();
        assert!(trill(&y, SR, 2000.0).is_none());
    }

    #[test]
    fn band_onsets_count_strikes_in_the_band() {
        let mut x = vec![0.0f32; (0.3 * SR) as usize];
        // Strikes of a 1248 Hz resonance every 14 ms, plus a loud steady low tone that hides them broadband.
        for s in 0..15 {
            let a = (s as f32 * 0.014 * SR) as usize + 200;
            for j in 0..(0.012 * SR) as usize {
                if a + j < x.len() {
                    let t = j as f32 / SR;
                    x[a + j] += 0.2 * (-t / 0.002).exp() * (std::f32::consts::TAU * 1248.0 * t).sin();
                }
            }
        }
        for (i, v) in x.iter_mut().enumerate() {
            *v += 0.8 * (std::f32::consts::TAU * 150.0 * i as f32 / SR).sin();
        }
        let n = band_onsets(&x, SR, 1248.0).len();
        assert!((13..=16).contains(&n), "strikes {n}");
    }
}
