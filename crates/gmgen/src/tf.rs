//! Time × band correction table `tf` (design D4c): dB gains on a fixed grid of `BANDS` log-spaced band centres ×
//! up to `TIMES.len()` time rows. The gain at (t, f) interpolates linearly in time between rows (held after the last
//! row) and linearly in log2 f between band centres (held outside).
//! - Attack `[0, s)` (whole wave when unlooped): STFT (Hann² OLA, N = 512, hop N/4), per-frame gain at the frame
//!   centre. Deterministic radix-2 FFT on `math` twiddles.
//! - Looped waves: the loop `[s, s+L)` is untouched (the harmonic correction owns it); the attack's gains fade to
//!   0 dB over the last 10 ms before `s`, so the attack meets the loop continuously.
//!
//! `measure` gives the band energies on the same grid; `gmfit correct` sets the table from original/model ratios.
use crate::math::{cos_cycles, db, log2, sin_cycles, to_db};
use alloc::vec;
use alloc::vec::Vec;

pub use crate::schema::{ENV_TIMES, TF_BANDS as BANDS, TF_RATIO as RATIO, TF_TIMES as TIMES};
const N: usize = 512;
const HOP: usize = N / 4;

/// Band centre `b` in Hz: 55 · RATIO^b (55 Hz … ~10 kHz).
pub fn centre(b: usize) -> f32 {
    let mut c = 55.0;
    for _ in 0..b {
        c *= RATIO;
    }
    c
}

/// Fractional band position of frequency `hz` (clamped to 0..BANDS-1).
fn band_pos(hz: f32) -> f32 {
    if hz <= 55.0 {
        return 0.0;
    }
    (log2(hz / 55.0) / log2(RATIO)).min((BANDS - 1) as f32)
}

/// Fractional row position of time `t` s (clamped to 0..rows-1).
fn row_pos(t: f32, rows: usize) -> f32 {
    let last = rows - 1;
    if t >= TIMES[last] {
        return last as f32;
    }
    let mut j = 0;
    while j + 1 < rows && TIMES[j + 1] <= t {
        j += 1;
    }
    j as f32 + (t - TIMES[j]) / (TIMES[j + 1] - TIMES[j])
}

/// Rows a table needs for an attack of `secs`: every row time below it plus the next (≥ 1).
pub fn rows_for(secs: f32) -> usize {
    (TIMES.iter().filter(|t| **t < secs).count() + 1).min(TIMES.len())
}

/// Per-bin linear gain at time `t` for an `nbins`-bin half spectrum (bin k ↔ k·rate/size).
fn gains(tf: &[f32], t: f32, size: usize, rate: f32) -> Vec<f32> {
    let rows = tf.len() / BANDS;
    let r = row_pos(t, rows);
    let (j, w) = (r as usize, r - r as usize as f32);
    let row: Vec<f32> = (0..BANDS)
        .map(|b| {
            let a = tf[j * BANDS + b];
            let c = if j + 1 < rows { tf[(j + 1) * BANDS + b] } else { a };
            a + (c - a) * w
        })
        .collect();
    (0..=size / 2)
        .map(|k| {
            let p = band_pos(k as f32 * rate / size as f32);
            let (i, w) = (p as usize, p - p as usize as f32);
            let a = row[i];
            let c = row[(i + 1).min(BANDS - 1)];
            db(a + (c - a) * w)
        })
        .collect()
}

/// In-place radix-2 complex FFT (`inverse` unscaled). `re.len()` is a power of two.
fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
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
        for k in 0..half {
            let (c, s) = (cos_cycles(k as f32 / len as f32), sign * sin_cycles(k as f32 / len as f32));
            let mut i = k;
            while i < n {
                let (ar, ai) = (re[i + half], im[i + half]);
                let (tr, ti) = (ar * c - ai * s, ar * s + ai * c);
                re[i + half] = re[i] - tr;
                im[i + half] = im[i] - ti;
                re[i] += tr;
                im[i] += ti;
                i += len;
            }
        }
        len <<= 1;
    }
}

fn hann(k: usize) -> f32 {
    0.5 - 0.5 * cos_cycles(k as f32 / N as f32)
}

/// STFT frames over `x` padded by N on both sides: `(frame start in x (may be negative), re, im)`.
fn frames(x: &[f32]) -> Vec<(isize, Vec<f32>, Vec<f32>)> {
    let mut out = Vec::new();
    let mut p = -(N as isize);
    while p < x.len() as isize {
        let mut re: Vec<f32> = (0..N)
            .map(|k| {
                let i = p + k as isize;
                if i >= 0 && (i as usize) < x.len() { x[i as usize] * hann(k) } else { 0.0 }
            })
            .collect();
        let mut im = vec![0.0f32; N];
        fft(&mut re, &mut im, false);
        out.push((p, re, im));
        p += HOP as isize;
    }
    out
}

/// Applies `tf` in place. `lp` = loop `(start, length)`.
pub fn apply(x: &mut [f32], lp: Option<(usize, usize)>, rate: f32, tf: &[f32]) {
    if tf.len() < BANDS {
        return;
    }
    let attack_end = lp.map_or(x.len(), |(s, _)| s);
    let orig: Vec<f32> = x.to_vec();
    if attack_end > 0 {
        // Reach past the loop start so frames there see the signal continuing.
        let src_ext = &orig[..(attack_end + N).min(orig.len())];
        let mut acc = vec![0.0f32; src_ext.len()];
        for (p, mut re, mut im) in frames(src_ext) {
            let c = (p + N as isize / 2).max(0) as usize;
            let mut g = gains(tf, c as f32 / rate, N, rate);
            if lp.is_some() {
                // Fade to unity over the last 10 ms before the loop start (fully unity from it on).
                let f = (0.01 * rate) as usize;
                let w = (attack_end.saturating_sub(c) as f32 / f.max(1) as f32).min(1.0);
                g.iter_mut().for_each(|v| *v = 1.0 + (*v - 1.0) * w);
            }
            for (k, (r, i)) in re.iter_mut().zip(im.iter_mut()).enumerate() {
                let gk = g[k.min(N - k)];
                *r *= gk;
                *i *= gk;
            }
            fft(&mut re, &mut im, true);
            for (k, v) in re.iter().enumerate() {
                let i = p + k as isize;
                if i >= 0 && (i as usize) < acc.len() {
                    // Hann analysis + Hann synthesis at hop N/4 sums to 1.5·N (unscaled inverse).
                    acc[i as usize] += v * hann(k) / (1.5 * N as f32);
                }
            }
        }
        x[..attack_end].copy_from_slice(&acc[..attack_end]);
    }
}

/// Gain (dB) of the `env` table at time `t` s: linear between points, held after the last.
pub fn env_db(env: &[f32], t: f32) -> f32 {
    let n = env.len().min(ENV_TIMES.len());
    if n == 0 {
        return 0.0;
    }
    let mut j = 0;
    while j + 1 < n && ENV_TIMES[j + 1] <= t {
        j += 1;
    }
    if j + 1 >= n {
        return env[n - 1];
    }
    let w = (t - ENV_TIMES[j]) / (ENV_TIMES[j + 1] - ENV_TIMES[j]);
    env[j] + (env[j + 1] - env[j]) * w
}

/// Applies the fine envelope `env` (design D4d) in place: per-sample gain on the attack (whole unlooped wave). On a
/// looped wave the gain fades to unity over the last 10 ms before the loop start; the loop is untouched.
pub fn apply_env(x: &mut [f32], lp: Option<(usize, usize)>, rate: f32, env: &[f32]) {
    let end = lp.map_or(x.len(), |(s, _)| s);
    let f = ((0.01 * rate) as usize).max(1);
    for (i, v) in x.iter_mut().enumerate().take(end) {
        let mut g = env_db(env, i as f32 / rate);
        if lp.is_some() {
            g *= ((end - i) as f32 / f as f32).min(1.0);
        }
        *v *= db(g);
    }
}

/// RMS level (dB) of `x` around each `env` point: window from the midpoint to the previous point to the midpoint
/// to the next (≥ 1 ms, the last one reaching 50 ms further), clipped to `x`.
pub fn measure_env(x: &[f32], rate: f32, n: usize) -> Vec<f32> {
    let t = &ENV_TIMES[..n];
    (0..n)
        .map(|j| {
            let lo = if j == 0 { 0.0 } else { (t[j - 1] + t[j]) / 2.0 };
            let hi = if j + 1 < n { (t[j] + t[j + 1]) / 2.0 } else { t[j] + 0.05 };
            let (lo, hi) = (lo.min(t[j] - 0.0005).max(0.0), hi.max(t[j] + 0.0005));
            let (a, b) = ((lo * rate) as usize, ((hi * rate) as usize).min(x.len()));
            if a >= b {
                return -200.0;
            }
            let e: f32 = x[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32;
            to_db(crate::math::sqrt(e.max(1e-20)))
        })
        .collect()
}

/// Band energies in dB on the `tf` grid (`rows` × `BANDS`), measured over `x` with the same STFT: each frame's
/// power is shared between neighbouring rows and bands by the interpolation weights.
pub fn measure(x: &[f32], rate: f32, rows: usize) -> Vec<f32> {
    let mut e = vec![1e-12f32; rows * BANDS];
    for (p, re, im) in frames(x) {
        let t = (p + N as isize / 2).max(0) as f32 / rate;
        let r = row_pos(t, rows);
        let (j, wt) = (r as usize, r - r as usize as f32);
        for k in 1..=N / 2 {
            let pw = re[k] * re[k] + im[k] * im[k];
            let bp = band_pos(k as f32 * rate / N as f32);
            let (b, wb) = (bp as usize, bp - bp as usize as f32);
            for (jj, a) in [(j, 1.0 - wt), ((j + 1).min(rows - 1), wt)] {
                for (bb, c) in [(b, 1.0 - wb), ((b + 1).min(BANDS - 1), wb)] {
                    e[jj * BANDS + bb] += pw * a * c;
                }
            }
        }
    }
    e.iter().map(|v| to_db(crate::math::sqrt(*v))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat −6 dB `env` lowers every measured point by 6 dB; the loop is untouched.
    #[test]
    fn env_gain_lands() {
        let rate = 22050.0;
        let x: Vec<f32> = (0..9000).map(|i| sin_cycles(i as f32 * 1000.0 / rate) * 0.5).collect();
        let mut env = [-6.0f32; 20];
        env[0] = 0.0;
        let mut y = x.clone();
        apply_env(&mut y, Some((8000, 1000)), rate, &env);
        let (a, b) = (measure_env(&x, rate, 20), measure_env(&y, rate, 20));
        for j in 5..20 {
            assert!((b[j] - a[j] + 6.0).abs() < 0.1, "point {j} moved {}", b[j] - a[j]);
        }
        assert_eq!(y[8000..], x[8000..]);
    }

    #[test]
    fn fft_round_trips() {
        let mut re: Vec<f32> = (0..64).map(|i| sin_cycles(i as f32 * 3.0 / 64.0) + 0.25).collect();
        let x = re.clone();
        let mut im = vec![0.0; 64];
        fft(&mut re, &mut im, false);
        assert!((re[0] - 16.0).abs() < 1e-3 && (im[3] + 32.0).abs() < 1e-3);
        fft(&mut re, &mut im, true);
        for (a, b) in re.iter().zip(&x) {
            assert!((a / 64.0 - b).abs() < 1e-5);
        }
    }

    /// Zero table is (nearly) identity; +6 dB on bands 8–12 lifts band 10 by ~6 dB and leaves band 1; loop untouched.
    #[test]
    fn flat_table_is_identity_and_gain_lands() {
        let rate = 22050.0;
        let mut r = 1u32;
        let x: Vec<f32> = core::iter::repeat(())
            .take(5000)
            .map(|_| {
                r ^= r << 13;
                r ^= r >> 17;
                r ^= r << 5;
                (r as f32 / u32::MAX as f32 - 0.5) * 0.5
            })
            .collect();
        let mut y = x.clone();
        apply(&mut y, Some((3000, 1000)), rate, &[0.0; BANDS]);
        let err = x.iter().zip(&y).take(3000).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-4, "identity error {err}");
        let mut tf = [0.0f32; BANDS];
        tf[8..=12].fill(6.0);
        let mut z = x.clone();
        apply(&mut z, Some((3000, 1000)), rate, &tf);
        let (a, b) = (measure(&x[..3000], rate, 1), measure(&z[..3000], rate, 1));
        let d: Vec<f32> = a.iter().zip(&b).map(|(x, y)| y - x).collect();
        assert!((d[10] - 6.0).abs() < 1.0, "moved {d:?}");
        assert!((b[1] - a[1]).abs() < 0.5, "band 1 moved {}", b[1] - a[1]);
        assert_eq!(z[3000..], x[3000..], "loop must stay untouched");
    }
}
