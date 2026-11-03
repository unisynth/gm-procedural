//! Similarity score (design D6) of a generated wave against the original, on the "played" signals.
//!
//! - `d_spec`: mean |dB| difference of log-mel spectrograms at three STFT sizes (≥ 2 periods/512/2048, hop size/4, Hann),
//!   floored at −80 dB; frames in the first 50 ms count double (transients).
//! - `d_env`: mean |dB| difference of RMS envelopes (window ≥ 2 periods and ≥ 10 ms, every 2 ms; floor −80 dB).
//! - `d_loop`: mean |dB| difference of the loop's exact DFT magnitudes (bins up to Nyquist; 0 when unlooped).
//! - `score = exp(−(d_spec/W_SPEC + d_env/W_ENV + d_loop/W_LOOP))`, 1 for identical waves.
use crate::audio::{loop_of, played};
use gmgen::render::WaveMeta;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use serde_json::{json, Value};

pub const W_SPEC: f32 = 12.0;
pub const W_ENV: f32 = 6.0;
pub const W_LOOP: f32 = 12.0;
const FLOOR: f32 = -80.0;
/// Seconds of the played unroll that are compared.
pub const PLAYED_SECS: f32 = 0.5;

#[derive(Clone, Debug)]
pub struct Score {
    pub score: f32,
    pub d_spec: f32,
    pub d_env: f32,
    pub d_loop: f32,
    pub flags: Vec<&'static str>,
    pub f0_orig: Option<f32>,
    pub f0_gen: Option<f32>,
}

impl Score {
    pub fn json(&self) -> Value {
        json!({"score": r4(self.score), "d_spec": r4(self.d_spec), "d_env": r4(self.d_env), "d_loop": r4(self.d_loop),
               "flags": self.flags, "f0_orig": self.f0_orig.map(r4), "f0_gen": self.f0_gen.map(r4)})
    }
}

pub fn r4(x: f32) -> f64 {
    (x as f64 * 1e4).round() / 1e4
}

fn db(p: f32) -> f32 {
    (10.0 * p.max(1e-12).log10()).max(FLOOR)
}

fn hz_to_mel(f: f32) -> f32 {
    2595.0 * (1.0 + f / 700.0).log10()
}

/// Log-mel spectrogram: frames × bands (dB), plus each frame's start sample.
pub fn mel_spec(x: &[f32], rate: f32, size: usize, bands: usize) -> (Vec<Vec<f32>>, Vec<usize>) {
    let hop = size / 4;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(size);
    let win: Vec<f32> = (0..size).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos()).collect();
    let wsum: f32 = win.iter().sum();
    // Triangular mel filters over bins 0..=size/2.
    let (m0, m1) = (hz_to_mel(30.0), hz_to_mel(rate / 2.0));
    let edges: Vec<f32> = (0..bands + 2)
        .map(|k| {
            let m = m0 + (m1 - m0) * k as f32 / (bands + 1) as f32;
            700.0 * (10f32.powf(m / 2595.0) - 1.0) * size as f32 / rate
        })
        .collect();
    let mut frames = Vec::new();
    let mut starts = Vec::new();
    let mut start = 0usize;
    loop {
        let mut buf: Vec<Complex32> =
            (0..size).map(|i| Complex32::new(x.get(start + i).copied().unwrap_or(0.0) * win[i], 0.0)).collect();
        fft.process(&mut buf);
        let pow: Vec<f32> = buf[..=size / 2].iter().map(|c| c.norm_sqr() / (wsum * wsum / 4.0)).collect();
        let row: Vec<f32> = (0..bands)
            .map(|b| {
                let (lo, mid, hi) = (edges[b], edges[b + 1], edges[b + 2]);
                let mut acc = 0.0;
                let mut wt = 0.0;
                for (k, p) in pow.iter().enumerate() {
                    let kf = k as f32;
                    let w = if kf > lo && kf <= mid {
                        (kf - lo) / (mid - lo).max(1e-6)
                    } else if kf > mid && kf < hi {
                        (hi - kf) / (hi - mid).max(1e-6)
                    } else {
                        0.0
                    };
                    acc += w * p;
                    wt += w;
                }
                // Bands narrower than a bin take the nearest bin.
                if wt == 0.0 { db(pow[(mid.round() as usize).min(size / 2)]) } else { db(acc / wt) }
            })
            .collect();
        frames.push(row);
        starts.push(start);
        start += hop;
        if start >= x.len() {
            break;
        }
    }
    (frames, starts)
}

/// Smallest STFT size / envelope window: at least two periods of the wave's pitch, so frame values follow the
/// envelope and not the waveform phase.
pub fn min_window(m: &WaveMeta) -> usize {
    let f0 = 440.0 * 2f32.powf((m.unity as f32 - 69.0) / 12.0);
    ((2.0 * m.rate as f32 / f0) as usize).max((0.01 * m.rate as f32) as usize)
}

fn d_spec(a: &[f32], b: &[f32], rate: f32, min_win: usize) -> f32 {
    let mut total = 0.0;
    let small = min_win.next_power_of_two().clamp(128, 1024);
    for (size, bands) in [(small, 20usize), (512.max(small), 40), (2048, 64)] {
        let (fa, sa) = mel_spec(a, rate, size, bands);
        let (fb, _) = mel_spec(b, rate, size, bands);
        let (mut acc, mut wt) = (0.0f32, 0.0f32);
        for ((ra, rb), st) in fa.iter().zip(&fb).zip(&sa) {
            let w = if (*st as f32) < 0.05 * rate { 2.0 } else { 1.0 };
            let d: f32 = ra.iter().zip(rb).map(|(x, y)| (x - y).abs()).sum::<f32>() / ra.len() as f32;
            acc += w * d;
            wt += w;
        }
        total += acc / wt.max(1e-9);
    }
    total / 3.0
}

pub fn rms_env(x: &[f32], frame: usize) -> Vec<f32> {
    x.chunks(frame).map(|c| db(c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32)).collect()
}

/// RMS (dB) over centred windows of `win` samples, every `hop` samples.
pub fn rms_env_win(x: &[f32], win: usize, hop: usize) -> Vec<f32> {
    let mut sq = vec![0.0f64; x.len() + 1];
    for (i, v) in x.iter().enumerate() {
        sq[i + 1] = sq[i] + (*v as f64) * (*v as f64);
    }
    (0..x.len().div_ceil(hop))
        .map(|k| {
            let c = k * hop;
            let (a, b) = (c.saturating_sub(win / 2), (c + win / 2).min(x.len()).max(c.saturating_sub(win / 2) + 1));
            db(((sq[b] - sq[a]) / (b - a) as f64) as f32)
        })
        .collect()
}

fn loop_db(x: &[f32]) -> Vec<f32> {
    let l = x.len();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(l);
    let mut buf: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    fft.process(&mut buf);
    buf[..=l / 2].iter().map(|c| db(c.norm_sqr() * 4.0 / (l * l) as f32)).collect()
}

/// Fundamental by normalised autocorrelation (20 Hz–2 kHz) over up to 2048 samples from `from`; `None` when the
/// best peak is weaker than 0.8 (unpitched).
pub fn pitch(x: &[f32], from: usize, rate: f32) -> Option<f32> {
    let seg: Vec<f32> = x.iter().skip(from).take(2048).copied().collect();
    let n = seg.len();
    let (lo, hi) = ((rate / 2000.0) as usize, ((rate / 20.0) as usize).min(n / 2));
    if hi <= lo + 2 {
        return None;
    }
    let e0: f32 = seg.iter().map(|v| v * v).sum();
    if e0 < 1e-9 {
        return None;
    }
    let r: Vec<f32> = (0..=hi + 1)
        .map(|lag| {
            let (mut c, mut e1, mut e2) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..n - lag {
                c += seg[i] * seg[i + lag];
                e1 += seg[i] * seg[i];
                e2 += seg[i + lag] * seg[i + lag];
            }
            c / (e1 * e2).sqrt().max(1e-12)
        })
        .collect();
    // First lag whose correlation is within 0.97 of the global best (avoids octave errors toward long lags).
    let best = (lo..=hi).map(|k| r[k]).fold(f32::MIN, f32::max);
    if best < 0.8 {
        return None;
    }
    let k = (lo..=hi).find(|&k| r[k] >= 0.97 * best && r[k] >= r[k - 1] && r[k] >= r[k + 1])?;
    let (a, b, c) = (r[k - 1], r[k], r[k + 1]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-9 { 0.5 * (a - c) / den } else { 0.0 };
    Some(rate / (k as f32 + off))
}

/// Scores `synth` against `orig` (both raw wave samples of length `m.len`).
pub fn score(orig: &[f32], synth: &[f32], m: &WaveMeta) -> Score {
    let rate = m.rate as f32;
    let (a, b) = (played(orig, m, PLAYED_SECS), played(synth, m, PLAYED_SECS));
    let mut flags = Vec::new();
    if synth.iter().any(|v| !v.is_finite()) {
        return Score { score: 0.0, d_spec: 99.0, d_env: 99.0, d_loop: 99.0, flags: vec!["nan"], f0_orig: None, f0_gen: None };
    }
    let win = min_window(m);
    let ds = d_spec(&a, &b, rate, win);
    let frame = (0.002 * rate) as usize;
    let (ea, eb) = (rms_env_win(&a, win, frame), rms_env_win(&b, win, frame));
    let de = ea.iter().zip(&eb).map(|(x, y)| (x - y).abs()).sum::<f32>() / ea.len().max(1) as f32;
    let dl = match loop_of(m) {
        Some((s, l)) if l >= 4 => {
            let (la, lb) = (loop_db(&orig[s..s + l]), loop_db(&synth[s..s + l]));
            // DC excluded: an offset in a tiny loop would dominate the mean.
            la.iter().zip(&lb).skip(1).map(|(x, y)| (x - y).abs()).sum::<f32>() / (la.len() - 1).max(1) as f32
        }
        _ => 0.0,
    };
    let peak = synth.iter().fold(0.0f32, |p, v| p.max(v.abs()));
    if peak < 1e-3 {
        flags.push("silent");
    }
    let clips = |x: &[f32]| {
        let mut run = 0;
        x.iter().any(|v| {
            run = if v.abs() >= 32767.0 / 32768.0 { run + 1 } else { 0 };
            run > 4
        })
    };
    // Only when the original does not clip there itself.
    if clips(synth) && !clips(orig) {
        flags.push("clipped");
    }
    // Envelope: any 10 ms span (5 frames) off by more than 6 dB where the original is above −40 dB.
    if ea.windows(5).zip(eb.windows(5)).any(|(x, y)| {
        let mx = x.iter().copied().fold(FLOOR, f32::max);
        let d = x.iter().zip(y).map(|(p, q)| (p - q).abs()).sum::<f32>() / 5.0;
        mx > -40.0 && d > 6.0
    }) {
        flags.push("env_off");
    }
    let from = loop_of(m).map_or(0, |(s, _)| s);
    let (fa, fb) = (pitch(&a, from, rate), pitch(&b, from, rate));
    if let (Some(x), Some(y)) = (fa, fb) {
        if (1200.0 * (y / x).log2()).abs() > 5.0 {
            flags.push("pitch_off");
        }
    }
    if let Some((s, l)) = loop_of(m) {
        // Curvature across the wrap (end → start) against the largest curvature inside the loop (last two
        // samples excluded, they take part in the wrap): an ordinary periodic join stays well below 4×.
        if l >= 8 {
            let g = &synth[s..s + l];
            let d2w = (g[0] - 2.0 * g[l - 1] + g[l - 2]).abs();
            let inside = g[..l - 2].windows(3).map(|w| (w[0] - 2.0 * w[1] + w[2]).abs()).fold(0.0f32, f32::max);
            if d2w > 4.0 * inside + 1e-4 {
                flags.push("loop_click");
            }
        }
    }
    let score = (-(ds / W_SPEC + de / W_ENV + dl / W_LOOP)).exp();
    Score { score, d_spec: ds, d_env: de, d_loop: dl, flags, f0_orig: fa, f0_gen: fb }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(len: usize, looped: Option<(u32, u32)>) -> WaveMeta {
        WaveMeta { rate: 22050, len, unity: 60, fine: 0, looped }
    }

    fn tone(len: usize, hz: f32, amp: f32) -> Vec<f32> {
        (0..len).map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / 22050.0).sin()).collect()
    }

    #[test]
    fn identical_scores_one() {
        let m = meta(4000, Some((2000, 2000)));
        let x = tone(4000, 22050.0 / 100.0, 0.5);
        let s = score(&x, &x, &m);
        assert_eq!((s.score, s.d_spec, s.d_env, s.d_loop), (1.0, 0.0, 0.0, 0.0));
        assert!(s.flags.is_empty(), "{:?}", s.flags);
    }

    #[test]
    fn silence_scores_below_tenth() {
        let m = meta(4000, Some((2000, 2000)));
        let x = tone(4000, 22050.0 / 100.0, 0.5);
        let s = score(&x, &vec![0.0; 4000], &m);
        assert!(s.score < 0.1, "{}", s.score);
        assert!(s.flags.contains(&"silent"));
    }

    #[test]
    fn detune_sets_pitch_off() {
        let m = meta(8000, None);
        let x = tone(8000, 220.0, 0.5);
        let y = tone(8000, 220.0 * 2f32.powf(10.0 / 1200.0), 0.5);
        let s = score(&x, &y, &m);
        assert!(s.flags.contains(&"pitch_off"), "{:?} {:?} {:?}", s.flags, s.f0_orig, s.f0_gen);
        let s2 = score(&x, &tone(8000, 220.0 * 2f32.powf(2.0 / 1200.0), 0.5), &m);
        assert!(!s2.flags.contains(&"pitch_off"));
    }

    #[test]
    fn wrap_step_sets_loop_click() {
        let m = meta(3000, Some((1000, 2000)));
        let x = tone(3000, 22050.0 / 100.0, 0.5);
        let mut y = x.clone();
        y[2999] = -0.9; // last loop sample jumps away from the loop start
        let s = score(&x, &y, &m);
        assert!(s.flags.contains(&"loop_click"), "{:?}", s.flags);
    }
}
