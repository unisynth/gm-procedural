//! Optional per-wave harmonic correction (design D4b): gains in dB at harmonics 1..C of the wave's f0, interpolated
//! linearly in harmonic number between entries (0 dB beyond the table, fading over one harmonic).
//! - Loop `[s, s+L)`: exact — DFT of the loop, per-bin gain, inverse DFT; still periodic, so the wrap stays clean.
//!   Samples after the loop end continue the corrected loop.
//! - Attack `[0, s)` (whole wave when unlooped): linear-phase FIR through the same curve, delay-compensated; its
//!   length spans ≥ 4 periods of f0 (129…2049 taps) so neighbouring harmonics get their own gains.
use crate::math::{cos_cycles, db};
use alloc::vec;
use alloc::vec::Vec;

const MIN_TAPS: usize = 129;
const MAX_TAPS: usize = 2049;

/// FIR length for a fundamental: odd, ≥ 4 periods.
pub fn taps(f0: f32, rate: f32) -> usize {
    let t = ((4.0 * rate / f0.max(1.0)) as usize).clamp(MIN_TAPS, MAX_TAPS);
    t | 1
}

/// Gain (linear) at harmonic number `h` (≥ 0).
pub fn gain_at(c: &[f32], h: f32) -> f32 {
    if c.is_empty() {
        return 1.0;
    }
    let x = h - 1.0;
    let d = if x <= 0.0 {
        c[0]
    } else {
        let i = x as usize;
        let w = x - i as f32;
        let a = c.get(i).copied().unwrap_or(0.0);
        let b = c.get(i + 1).copied().unwrap_or(0.0);
        a + (b - a) * w
    };
    db(d)
}

pub(crate) mod fft;

/// DFT of the real `x` (any length; `fft`).
pub(crate) fn dft(x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let (mut re, mut im) = (x.to_vec(), vec![0.0f32; x.len()]);
    fft::fft(&mut re, &mut im, false);
    (re, im)
}

/// Real part of the inverse DFT, scaled by 1/L.
pub(crate) fn idft(re: &[f32], im: &[f32]) -> Vec<f32> {
    let l = re.len();
    let (mut r, mut i) = (re.to_vec(), im.to_vec());
    fft::fft(&mut r, &mut i, true);
    r.iter().map(|v| v / l as f32).collect()
}

/// Applies the correction in place. `lp` = loop `(start, length)` inside the wave, `f0` in Hz.
pub fn apply(x: &mut [f32], lp: Option<(usize, usize)>, f0: f32, rate: f32, c: &[f32]) {
    let orig: Vec<f32> = x.to_vec();
    let attack_end = lp.map_or(x.len(), |(s, _)| s);
    // FIR by frequency sampling of the curve on N/2+1 points, Hann-windowed.
    let ntaps = taps(f0, rate);
    let m = ntaps / 2;
    let nfreq = ntaps.next_power_of_two();
    let curve: Vec<f32> = (0..=nfreq).map(|k| gain_at(c, (k as f32 / (2 * nfreq) as f32) * rate / f0)).collect();
    // h[t] = (1/N)·(g₀ + 2·Σ g_k·cos(2πkt/N) + g_{N/2}·cos(πt)), N = 2·nfreq: the inverse FFT of the even spectrum
    // G_k = G_{N−k} = curve[k]. Hann-windowed, centred (t = n − m).
    let nn = 2 * nfreq;
    let (mut gr, mut gi) = (vec![0.0f32; nn], vec![0.0f32; nn]);
    for k in 0..nn {
        gr[k] = curve[k.min(nn - k)];
    }
    fft::fft(&mut gr, &mut gi, true);
    let h: Vec<f32> = (0..ntaps)
        .map(|n| {
            let t = (n as isize - m as isize).rem_euclid(nn as isize) as usize;
            let win = 0.5 - 0.5 * cos_cycles(n as f32 / (ntaps - 1) as f32);
            gr[t] / nn as f32 * win
        })
        .collect();
    // Linear-phase (symmetric) FIR, delay-compensated: y[i] = Σ h[k]·x[i+k−m] = (h ∗ x)[i+m], by FFT convolution.
    if attack_end > 0 {
        let na = (attack_end + m).min(orig.len());
        let size = (na + ntaps).next_power_of_two();
        let (mut ar, mut ai) = (vec![0.0f32; size], vec![0.0f32; size]);
        ar[..na].copy_from_slice(&orig[..na]);
        let (mut hr, mut hi) = (vec![0.0f32; size], vec![0.0f32; size]);
        hr[..ntaps].copy_from_slice(&h);
        fft::fft(&mut ar, &mut ai, false);
        fft::fft(&mut hr, &mut hi, false);
        for k in 0..size {
            let (xr, xi) = (ar[k] * hr[k] - ai[k] * hi[k], ar[k] * hi[k] + ai[k] * hr[k]);
            ar[k] = xr;
            ai[k] = xi;
        }
        fft::fft(&mut ar, &mut ai, true);
        for (i, y) in x.iter_mut().enumerate().take(attack_end) {
            *y = ar[i + m] / size as f32;
        }
    }
    if let Some((s, l)) = lp {
        let (mut re, mut im) = dft(&orig[s..s + l]);
        for k in 0..l {
            let kk = k.min(l - k);
            let g = gain_at(c, kk as f32 * rate / l as f32 / f0);
            re[k] *= g;
            im[k] *= g;
        }
        let y = idft(&re, &im);
        for i in s..x.len() {
            x[i] = y[(i - s) % l];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::sin_cycles;

    fn harmonic_db(x: &[f32], h: usize, periods: usize) -> f32 {
        let (re, im) = dft(x);
        let k = h * periods;
        crate::math::to_db(crate::math::sqrt(re[k] * re[k] + im[k] * im[k]))
    }

    /// The FFT-designed, FFT-applied attack FIR equals the direct cosine-sum design and direct convolution (the
    /// pre-D9 code) within 1e-4 of the peak, on an unlooped harmonic signal.
    #[test]
    fn fir_by_fft_matches_direct() {
        let (rate, f0) = (22050.0f32, 147.0f32);
        let c = [2.0f32, -6.0, 3.0, -12.0, 0.0, 4.0];
        let sig: Vec<f32> = (0..3000).map(|i| (1..12).map(|k| sin_cycles(k as f32 * f0 * i as f32 / rate) / k as f32).sum()).collect();
        let mut y = sig.clone();
        apply(&mut y, None, f0, rate, &c);
        let ntaps = taps(f0, rate);
        let m = ntaps / 2;
        let nfreq = ntaps.next_power_of_two();
        let curve: Vec<f32> = (0..=nfreq).map(|k| gain_at(&c, (k as f32 / (2 * nfreq) as f32) * rate / f0)).collect();
        let h: Vec<f32> = (0..ntaps)
            .map(|n| {
                let t = n as f32 - m as f32;
                let mut acc = curve[0];
                for (k, g) in curve.iter().enumerate().skip(1) {
                    acc += if k == nfreq { 1.0 } else { 2.0 } * g * cos_cycles(k as f32 * t / (2 * nfreq) as f32);
                }
                acc / (2 * nfreq) as f32 * (0.5 - 0.5 * cos_cycles(n as f32 / (ntaps - 1) as f32))
            })
            .collect();
        let peak = sig.iter().map(|v| v.abs()).fold(0.0, f32::max);
        for i in (0..3000).step_by(7) {
            let want: f32 = h.iter().enumerate().map(|(k, hk)| {
                let j = i as isize + k as isize - m as isize;
                if j >= 0 && (j as usize) < sig.len() { hk * sig[j as usize] } else { 0.0 }
            }).sum();
            assert!((y[i] - want).abs() < 1e-4 * peak, "sample {i}: {} vs {want}", y[i]);
        }
    }

    /// Spec "Correction fixes loop timbre": harmonic 3 at -6 dB lifted by +6 dB, others unchanged.
    #[test]
    fn correction_fixes_loop_timbre() {
        let (rate, f0, periods) = (22050.0f32, 22050.0 / 100.0, 4usize); // period 100 samples, loop 400
        let l = 100 * periods;
        let s = 300;
        let amps = [1.0f32, 0.5, crate::math::db(-6.0) * 0.5, 0.25];
        let sig: Vec<f32> = (0..s + l)
            .map(|i| amps.iter().enumerate().map(|(k, a)| a * sin_cycles((k + 1) as f32 * i as f32 / 100.0)).sum())
            .collect();
        let mut y = sig.clone();
        apply(&mut y, Some((s, l)), f0, rate, &[0.0, 0.0, 6.0]);
        let loop_in = &sig[s..s + l];
        let loop_out = &y[s..s + l];
        let d3 = harmonic_db(loop_out, 3, periods) - harmonic_db(loop_in, 3, periods);
        assert!((d3 - 6.0).abs() < 0.1, "harmonic 3 moved {d3} dB");
        for h in [1, 2, 4] {
            let d = harmonic_db(loop_out, h, periods) - harmonic_db(loop_in, h, periods);
            assert!(d.abs() < 0.1, "harmonic {h} moved {d} dB");
        }
        // Still periodic: wrap step equals an ordinary step.
        let wrap = (y[s] - y[s + l - 1]).abs();
        let max_step = loop_out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(wrap <= max_step * 1.001);
    }

    #[test]
    fn empty_table_is_identity_gain() {
        assert_eq!(gain_at(&[], 3.0), 1.0);
        assert!((gain_at(&[0.0, 6.0], 1.5) - db(3.0)).abs() < 1e-6);
        assert_eq!(gain_at(&[0.0, 6.0], 4.0), 1.0);
    }
}
