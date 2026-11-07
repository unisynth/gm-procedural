//! Host-only target conditioning (change add-gmplus-bank, D2): Kaiser-windowed sinc resampler, DC high-pass and the
//! SoundFont static low-pass (`initialFilterFc` in absolute cents, `initialFilterQ` in centibels).

use std::f64::consts::PI;

/// Taps of the resampling kernel (half width 32 input samples at unity ratio).
pub const TAPS: usize = 64;
const BETA: f64 = 8.6;

fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
    while term > 1e-12 * sum {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        k += 1.0;
    }
    sum
}

/// Reads `x` at positions `start + n·step` for `n < len` (positions in input samples). `step > 1` lowers the
/// kernel cutoff to `0.95/step` so nothing aliases. Outside the input reads zero.
pub fn resample(x: &[f32], start: f64, step: f64, len: usize) -> Vec<f32> {
    let cut = if step > 1.0 { 0.95 / step } else { 1.0 };
    let half = (TAPS / 2) as f64 / cut;
    let i0b = bessel_i0(BETA);
    let kernel = |d: f64| -> f64 {
        let r = d / half;
        if r.abs() >= 1.0 {
            return 0.0;
        }
        let w = bessel_i0(BETA * (1.0 - r * r).sqrt()) / i0b;
        let s = if d == 0.0 { 1.0 } else { (PI * d * cut).sin() / (PI * d * cut) };
        cut * s * w
    };
    (0..len)
        .map(|n| {
            let p = start + n as f64 * step;
            let (lo, hi) = ((p - half).ceil() as i64, (p + half).floor() as i64);
            let mut acc = 0.0;
            for k in lo.max(0)..=hi.min(x.len() as i64 - 1) {
                acc += x[k as usize] as f64 * kernel(p - k as f64);
            }
            acc as f32
        })
        .collect()
}

/// Reads `x` at arbitrary increasing positions. The kernel cutoff follows the local step (`pos[n+1] − pos[n]`, the
/// last step repeated), as `resample` does for a constant one.
pub fn resample_at(x: &[f32], pos: &[f64]) -> Vec<f32> {
    let i0b = bessel_i0(BETA);
    (0..pos.len())
        .map(|n| {
            let step = if n + 1 < pos.len() { pos[n + 1] - pos[n] } else if n > 0 { pos[n] - pos[n - 1] } else { 1.0 };
            let cut = if step > 1.0 { 0.95 / step } else { 1.0 };
            let half = (TAPS / 2) as f64 / cut;
            let p = pos[n];
            let (lo, hi) = ((p - half).ceil() as i64, (p + half).floor() as i64);
            let mut acc = 0.0;
            for k in lo.max(0)..=hi.min(x.len() as i64 - 1) {
                let d = p - k as f64;
                let r = d / half;
                if r.abs() >= 1.0 {
                    continue;
                }
                let w = bessel_i0(BETA * (1.0 - r * r).sqrt()) / i0b;
                let s = if d == 0.0 { 1.0 } else { (PI * d * cut).sin() / (PI * d * cut) };
                acc += x[k as usize] as f64 * cut * s * w;
            }
            acc as f32
        })
        .collect()
}

/// Runs a filter whose coefficients change: `coef(n)` is asked every `block` samples (state carried over).
pub fn run_varying(x: &[f32], block: usize, coef: impl Fn(usize) -> Biquad) -> Vec<f32> {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut f = coef(0);
    x.iter()
        .enumerate()
        .map(|(n, &v)| {
            if n % block == 0 && n > 0 {
                f = coef(n);
            }
            let v = v as f64;
            let y = f.b[0] * v + f.b[1] * x1 + f.b[2] * x2 - f.a[0] * y1 - f.a[1] * y2;
            (x2, x1, y2, y1) = (x1, v, y1, y);
            y as f32
        })
        .collect()
}

/// Two-pole biquad (RBJ), direct form I, f64 state.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

impl Biquad {
    pub fn lowpass(fc: f64, q: f64, rate: f64) -> Biquad {
        let w = 2.0 * PI * fc / rate;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        let a0 = 1.0 + al;
        Biquad { b: [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0], a: [-2.0 * c / a0, (1.0 - al) / a0] }
    }
    pub fn highpass(fc: f64, q: f64, rate: f64) -> Biquad {
        let w = 2.0 * PI * fc / rate;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        let a0 = 1.0 + al;
        Biquad { b: [(1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0], a: [-2.0 * c / a0, (1.0 - al) / a0] }
    }
    pub fn scaled(mut self, g: f64) -> Biquad {
        self.b.iter_mut().for_each(|b| *b *= g);
        self
    }
    pub fn run(&self, x: &[f32]) -> Vec<f32> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        x.iter()
            .map(|&v| {
                let v = v as f64;
                let y = self.b[0] * v + self.b[1] * x1 + self.b[2] * x2 - self.a[0] * y1 - self.a[1] * y2;
                (x2, x1, y2, y1) = (x1, v, y1, y);
                y as f32
            })
            .collect()
    }
    /// Magnitude at `f` Hz.
    pub fn gain(&self, f: f64, rate: f64) -> f64 {
        let w = 2.0 * PI * f / rate;
        let z1 = (w.cos(), -w.sin());
        let z2 = ((2.0 * w).cos(), -(2.0 * w).sin());
        let num = (self.b[0] + self.b[1] * z1.0 + self.b[2] * z2.0, self.b[1] * z1.1 + self.b[2] * z2.1);
        let den = (1.0 + self.a[0] * z1.0 + self.a[1] * z2.0, self.a[0] * z1.1 + self.a[1] * z2.1);
        (num.0.hypot(num.1)) / (den.0.hypot(den.1))
    }
}

/// Removes DC: 2nd-order Butterworth high-pass at 10 Hz.
pub fn dc_block(x: &[f32], rate: f64) -> Vec<f32> {
    Biquad::highpass(10.0, std::f64::consts::FRAC_1_SQRT_2, rate).run(x)
}

/// SoundFont cutoff in absolute cents → Hz.
pub fn cents_hz(c: f64) -> f64 {
    8.176 * (c / 1200.0).exp2()
}

/// The SoundFont static low-pass as FluidSynth implements it: resonance peak `q_cb/10 − 3.01` dB, DC gain
/// `1/√q`. `None` when the cutoff sits at or above 13 500 cents or 0.45 × rate (the filter is open).
pub fn sf_lowpass(fc_cents: f64, q_cb: f64, rate: f64) -> Option<Biquad> {
    if fc_cents >= 13500.0 || cents_hz(fc_cents) >= 0.45 * rate {
        return None;
    }
    Some(sf_lowpass_clamped(fc_cents, q_cb, rate))
}

/// The same filter with the cutoff clamped to 1500 cents .. 0.45 × rate (for a sweep that passes the open point).
pub fn sf_lowpass_clamped(fc_cents: f64, q_cb: f64, rate: f64) -> Biquad {
    let fc = cents_hz(fc_cents.clamp(1500.0, 13500.0)).min(0.45 * rate);
    let q = 10f64.powf((q_cb.max(0.0) / 10.0 - 3.01) / 20.0);
    Biquad::lowpass(fc, q, rate).scaled(1.0 / q.sqrt().max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(f: f64, rate: f64, n: usize) -> Vec<f32> {
        (0..n).map(|k| (2.0 * PI * f * k as f64 / rate).sin() as f32).collect()
    }

    /// Frequency by linearly interpolated rising zero crossings over the middle of the signal.
    fn freq(x: &[f32], rate: f64) -> f64 {
        let (a, b) = (x.len() / 8, x.len() * 7 / 8);
        let mut cross = Vec::new();
        for k in a..b {
            let (p, q) = (x[k] as f64, x[k + 1] as f64);
            if p < 0.0 && q >= 0.0 {
                cross.push(k as f64 + p / (p - q));
            }
        }
        (cross.len() - 1) as f64 / (cross[cross.len() - 1] - cross[0]) * rate
    }

    fn rms(x: &[f32]) -> f64 {
        let m = &x[x.len() / 8..x.len() * 7 / 8];
        (m.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / m.len() as f64).sqrt()
    }

    #[test]
    fn resample_keeps_frequency_and_level() {
        let rate = 32000.0;
        for (f, step) in [(1000.0, 1.0123), (440.0, 0.6890625), (3000.0, 1.378125)] {
            let x = sine(f, rate, 64000);
            let n = ((x.len() as f64 - 1.0) / step) as usize;
            let y = resample(&x, 0.0, step, n);
            let want = f * step;
            let cents = 1200.0 * (freq(&y, rate) / want).log2();
            assert!(cents.abs() < 0.1, "f {f} step {step}: {cents} cents");
            let db = 20.0 * (rms(&y) / rms(&x)).log10();
            assert!(db.abs() < 0.05, "f {f} step {step}: {db} dB");
        }
    }

    #[test]
    fn resample_at_matches_constant_step() {
        let x = sine(700.0, 32000.0, 8000);
        let pos: Vec<f64> = (0..5000).map(|n| 3.0 + n as f64 * 1.21).collect();
        let (a, b) = (resample(&x, 3.0, 1.21, 5000), resample_at(&x, &pos));
        assert!(a.iter().zip(&b).all(|(p, q)| (p - q).abs() < 1e-6));
    }

    #[test]
    fn varying_filter_equals_fixed_when_constant() {
        let x = sine(3000.0, 32000.0, 4000);
        let f = Biquad::lowpass(1000.0, 0.7, 32000.0);
        assert_eq!(run_varying(&x, 16, |_| f), f.run(&x));
    }

    #[test]
    fn resample_identity() {
        let x = sine(1234.0, 32000.0, 4000);
        let y = resample(&x, 0.0, 1.0, x.len());
        for k in 100..3900 {
            assert!((x[k] - y[k]).abs() < 1e-5);
        }
    }

    #[test]
    fn downsampling_rejects_aliases() {
        // 19 kHz at 44.1 kHz, read at step 44.1/32, is above the 16 kHz Nyquist of the 32 kHz result.
        let x = sine(19000.0, 44100.0, 44100);
        let step = 44100.0 / 32000.0;
        let y = resample(&x, 0.0, step, 30000);
        assert!(rms(&y) < 0.01, "alias rms {}", rms(&y));
    }

    #[test]
    fn dc_removed() {
        let x: Vec<f32> = sine(200.0, 32000.0, 64000).iter().map(|v| v * 0.5 + 0.25).collect();
        let y = dc_block(&x, 32000.0);
        let tail = &y[48000..];
        let mean = tail.iter().map(|&v| v as f64).sum::<f64>() / tail.len() as f64;
        assert!(mean.abs() < 1e-3, "dc {mean}");
        assert!((rms(&y[32000..]) - 0.5 / 2f64.sqrt()).abs() < 0.01);
    }

    #[test]
    fn sf_filter_cutoff() {
        let rate = 32000.0;
        for cents in [6000.0, 8000.0, 10000.0] {
            let f = sf_lowpass(cents, 0.0, rate).unwrap();
            let fc = cents_hz(cents);
            // −3 dB point by bisection on the magnitude response.
            let (mut lo, mut hi) = (fc * 0.5, fc * 1.5);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if 20.0 * f.gain(mid, rate).log10() > -3.0103 { lo = mid } else { hi = mid }
            }
            assert!((lo / fc - 1.0).abs() < 0.02, "{cents} cents: −3 dB at {lo} Hz, want {fc}");
            assert!((f.gain(1.0, rate) - 1.0).abs() < 1e-6, "unity DC gain at Q 0");
        }
        assert!(sf_lowpass(13500.0, 0.0, rate).is_none());
        assert!(sf_lowpass(13000.0, 0.0, rate).is_none(), "above 0.45 × rate");
        let r = sf_lowpass(8000.0, 120.0, rate).unwrap();
        let peak = (1..4000).map(|k| r.gain(k as f64 * 2.0, rate)).fold(0.0, f64::max);
        assert!(peak > r.gain(1.0, rate) * 1.5, "Q 12 dB resonates");
    }
}
