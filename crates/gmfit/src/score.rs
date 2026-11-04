//! Similarity score (design D6) of a generated wave against the original, on the "played" signals.
//!
//! - `d_spec`: mean |dB| difference of log-mel spectrograms at three STFT sizes (≥ 2 periods/512/2048, hop size/4, Hann),
//!   A-weighted per band, then floored at (peak − 50 dB) so inaudible differences do not count; frames in the first
//!   50 ms count double.
//! - `d_onset`: the same A-weighted comparison over the frames starting in the first 50 ms only, so an attack
//!   (a kick's click) weighs as much as the ear gives it instead of being diluted over 0.5 s.
//! - `d_env`: mean |dB| difference of RMS envelopes (window ≥ 2 periods and ≥ 10 ms, every 2 ms; floor −80 dB) over
//!   the attack. Over the looped part a held note's shimmer is a texture: there the envelopes are compared after
//!   100 ms smoothing, plus the difference of their fine-envelope standard deviations (wobble depth), so a
//!   shimmer of the right depth is not punished for a different pattern.
//! - `d_loop`: mean |dB| difference of the loop's exact DFT magnitudes (bins up to Nyquist; 0 when unlooped).
//! - `score = exp(−(d_spec/W_SPEC + d_onset/W_ONSET + d_env/W_ENV + d_loop/W_LOOP))`, 1 for identical waves.
use crate::audio::{loop_of, played};
use gmgen::render::WaveMeta;
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use serde_json::{json, Value};

pub const W_SPEC: f32 = 12.0;
pub const W_ENV: f32 = 6.0;
pub const W_LOOP: f32 = 12.0;
pub const W_ONSET: f32 = 12.0;
/// Periodicity difference (0–1): a pure tone vs narrow-band noise of the same spectrum (design D4l).
pub const W_PER: f32 = 1.0;
/// Modulation of the strongest partial (wavelet, design D4m): loudness flutter (per 6 dB) + pitch movement (per
/// 100 cents) difference. A trill (FM) vs beating partials (AM) of the same spectrum.
pub const W_MOD: f32 = 1.0;
const FLOOR: f32 = -80.0;
/// Dynamic range compared by `d_spec` (below the peak) and by `d_loop` (`LOOP_RANGE`).
pub const SPEC_RANGE: f32 = 50.0;
pub const LOOP_RANGE: f32 = 60.0;
/// Seconds of the played unroll that are compared.
pub const PLAYED_SECS: f32 = 0.5;

#[derive(Clone, Debug)]
pub struct Score {
    pub score: f32,
    pub d_spec: f32,
    pub d_env: f32,
    pub d_loop: f32,
    pub d_onset: f32,
    pub d_per: f32,
    pub d_mod: f32,
    pub flags: Vec<&'static str>,
    pub f0_orig: Option<f32>,
    pub f0_gen: Option<f32>,
    /// Character descriptors (`describe`, design D4n): fine contrast gen − orig (dB), percussive share
    /// (orig, gen), peaks per frame (orig, gen). Reported and flagged, not scored.
    pub desc: Option<Desc>,
}

#[derive(Clone, Copy, Debug)]
pub struct Desc {
    pub contrast_orig: f32,
    pub contrast: f32,
    pub perc: (f32, f32),
    pub peaks: (f32, f32),
}

/// Thresholds of the character flags, set on the drum descriptor study (design D4n).
pub const LINES_DB: f32 = 6.0;
pub const PERC_DIFF: f32 = 0.2;
pub const PEAKS_RATIO: f32 = 1.6;

impl Score {
    pub fn json(&self) -> Value {
        json!({"score": r4(self.score), "d_spec": r4(self.d_spec), "d_env": r4(self.d_env), "d_loop": r4(self.d_loop), "d_onset": r4(self.d_onset),
               "d_per": r4(self.d_per), "d_mod": r4(self.d_mod), "flags": self.flags, "f0_orig": self.f0_orig.map(r4), "f0_gen": self.f0_gen.map(r4),
               "desc": self.desc.map(|d| json!({"contrast_orig": r4(d.contrast_orig), "contrast": r4(d.contrast), "perc": [r4(d.perc.0), r4(d.perc.1)], "peaks": [r4(d.peaks.0), r4(d.peaks.1)]}))})
    }

    /// Adds the character descriptors over the first second played, with their flags: `lines` (copy ≥ 6 dB
    /// peakier where the original is a wash, ≤ `desc::WASH_DB`: a comb for a noise), `transient_off` (percussive share
    /// off by ≥ 0.2), `peaks_off` (peaks per frame off by ≥ 1.6×). Too slow for the fit loop.
    pub fn describe(&mut self, orig: &[f32], synth: &[f32], m: &WaveMeta) {
        if self.flags.contains(&"nan") {
            return;
        }
        let (a, b) = (played(orig, m, 1.0), played(synth, m, 1.0));
        let rate = m.rate as f32;
        let (co, cd) = crate::desc::contrast_pair(&a, &b, rate);
        let d = Desc {
            contrast_orig: co,
            contrast: cd,
            perc: (crate::desc::percussive_share(&a), crate::desc::percussive_share(&b)),
            peaks: (crate::desc::peaks_per_frame(&a), crate::desc::peaks_per_frame(&b)),
        };
        if d.contrast >= LINES_DB && d.contrast_orig <= crate::desc::WASH_DB {
            self.flags.push("lines");
        }
        if (d.perc.0 - d.perc.1).abs() >= PERC_DIFF {
            self.flags.push("transient_off");
        }
        let r = (d.peaks.1 + 1.0) / (d.peaks.0 + 1.0);
        if r >= PEAKS_RATIO || r <= 1.0 / PEAKS_RATIO {
            self.flags.push("peaks_off");
        }
        self.desc = Some(d);
    }
}

pub fn r4(x: f32) -> f64 {
    (x as f64 * 1e4).round() / 1e4
}

fn db(p: f32) -> f32 {
    (10.0 * p.max(1e-12).log10()).max(FLOOR)
}

/// A-weighting (IEC 61672) in dB at `f` Hz: the ear's sensitivity, about −30 dB at 50 Hz, 0 at 1 kHz, +1.3 at
/// 2.5 kHz. Added to band levels before the audibility floor so quiet but audible high content (a kick's click)
/// counts and inaudible low rumble does not dominate.
pub fn a_weight(f: f32) -> f32 {
    let f2 = (f.max(1.0) as f64).powi(2);
    let ra = 12194f64.powi(2) * f2 * f2
        / ((f2 + 20.6f64.powi(2)) * ((f2 + 107.7f64.powi(2)) * (f2 + 737.9f64.powi(2))).sqrt() * (f2 + 12194f64.powi(2)));
    (20.0 * ra.log10() + 2.0) as f32
}

/// Centre frequencies (Hz) of `mel_spec`'s bands.
fn mel_centres(rate: f32, bands: usize) -> Vec<f32> {
    let (m0, m1) = (hz_to_mel(30.0), hz_to_mel(rate / 2.0));
    (1..=bands).map(|k| 700.0 * (10f32.powf((m0 + (m1 - m0) * k as f32 / (bands + 1) as f32) / 2595.0) - 1.0)).collect()
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
    // Bands at least 3 bins wide (fewer bins make the band level of a noise a random variable).
    let edges: Vec<(f32, f32, f32)> = (0..bands)
        .map(|b| {
            let (lo, mid, hi) = (edges[b], edges[b + 1], edges[b + 2]);
            if hi - lo >= 3.0 { (lo, mid, hi) } else { (mid - 1.5, mid, mid + 1.5) }
        })
        .collect();
    let mut frames: Vec<Vec<f32>> = Vec::new();
    let mut starts = Vec::new();
    let mut start = 0usize;
    loop {
        // Frames are centred on `start` (zero padding before 0), so the onset sits under the window's peak.
        let mut buf: Vec<Complex32> = (0..size)
            .map(|i| {
                let j = (start + i).checked_sub(size / 2);
                Complex32::new(j.and_then(|j| x.get(j)).copied().unwrap_or(0.0) * win[i], 0.0)
            })
            .collect();
        fft.process(&mut buf);
        let pow: Vec<f32> = buf[..=size / 2].iter().map(|c| c.norm_sqr() / (wsum * wsum / 4.0)).collect();
        let row: Vec<f32> = (0..bands)
            .map(|b| {
                let (lo, mid, hi) = edges[b];
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
                if wt == 0.0 { pow[(mid.round() as usize).min(size / 2)] } else { acc / wt }
            })
            .collect();
        frames.push(row);
        starts.push(start);
        start += hop;
        if start >= x.len() {
            break;
        }
    }
    // Power smoothed over 5 frames (±2) before dB, so independent noises of the same colour compare equal.
    let n = frames.len();
    let smoothed: Vec<Vec<f32>> = (0..n)
        .map(|k| {
            let (a, b) = (k.saturating_sub(2), (k + 2).min(n - 1));
            (0..bands).map(|j| db((a..=b).map(|q| frames[q][j]).sum::<f32>() / (b - a + 1) as f32)).collect()
        })
        .collect();
    (smoothed, starts)
}

/// Smallest STFT size / envelope window: at least two periods of the wave's pitch, so frame values follow the
/// envelope and not the waveform phase.
pub fn min_window(m: &WaveMeta) -> usize {
    let f0 = 440.0 * 2f32.powf((m.unity as f32 - 69.0) / 12.0);
    ((2.0 * m.rate as f32 / f0) as usize).max((0.01 * m.rate as f32) as usize)
}

/// `(d_spec, d_onset)`.
fn d_spec(a: &[f32], b: &[f32], rate: f32, min_win: usize) -> (f32, f32) {
    let mut total = 0.0;
    let mut onset = 0.0;
    let small = min_win.next_power_of_two().clamp(128, 1024);
    for (size, bands) in [(small, 20usize), (512.max(small), 40), (2048, 64)] {
        let (mut fa, sa) = mel_spec(a, rate, size, bands);
        let (mut fb, _) = mel_spec(b, rate, size, bands);
        let aw: Vec<f32> = mel_centres(rate, bands).iter().map(|f| a_weight(*f)).collect();
        for row in fa.iter_mut().chain(fb.iter_mut()) {
            row.iter_mut().zip(&aw).for_each(|(v, w)| *v += w);
        }
        // Audibility floor: differences more than SPEC_RANGE dB below the louder spectrogram's peak do not count.
        let top = fa.iter().chain(&fb).flatten().copied().fold(FLOOR, f32::max);
        for v in fa.iter_mut().chain(fb.iter_mut()).flatten() {
            *v = v.max(top - SPEC_RANGE);
        }
        let (mut acc, mut wt, mut on, mut non) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for ((ra, rb), st) in fa.iter().zip(&fb).zip(&sa) {
            let early = (*st as f32) < 0.05 * rate;
            let w = if early { 2.0 } else { 1.0 };
            let d: f32 = ra.iter().zip(rb).map(|(x, y)| (x - y).abs()).sum::<f32>() / ra.len() as f32;
            acc += w * d;
            wt += w;
            if early {
                on += d;
                non += 1.0;
            }
        }
        total += acc / wt.max(1e-9);
        onset += on / non.max(1.0);
    }
    (total / 3.0, onset / 3.0)
}

/// Envelope distance: exact over frames before `loop_from`; smoothed (±`smooth`/2 frames) plus wobble-depth
/// difference over the rest (see module doc).
fn d_env(ea: &[f32], eb: &[f32], loop_from: Option<usize>, smooth: usize) -> f32 {
    let n = ea.len().min(eb.len());
    if n == 0 {
        return 0.0;
    }
    let lf = loop_from.unwrap_or(n).min(n);
    let mut sum: f32 = ea[..lf].iter().zip(&eb[..lf]).map(|(x, y)| (x - y).abs()).sum();
    if lf < n {
        let h = (smooth / 2).max(1);
        let sm = |e: &[f32], k: usize| {
            let (a, b) = (k.saturating_sub(h).max(lf), (k + h).min(n - 1));
            e[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
        };
        sum += (lf..n).map(|k| (sm(ea, k) - sm(eb, k)).abs()).sum::<f32>();
        let sd = |e: &[f32]| {
            let r: Vec<f32> = (lf..n).map(|k| e[k] - sm(e, k)).collect();
            (r.iter().map(|v| v * v).sum::<f32>() / r.len() as f32).sqrt()
        };
        sum += (sd(ea) - sd(eb)).abs() * (n - lf) as f32;
    }
    sum / n as f32
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

fn loop_power(x: &[f32]) -> Vec<f32> {
    let l = x.len();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(l);
    let mut buf: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    fft.process(&mut buf);
    buf[..=l / 2].iter().map(|c| c.norm_sqr() * 4.0 / (l * l) as f32).collect()
}

/// Mean |dB| difference of the two loops' spectra, compared per harmonic cluster (bins within ±m/2 of `k·m`, m =
/// periods per loop) when pitched, per third-octave band otherwise; DC excluded. A short loop (m = 1) is compared
/// bin by bin; a long ensemble loop by the energy around each harmonic, not by its exact bins.
pub fn loop_distance(a: &[f32], b: &[f32], rate: f32, periods: Option<usize>) -> f32 {
    let (pa, pb) = (loop_power(a), loop_power(b));
    let n = pa.len() - 1;
    let groups: Vec<(usize, usize)> = match periods {
        Some(m) if m >= 1 && m <= n => {
            let w = m / 2;
            (1..=n / m).map(|k| ((k * m).saturating_sub(w).max(1), (k * m + w).min(n))).collect()
        }
        _ => {
            let mut g = Vec::new();
            let mut lo = 1usize;
            while lo <= n {
                let hi = ((lo as f32 * 1.26).ceil() as usize).max(lo).min(n);
                g.push((lo, hi));
                lo = hi + 1;
            }
            g
        }
    };
    let hz = |lo: usize, hi: usize| (lo + hi) as f32 / 2.0 * rate / a.len() as f32;
    let la: Vec<f32> = groups.iter().map(|&(lo, hi)| db(pa[lo..=hi].iter().sum()) + a_weight(hz(lo, hi))).collect();
    let lb: Vec<f32> = groups.iter().map(|&(lo, hi)| db(pb[lo..=hi].iter().sum()) + a_weight(hz(lo, hi))).collect();
    let top = la.iter().chain(&lb).copied().fold(FLOOR, f32::max);
    let d: f32 = la.iter().zip(&lb).map(|(x, y)| (x.max(top - LOOP_RANGE) - y.max(top - LOOP_RANGE)).abs()).sum();
    d / groups.len().max(1) as f32
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

/// Buzz: within-period peakiness of the content above 2.5 kHz — mean over periods of (peak of the 0.5 ms
/// power) / (mean power). Phase-aligned upper partials pulse once per period (≈5 for a steady Schroeder
/// spectrum); a string section's scattered partials stay near 3. `period` in samples; 0 when silent.
pub fn buzz(x: &[f32], rate: f32, period: usize) -> f32 {
    buzz_band(x, rate, period, 2500.0, None)
}

/// [`buzz`] of the band `lo`…`hi` Hz (4th-order Butterworth-ish edges; `hi` None = no low-pass).
pub fn buzz_band(x: &[f32], rate: f32, period: usize, lo: f32, hi: Option<f32>) -> f32 {
    let biquad = |y: &mut [f32], f: f32, high: bool| {
        let w = std::f32::consts::TAU * f / rate;
        let (cw, al) = (w.cos(), w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2));
        let a0 = 1.0 + al;
        let (b0, b1) = if high { ((1.0 + cw) / 2.0 / a0, -(1.0 + cw) / a0) } else { ((1.0 - cw) / 2.0 / a0, (1.0 - cw) / a0) };
        let (a1, a2) = (-2.0 * cw / a0, (1.0 - al) / a0);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for v in y.iter_mut() {
            let o = b0 * *v + b1 * x1 + b0 * x2 - a1 * y1 - a2 * y2;
            (x2, x1, y2, y1) = (x1, *v, y1, o);
            *v = o;
        }
    };
    let mut y = x.to_vec();
    for _ in 0..2 {
        biquad(&mut y, lo, true);
        if let Some(h) = hi {
            biquad(&mut y, h, false);
        }
    }
    let k = ((0.0005 * rate) as usize).max(1);
    let p: Vec<f32> = y.windows(k).map(|w| w.iter().map(|v| v * v).sum::<f32>() / k as f32).collect();
    if period < 4 || p.len() < 2 * period {
        return 0.0;
    }
    let ch: Vec<f32> = p.chunks_exact(period).filter_map(|c| {
        let m = c.iter().sum::<f32>() / period as f32;
        (m > 1e-12).then(|| c.iter().copied().fold(0.0, f32::max) / m)
    }).collect();
    if ch.is_empty() { 0.0 } else { ch.iter().sum::<f32>() / ch.len() as f32 }
}

/// Scores `synth` against `orig` (both raw wave samples of length `m.len`).
pub fn score(orig: &[f32], synth: &[f32], m: &WaveMeta) -> Score {
    let rate = m.rate as f32;
    let (a, b) = (played(orig, m, PLAYED_SECS), played(synth, m, PLAYED_SECS));
    let mut flags = Vec::new();
    if synth.iter().any(|v| !v.is_finite()) {
        return Score { score: 0.0, d_spec: 99.0, d_env: 99.0, d_loop: 99.0, d_onset: 99.0, d_per: 1.0, d_mod: 1.0, flags: vec!["nan"], f0_orig: None, f0_gen: None, desc: None };
    }
    let win = min_window(m);
    let (ds, don) = d_spec(&a, &b, rate, win);
    let frame = (0.002 * rate) as usize;
    let (ea, eb) = (rms_env_win(&a, win, frame), rms_env_win(&b, win, frame));
    let de = d_env(&ea, &eb, loop_of(m).map(|(s, _)| s / frame), (0.1 * rate) as usize / frame);
    let from = loop_of(m).map_or(0, |(s, _)| s);
    let (fa, fb) = (pitch(&a, from, rate), pitch(&b, from, rate));
    let dl = match loop_of(m) {
        Some((s, l)) if l >= 4 => loop_distance(&orig[s..s + l], &synth[s..s + l], rate, fa.map(|f| ((l as f32 * f / rate).round() as usize).max(1))),
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
    if let (Some(x), Some(y)) = (fa, fb) {
        if (1200.0 * (y / x).log2()).abs() > 5.0 {
            flags.push("pitch_off");
        }
    }
    // Buzz (not in the score — a per-period effect the frame metrics average out): upper band clearly more
    // pulse-like than the original's, where it carries audible energy.
    if let (Some(f), Some(_)) = (fa, loop_of(m)) {
        let per = (rate / f).round() as usize;
        let skip = (0.1 * rate) as usize;
        if a.len() > skip + 4 * per {
            let (ba, bb) = (buzz(&a[skip..], rate, per), buzz(&b[skip..], rate, per));
            if ba > 0.0 && bb > 1.4 * ba {
                flags.push("buzz");
            }
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
    let dp = match (periodicity(&a, rate), periodicity(&b, rate)) {
        (Some(x), Some(y)) => (x - y).abs(),
        _ => 0.0,
    };
    if dp > 0.25 {
        flags.push("tone_off");
    }
    // Only for a real partial: the original tonal (tonal share > 0.6) and its partial stable (pitch sd < 300 cents);
    // on noise the "partial" moves at random. Each part capped at 1.
    let tonal = crate::modal::tonal_share(&a[..a.len().min((0.3 * rate) as usize)], rate) > 0.6;
    let dm = match strongest_partial(&a, rate).filter(|_| tonal) {
        Some(f) => match (modulation(&a, rate, f), modulation(&b, rate, f)) {
            (Some((am_a, fm_a, ra)), Some((am_b, fm_b, rb))) if fm_a < 300.0 => {
                // Rate of the pitch movement too, when the original really trills (≥ 50 cents; whistles 93–119):
                // a steady tone's 15–25 cents of measurement wobble has a random rate (bass 240 lost 0.6 to it).
                let rate_term = if fm_a >= 50.0 && ra > 0.0 && rb > 0.0 { (rb / ra).log2().abs().min(1.0) } else { 0.0 };
                ((am_a - am_b).abs() / 6.0).min(1.0) + ((fm_a - fm_b).abs() / 100.0).min(1.0) + rate_term
            }
            _ => 0.0,
        },
        None => 0.0,
    };
    if dm > 0.5 {
        flags.push("mod_off");
    }
    let score = (-(ds / W_SPEC + don / W_ONSET + de / W_ENV + dl / W_LOOP + dp / W_PER + dm / W_MOD)).exp();
    Score { score, d_spec: ds, d_env: de, d_loop: dl, d_onset: don, d_per: dp, d_mod: dm, flags, f0_orig: fa, f0_gen: fb, desc: None }
}

/// Strongest spectral peak (Hz, 100 Hz – 0.45·rate) of the first 300 ms.
fn strongest_partial(x: &[f32], rate: f32) -> Option<f32> {
    let n = x.len().min((0.3 * rate) as usize);
    if n < 256 {
        return None;
    }
    let nfft = n.next_power_of_two() * 2;
    let mut buf: Vec<Complex32> = (0..nfft)
        .map(|k| {
            let w = if k < n { 0.5 - 0.5 * (std::f32::consts::TAU * (k as f32 + 0.5) / n as f32).cos() } else { 0.0 };
            Complex32::new(if k < n { x[k] * w } else { 0.0 }, 0.0)
        })
        .collect();
    FftPlanner::<f32>::new().plan_fft_forward(nfft).process(&mut buf);
    let hz = |k: usize| k as f32 * rate / nfft as f32;
    (1..nfft / 2).filter(|&k| hz(k) >= 100.0 && hz(k) <= 0.45 * rate).max_by(|a, b| buf[*a].norm_sqr().total_cmp(&buf[*b].norm_sqr())).map(hz)
}

/// Loudness flutter (dB sd), pitch movement (cents sd) and its rate (Hz) of the partial at `f`: Morlet band (sd min(f/4, 250 Hz)) over the part
/// within 12 dB of its peak in the first 300 ms, after removing the slow trend (25 ms moving average) so decays
/// and glides do not count. `None` when that part is shorter than 100 ms.
fn modulation(x: &[f32], rate: f32, f: f32) -> Option<(f32, f32, f32)> {
    let x = &x[..x.len().min((0.3 * rate) as usize)];
    // Band sd min(f/4, 250 Hz): passes a ±100 Hz trill's sidebands, keeps neighbouring partials out.
    let (amp, inst) = crate::wavelet::band(x, rate, f, (f / 250.0).max(4.0));
    let top = amp.iter().copied().fold(0.0f32, f32::max);
    let loud: Vec<usize> = (0..amp.len()).filter(|&k| amp[k] > 0.25 * top).collect();
    let (a, b) = (*loud.first()?, *loud.last()?);
    // Sustained sounds only (≥ 100 ms loud): on a short strike the decay and glide leak into the measure.
    if b < a + (0.1 * rate) as usize {
        return None;
    }
    let w = ((0.025 * rate) as usize).max(1);
    let detrend_sd = |v: &[f32]| {
        let n = v.len();
        let mut acc = 0.0f32;
        let mut s = 0.0f32;
        for k in 0..n {
            let lo = k.saturating_sub(w / 2);
            let hi = (k + w / 2).min(n - 1);
            let m = v[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32;
            let d = v[k] - m;
            acc += d * d;
            s += 1.0;
        }
        (acc / s.max(1.0)).sqrt()
    };
    let db: Vec<f32> = amp[a..=b].iter().map(|v| 20.0 * v.max(1e-9).log10()).collect();
    let cents: Vec<f32> = inst[a..=b].iter().map(|v| 1200.0 * (v.max(1.0) / f).log2()).collect();
    // Rate of the pitch movement: strongest 15–400 Hz periodicity of the cents curve.
    let (r, _) = crate::wavelet::mod_rate(&cents, rate, 15.0, 400.0);
    Some((detrend_sd(&db), detrend_sd(&cents), r))
}

/// Periodicity (design D4l): mean over 30 ms frames (hop 15 ms, first 300 ms, frames within 30 dB of the loudest)
/// of the highest normalised autocorrelation at lags 0.5–25 ms. ≈1 for a tone (gliding too), lower for noise.
pub fn periodicity(x: &[f32], rate: f32) -> Option<f32> {
    let n = (0.03 * rate) as usize;
    let hop = n / 2;
    let end = x.len().min((0.3 * rate) as usize);
    if end < n {
        return None;
    }
    let frames: Vec<usize> = (0..=(end - n)).step_by(hop).collect();
    let en: Vec<f32> = frames.iter().map(|&a| x[a..a + n].iter().map(|v| v * v).sum()).collect();
    let top = en.iter().copied().fold(0.0f32, f32::max);
    if top < 1e-9 {
        return None;
    }
    let nfft = (2 * n).next_power_of_two();
    let mut planner = FftPlanner::<f32>::new();
    let (fwd, inv) = (planner.plan_fft_forward(nfft), planner.plan_fft_inverse(nfft));
    let (lo, hi) = (((0.0005 * rate) as usize).max(2), ((0.025 * rate) as usize).min(n - 1));
    let mut vals = Vec::new();
    for (&a, &e) in frames.iter().zip(&en) {
        if e < top * 1e-3 {
            continue;
        }
        let mut buf: Vec<Complex32> = (0..nfft).map(|k| Complex32::new(if k < n { x[a + k] } else { 0.0 }, 0.0)).collect();
        fwd.process(&mut buf);
        buf.iter_mut().for_each(|c| *c = Complex32::new(c.norm_sqr(), 0.0));
        inv.process(&mut buf);
        let r0 = buf[0].re.max(1e-20);
        // Unbiased: divide each lag by its overlap so long lags are not penalised.
        let best = (lo..=hi).map(|k| buf[k].re / r0 * n as f32 / (n - k) as f32).fold(f32::MIN, f32::max);
        vals.push(best.clamp(0.0, 1.0));
    }
    (!vals.is_empty()).then(|| vals.iter().sum::<f32>() / vals.len() as f32)
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
    fn aligned_upper_partials_buzz() {
        // 40 equal harmonics of 220 Hz: in-phase partials pulse once per period, scattered phases do not.
        let (rate, f) = (22050.0f32, 220.0f32);
        let tone = |ph: &dyn Fn(usize) -> f32| -> Vec<f32> {
            (0..11025).map(|n| (1..=40).map(|k| (std::f32::consts::TAU * (k as f32 * f * n as f32 / rate + ph(k))).cos()).sum::<f32>() / 40.0).collect()
        };
        let aligned = tone(&|_| 0.0);
        // (A phase linear in k is only a delay — still one pulse per period; mix the hash.)
        let scattered = tone(&|k| {
            let h = (k as u32).wrapping_mul(2_654_435_761);
            ((h ^ (h >> 15)).wrapping_mul(0x85EB_CA6B) >> 8) as f32 / (1u32 << 24) as f32
        });
        let per = (rate / f).round() as usize;
        let (ba, bs) = (buzz(&aligned, rate, per), buzz(&scattered, rate, per));
        assert!(ba > 1.5 * bs, "aligned {ba} scattered {bs}");
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

    /// Two independent noises with the same colour and envelope must compare as (nearly) equal.
    #[test]
    fn independent_noises_compare_equal() {
        let noise = |seed: u64| {
            let mut r = seed;
            (0..11025)
                .map(|i| {
                    r ^= r << 13;
                    r ^= r >> 7;
                    r ^= r << 17;
                    ((r >> 11) as f32 / (1u64 << 53) as f32 - 0.5) * (-(i as f32) / 3000.0).exp()
                })
                .collect::<Vec<f32>>()
        };
        let m = meta(11025, None);
        let s = score(&noise(1), &noise(2), &m);
        assert!(s.d_spec < 1.5, "d_spec {}", s.d_spec);
    }

    /// Over a loop, two shimmers of equal depth but different pattern compare close; flat vs shimmer does not.
    #[test]
    fn loop_shimmer_compares_by_depth() {
        let (n, lf) = (250usize, 10usize);
        let wob = |ph: f32, depth: f32| -> Vec<f32> {
            (0..n).map(|k| if k < lf { -10.0 } else { -10.0 + depth * ((k as f32 * 0.37 + ph).sin() + 0.5 * (k as f32 * 0.11 + 2.0 * ph).sin()) }).collect()
        };
        let (a, b, flat) = (wob(0.0, 1.5), wob(2.1, 1.5), wob(0.0, 0.0));
        let same = d_env(&a, &b, Some(lf), 50);
        let diff = d_env(&a, &flat, Some(lf), 50);
        assert!(same < 0.4, "same-depth shimmer {same}");
        assert!(diff > 2.0 * same && diff > 0.8, "flat vs shimmer {diff} (same {same})");
    }

    #[test]
    fn a_weighting_reference_points() {
        assert!((a_weight(1000.0)).abs() < 0.1);
        assert!((a_weight(50.0) + 30.2).abs() < 0.5);
        assert!((a_weight(100.0) + 19.1).abs() < 0.5);
        assert!((a_weight(4000.0) - 1.0).abs() < 0.2);
    }

    /// A kick missing its broadband click must score clearly lower than the full kick against itself.
    #[test]
    fn missing_click_is_penalised() {
        let rate = 22050.0;
        let body: Vec<f32> = (0..2400).map(|i| (std::f32::consts::TAU * 55.0 * i as f32 / rate).sin() * 0.8 * (-(i as f32) / 600.0).exp()).collect();
        let mut r = 7u32;
        let with_click: Vec<f32> = body
            .iter()
            .enumerate()
            .map(|(i, v)| {
                r ^= r << 13;
                r ^= r >> 17;
                r ^= r << 5;
                v + (r as f32 / u32::MAX as f32 - 0.5) * 0.3 * (-(i as f32) / 40.0).exp()
            })
            .collect();
        let m = meta(2400, None);
        let s = score(&with_click, &body, &m);
        assert!(s.d_onset > 3.0, "click missing only costs d_onset {}", s.d_onset);
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
