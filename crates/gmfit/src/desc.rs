//! Character descriptors (design D4n). They are reported and flagged but stay out of the score. Each catches a
//! defect the frame metrics average away, found by ranking the drum waves the user marked bad
//! (research/gm-procedural-pilot.md, "descriptor study"):
//! - fine spectral contrast: lines where the original is a noise wash (cymbal loops made of a harmonic comb);
//!   only meaningful when the original is a wash (≤ `WASH_DB`): on a pitched tone both are lines and leakage rules;
//! - percussive share: the original is made of tiny transients (jingles, bubbles), the copy is smooth;
//! - peaks per frame: modes smeared into many peaks (or the reverse).

use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

/// Octave bands (Hz) compared by `contrast`.
/// Weighted fine contrast up to which a sound counts as a noise wash (cymbal, brush, open hat loop ≈ 7–19 dB;
/// clean tones and the ride bell above).
pub const WASH_DB: f32 = 20.0;

const OCT: [f32; 7] = [86.0, 172.0, 344.0, 689.0, 1378.0, 2756.0, 5512.0];

fn power_frames(x: &[f32], n: usize, hop: usize, centred: bool) -> Vec<Vec<f32>> {
    let pad = if centred { n / 2 } else { 0 };
    let total = (x.len() + 2 * pad).max(n);
    let get = |i: usize| if i >= pad && i - pad < x.len() { x[i - pad] } else { 0.0 };
    let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
    let win: Vec<f32> = (0..n).map(|k| 0.5 - 0.5 * (std::f32::consts::TAU * k as f32 / n as f32).cos()).collect();
    let mut out = Vec::new();
    let mut s = 0;
    while s + n <= total {
        let mut buf: Vec<Complex32> = (0..n).map(|k| Complex32::new(get(s + k) * win[k], 0.0)).collect();
        fft.process(&mut buf);
        out.push(buf[..n / 2 + 1].iter().map(|c| c.norm_sqr()).collect());
        s += hop;
    }
    out
}

fn active(p: &[Vec<f32>], db: f32) -> Vec<bool> {
    let e: Vec<f32> = p.iter().map(|f| f.iter().sum()).collect();
    let top = e.iter().copied().fold(0.0f32, f32::max);
    e.iter().map(|&v| top > 0.0 && v > top * 10f32.powf(-db / 10.0)).collect()
}

/// Fine spectral contrast per octave band: 97th percentile over median of the 8192-point power spectrum
/// (averaged over frames), in dB, with each band's energy. Noise wash ≈ 10–13 dB, a comb of lines 20–50 dB.
pub fn contrast(x: &[f32], rate: f32) -> Vec<(f32, f32)> {
    let n = 8192;
    let p = power_frames(x, n, n / 4, false);
    let mut avg = vec![0.0f32; n / 2 + 1];
    for f in &p {
        for (a, v) in avg.iter_mut().zip(f) {
            *a += v / p.len() as f32;
        }
    }
    OCT.iter()
        .map(|&lo| {
            let mut q: Vec<f32> = (0..avg.len()).filter(|&k| {
                let hz = k as f32 * rate / n as f32;
                hz >= lo && hz < 2.0 * lo && hz < 0.5 * rate
            }).map(|k| avg[k] + 1e-20).collect();
            if q.len() < 8 {
                return (0.0, 0.0);
            }
            let e = q.iter().sum::<f32>();
            q.sort_by(f32::total_cmp);
            let pct = |r: f32| q[((q.len() - 1) as f32 * r) as usize];
            (10.0 * (pct(0.97) / pct(0.5)).log10(), e)
        })
        .collect()
}

/// Original fine contrast and generated minus original (dB), both weighted by the original's band energy
/// (square root). Difference > 0: the copy has lines where the original is denser.
pub fn contrast_pair(orig: &[f32], synth: &[f32], rate: f32) -> (f32, f32) {
    let (a, b) = (contrast(orig, rate), contrast(synth, rate));
    let w: Vec<f32> = a.iter().map(|v| v.1.sqrt()).collect();
    let tot = w.iter().sum::<f32>();
    if tot <= 0.0 {
        return (0.0, 0.0);
    }
    let avg = |f: &dyn Fn(usize) -> f32| (0..a.len()).map(|k| w[k] * f(k)).sum::<f32>() / tot;
    (avg(&|k| a[k].0), avg(&|k| b[k].0 - a[k].0))
}

/// Generated minus original fine contrast (`contrast_pair`).
pub fn contrast_diff(orig: &[f32], synth: &[f32], rate: f32) -> f32 {
    contrast_pair(orig, synth, rate).1
}

fn median_of(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// Percussive share (median-filter harmonic/percussive separation, 17 frames × 17 bins, Wiener mask):
/// fraction of the energy in short transients. Jingles and bubbles ≈ 0.6–0.7, a smooth tone ≈ 0.1.
pub fn percussive_share(x: &[f32]) -> f32 {
    let p = power_frames(x, 1024, 256, true);
    let (nt, nf) = (p.len(), p.first().map_or(0, |f| f.len()));
    if nt < 3 {
        return 0.0;
    }
    let (mut num, mut den) = (0.0f64, 0.0f64);
    let mut buf = Vec::with_capacity(17);
    for t in 0..nt {
        for k in 0..nf {
            buf.clear();
            buf.extend((t.saturating_sub(8)..(t + 9).min(nt)).map(|i| p[i][k]));
            let h = median_of(&mut buf);
            buf.clear();
            buf.extend((k.saturating_sub(8)..(k + 9).min(nf)).map(|j| p[t][j]));
            let pm = median_of(&mut buf);
            let mask = pm * pm / (h * h + pm * pm + 1e-30);
            num += (mask * p[t][k]) as f64;
            den += p[t][k] as f64;
        }
    }
    if den > 0.0 { (num / den) as f32 } else { 0.0 }
}

/// Mean number of spectral peaks per frame (1024-point, frames within 30 dB of the loudest), counting peaks
/// within 30 dB of the frame's maximum.
pub fn peaks_per_frame(x: &[f32]) -> f32 {
    let p = power_frames(x, 1024, 256, true);
    let act = active(&p, 30.0);
    let counts: Vec<usize> = p.iter().zip(&act).filter(|(_, a)| **a).map(|(f, _)| {
        let top = f.iter().copied().fold(0.0f32, f32::max) * 1e-3;
        f.windows(3).filter(|w| w[1] > w[0] && w[1] > w[2] && w[1] > top).count()
    }).collect();
    if counts.is_empty() { 0.0 } else { counts.iter().sum::<usize>() as f32 / counts.len() as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n).map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as f32 / u32::MAX as f32 * 2.0 - 1.0
        }).collect()
    }

    #[test]
    fn comb_has_more_contrast_than_noise() {
        let rate = 22050.0;
        let wash = noise(22050, 7);
        let comb: Vec<f32> = (0..22050).map(|i| (1..200).map(|h| (std::f32::consts::TAU * 47.3 * h as f32 * i as f32 / rate).sin()).sum::<f32>() * 0.01).collect();
        assert!(contrast_diff(&wash, &comb, rate) > 10.0, "{}", contrast_diff(&wash, &comb, rate));
        assert!(contrast_diff(&wash, &noise(22050, 9), rate).abs() < 3.0);
    }

    #[test]
    fn clicks_are_percussive_tone_is_not() {
        let tone: Vec<f32> = (0..22050).map(|i| (i as f32 * 0.07).sin() * 0.5).collect();
        let mut clicks = vec![0.0f32; 22050];
        for c in (0..22050).step_by(700) {
            for (k, v) in noise(30, c as u32 + 1).iter().enumerate() {
                clicks[c + k] = *v;
            }
        }
        let (pt, pc) = (percussive_share(&tone), percussive_share(&clicks));
        assert!(pt < 0.2 && pc > 0.5, "tone {pt} clicks {pc}");
    }

    #[test]
    fn peaks_count_partials() {
        let rate = 22050.0;
        let few: Vec<f32> = (0..11025).map(|i| (1..4).map(|h| (std::f32::consts::TAU * 440.0 * h as f32 * i as f32 / rate).sin()).sum::<f32>()).collect();
        let many: Vec<f32> = (0..11025).map(|i| (1..25).map(|h| (std::f32::consts::TAU * 440.0 * h as f32 * i as f32 / rate).sin()).sum::<f32>()).collect();
        let (a, b) = (peaks_per_frame(&few), peaks_per_frame(&many));
        assert!((2.5..4.5).contains(&a) && b > 15.0, "{a} {b}");
    }
}
