//! Optional per-wave harmonic correction (design D4b): gains in dB at harmonics 1..C of the wave's f0, interpolated
//! linearly in harmonic number between entries (0 dB beyond the table, fading over one harmonic).
//! - Loop `[s, s+L)`: exact — DFT of the loop, per-bin gain, inverse DFT; still periodic, so the wrap stays clean.
//!   Samples after the loop end continue the corrected loop.
//! - Attack `[0, s)` (whole wave when unlooped): linear-phase FIR through the same curve, delay-compensated; its
//!   length spans ≥ 4 periods of f0 (129…2049 taps) so neighbouring harmonics get their own gains.
use crate::math::{cos_cycles, db, sin_cycles};
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

/// Real DFT of `x` (O(L²), twiddles from one table).
pub(crate) fn dft(x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let l = x.len();
    let tw: Vec<(f32, f32)> = (0..l).map(|k| (cos_cycles(k as f32 / l as f32), sin_cycles(k as f32 / l as f32))).collect();
    let (mut re, mut im) = (vec![0.0f32; l], vec![0.0f32; l]);
    for k in 0..=l / 2 {
        let (mut sr, mut si, mut idx) = (0.0f32, 0.0f32, 0usize);
        for &v in x {
            let (c, s) = tw[idx];
            sr += v * c;
            si -= v * s;
            idx += k;
            if idx >= l {
                idx -= l;
            }
        }
        re[k] = sr;
        im[k] = si;
        if k > 0 && k < l - k {
            re[l - k] = sr;
            im[l - k] = -si;
        }
    }
    (re, im)
}

pub(crate) fn idft(re: &[f32], im: &[f32]) -> Vec<f32> {
    let l = re.len();
    let tw: Vec<(f32, f32)> = (0..l).map(|k| (cos_cycles(k as f32 / l as f32), sin_cycles(k as f32 / l as f32))).collect();
    (0..l)
        .map(|n| {
            let (mut acc, mut idx) = (0.0f32, 0usize);
            for k in 0..l {
                let (c, s) = tw[idx];
                acc += re[k] * c - im[k] * s;
                idx += n;
                if idx >= l {
                    idx -= l;
                }
            }
            acc / l as f32
        })
        .collect()
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
    let h: Vec<f32> = (0..ntaps)
        .map(|n| {
            let t = n as f32 - m as f32;
            let mut acc = curve[0];
            for (k, g) in curve.iter().enumerate().skip(1) {
                let w = if k == nfreq { 1.0 } else { 2.0 };
                acc += w * g * cos_cycles(k as f32 * t / (2 * nfreq) as f32);
            }
            let win = 0.5 - 0.5 * cos_cycles(n as f32 / (ntaps - 1) as f32);
            acc / (2 * nfreq) as f32 * win
        })
        .collect();
    for (i, y) in x.iter_mut().enumerate().take(attack_end) {
        let mut acc = 0.0;
        for (k, hk) in h.iter().enumerate() {
            let j = i as isize + k as isize - m as isize;
            if j >= 0 && (j as usize) < orig.len() {
                acc += hk * orig[j as usize];
            }
        }
        *y = acc;
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

    fn harmonic_db(x: &[f32], h: usize, periods: usize) -> f32 {
        let (re, im) = dft(x);
        let k = h * periods;
        crate::math::to_db(crate::math::sqrt(re[k] * re[k] + im[k] * im[k]))
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
