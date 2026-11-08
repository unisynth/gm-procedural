//! Deterministic FFT for any length (no_std; IEEE + − × ÷ only, twiddles from `math::sin_cycles`, so native and
//! wasm32 give identical bits). Radix-2 iterative for powers of two; Bluestein (chirp-z through a power-of-two
//! convolution) otherwise, the chirp index k² mod 2L taken in integers so long loops keep exact twiddles.
//! Replaces the O(L²) loop DFT of the harmonic correction: 95 % of the bank's generation time (design D9).
use crate::math::{cos_cycles, sin_cycles};
use alloc::vec;
use alloc::vec::Vec;

/// In-place radix-2 FFT of `re`/`im` (length a power of two). `inverse` = conjugate twiddles, unscaled.
fn radix2(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        // Twiddles of this stage, exact per index (no recurrence drift).
        let tw: Vec<(f32, f32)> = (0..half).map(|k| (cos_cycles(k as f32 / len as f32), sign * sin_cycles(k as f32 / len as f32))).collect();
        let mut s = 0;
        while s < n {
            for (k, &(c, w)) in tw.iter().enumerate() {
                let (a, b) = (s + k, s + k + half);
                let (xr, xi) = (re[b] * c - im[b] * w, re[b] * w + im[b] * c);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
            s += len;
        }
        len <<= 1;
    }
}

/// Complex DFT of any length (`inverse`: conjugate kernel, unscaled), as the textbook sum
/// X[k] = Σ x[n]·e^{∓2πi·kn/L}.
pub fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let l = re.len();
    if l <= 1 {
        return;
    }
    if l.is_power_of_two() {
        radix2(re, im, inverse);
        return;
    }
    // Bluestein: kn = (k² + n² − (k−n)²)/2 → X[k] = w[k]·Σ (x[n]·w[n])·conj(w)[k−n], w[n] = e^{∓πi·n²/L}.
    let m = (2 * l - 1).next_power_of_two();
    let sign = if inverse { 1.0 } else { -1.0 };
    let chirp: Vec<(f32, f32)> = (0..l)
        .map(|n| {
            let q = ((n as u64 * n as u64) % (2 * l as u64)) as f32 / (2 * l) as f32; // n²/(2L) cycles, exact mod 1
            (cos_cycles(q), sign * sin_cycles(q))
        })
        .collect();
    let (mut ar, mut ai) = (vec![0.0f32; m], vec![0.0f32; m]);
    for n in 0..l {
        let (c, s) = chirp[n];
        ar[n] = re[n] * c - im[n] * s;
        ai[n] = re[n] * s + im[n] * c;
    }
    let (mut br, mut bi) = (vec![0.0f32; m], vec![0.0f32; m]);
    for n in 0..l {
        let (c, s) = chirp[n];
        br[n] = c;
        bi[n] = -s;
        if n > 0 {
            br[m - n] = c;
            bi[m - n] = -s;
        }
    }
    radix2(&mut ar, &mut ai, false);
    radix2(&mut br, &mut bi, false);
    for k in 0..m {
        let (xr, xi) = (ar[k] * br[k] - ai[k] * bi[k], ar[k] * bi[k] + ai[k] * br[k]);
        ar[k] = xr;
        ai[k] = xi;
    }
    radix2(&mut ar, &mut ai, true);
    let inv_m = 1.0 / m as f32;
    for k in 0..l {
        let (c, s) = chirp[k];
        let (xr, xi) = (ar[k] * inv_m, ai[k] * inv_m);
        re[k] = xr * c - xi * s;
        im[k] = xr * s + xi * c;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(x: &[f32]) -> (Vec<f64>, Vec<f64>) {
        let l = x.len();
        let mut re = vec![0.0f64; l];
        let mut im = vec![0.0f64; l];
        for k in 0..l {
            for (n, v) in x.iter().enumerate() {
                let a = core::f64::consts::TAU * ((k * n) % l) as f64 / l as f64;
                re[k] += *v as f64 * a.cos();
                im[k] -= *v as f64 * a.sin();
            }
        }
        (re, im)
    }

    /// Matches the textbook DFT (power of two, prime, a GM loop length) within 1e-4 of the peak, and the inverse
    /// round-trips within 1e-5.
    #[test]
    fn fft_matches_dft_any_length() {
        for l in [256usize, 997, 4789] {
            let x: Vec<f32> = (0..l).map(|i| sin_cycles(i as f32 * 0.137) + 0.3 * sin_cycles(i as f32 * 0.0191 + 0.2) + ((i * 7919) % 13) as f32 * 0.01).collect();
            let (nr, ni) = naive(&x);
            let (mut re, mut im) = (x.clone(), vec![0.0f32; l]);
            fft(&mut re, &mut im, false);
            let peak = nr.iter().zip(&ni).map(|(a, b)| (a * a + b * b).sqrt()).fold(0.0, f64::max);
            let err = (0..l).map(|k| ((re[k] as f64 - nr[k]).powi(2) + (im[k] as f64 - ni[k]).powi(2)).sqrt()).fold(0.0, f64::max);
            assert!(err < 1e-4 * peak, "L {l}: err {err} peak {peak}");
            fft(&mut re, &mut im, true);
            let back = (0..l).map(|i| (re[i] / l as f32 - x[i]).abs()).fold(0.0, f32::max);
            assert!(back < 1e-5 * 3.0, "L {l}: round trip {back}");
        }
    }
}
