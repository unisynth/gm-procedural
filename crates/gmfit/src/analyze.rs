//! `gmfit analyze`: initial parameters from the original wave and the GM priors.
//!
//! - Pitch: periods in the loop `m = round(L·f0/rate)`, so the wave pitch is exactly `m·rate/L` and harmonic `k`
//!   sits on loop bin `k·m` (the generator snaps partials to the same grid).
//! - Sustain (looped): an `additive` layer whose harmonic table is the loop DFT at bins `k·m` (stiff-string B from
//!   the peak positions when the prior is inharmonic), plus a periodic `noise` layer for the energy between the
//!   harmonics when that is above −25 dB. The attack envelope comes from the 2 ms RMS curve against the loop RMS.
//! - Attack/whole: `drum`, `metal` or `noise` from the prior (pitch glide, decay, spectral centroid, 8 log bands),
//!   each layer's level calibrated by rendering it alone and matching RMS.
use crate::audio::loop_of;
use crate::priors::{self, Kind, Prior};
use crate::score::{pitch, rms_env};
use gmgen::params::{Layer, Params};
use gmgen::params_json::WaveFile;
use gmgen::render::WaveMeta;
use gmgen::schema::{family, Region, FAMILIES, MAX_NUMBERS};
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

/// Prior of the first instrument that uses wave `i` (standard kit preferred for drum waves).
pub fn prior_for_wave(i: usize) -> Prior {
    let bank = gmgen::bank::skeleton();
    let mut best: Option<(u8, Prior)> = None;
    for ins in &bank.instruments {
        for r in ins.regions.iter().filter(|r| r.wave as usize == i) {
            let (rank, p) = if ins.drums {
                (if ins.program == 0 { 0 } else { 1 }, priors::for_drum_key(r.key_lo.min(127) as u8))
            } else {
                (if ins.bank_msb == 0 { 0 } else { 2 }, priors::for_program(ins.program))
            };
            if best.is_none_or(|(b, _)| rank < b) {
                best = Some((rank, p));
            }
        }
    }
    best.map_or(priors::for_program(127), |b| b.1)
}

/// Loop DFT: magnitudes (amplitude units) and phases (radians, cosine reference) for bins 0..=L/2.
fn dft(x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let l = x.len();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(l);
    let mut buf: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    fft.process(&mut buf);
    (buf[..=l / 2].iter().map(|c| 2.0 * c.norm() / l as f32).collect(), buf[..=l / 2].iter().map(|c| c.arg()).collect())
}

/// Mean power spectrum (Hann, 2048) of `x`, bins 0..=1024.
pub(crate) fn avg_power(x: &[f32]) -> Vec<f32> {
    let n = 2048;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
    let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()).collect();
    let mut acc = vec![0.0f32; n / 2 + 1];
    let mut frames = 0;
    let mut start = 0;
    loop {
        let mut buf: Vec<Complex32> = (0..n).map(|i| Complex32::new(x.get(start + i).copied().unwrap_or(0.0) * win[i], 0.0)).collect();
        fft.process(&mut buf);
        for (a, c) in acc.iter_mut().zip(&buf) {
            *a += c.norm_sqr();
        }
        frames += 1;
        start += n / 2;
        if start >= x.len() {
            break;
        }
    }
    acc.iter().map(|a| a / frames as f32).collect()
}

/// 8 log-spaced bands `[centre, Q, gain dB]` from a power spectrum (bin width `rate / (2·(len−1))`).
pub(crate) fn bands(power: &[f32], rate: f32) -> Vec<f32> {
    bands_n(power, rate, 8, 1.8)
}

/// `n` log-spaced bands 120 Hz – 0.45·rate with band-pass `q`, gains re the strongest band (≥ −60 dB).
pub(crate) fn bands_n(power: &[f32], rate: f32, n: usize, q: f32) -> Vec<f32> {
    let nb = power.len() - 1;
    let hz = |b: usize| b as f32 * rate / (2.0 * nb as f32);
    let (lo, hi) = (120.0f32, rate * 0.45);
    let edges: Vec<f32> = (0..=n).map(|k| lo * (hi / lo).powf(k as f32 / n as f32)).collect();
    let mut out = Vec::new();
    let mut gains = Vec::new();
    for k in 0..n {
        let e: f32 = (0..=nb).filter(|&b| hz(b) >= edges[k] && hz(b) < edges[k + 1]).map(|b| power[b]).sum::<f32>()
            / (edges[k + 1] - edges[k]);
        gains.push(10.0 * e.max(1e-20).log10());
    }
    let top = gains.iter().copied().fold(f32::MIN, f32::max);
    for k in 0..n {
        let c = (edges[k] * edges[k + 1]).sqrt();
        out.extend([c, q, (gains[k] - top).max(-60.0)]);
    }
    out
}

/// Pitch track of a decaying body: periods between upward zero crossings of the one-pole low-passed signal
/// (cutoff `lp` Hz). Returns `(time s, Hz)` pairs.
fn zero_cross_track(x: &[f32], rate: f32, lp: f32) -> Vec<(f32, f32)> {
    let k = 1.0 - (-std::f32::consts::TAU * lp / rate).exp();
    let mut y = 0.0f32;
    let mut prev = 0.0f32;
    let mut last: Option<f32> = None;
    let mut out = Vec::new();
    let peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    for (i, v) in x.iter().enumerate() {
        y += k * (v - y);
        if prev < 0.0 && y >= 0.0 && peak > 0.0 {
            let t = (i as f32 - 1.0 + prev / (prev - y)) / rate;
            if let Some(l) = last {
                let f = 1.0 / (t - l);
                if (20.0..2000.0).contains(&f) {
                    out.push(((t + l) / 2.0, f));
                }
            }
            last = Some(t);
        }
        prev = y;
    }
    out
}

/// Fit-relevant scalars made explicit (so `fit` tunes them) with their current values.
pub(crate) fn open_for_fit(l: &mut Layer) {
    let names: &[&str] = match gmgen::schema::FAMILIES[l.family].name {
        "additive" => &["level_db", "a", "h", "d", "s", "tilt", "tilt_t", "pdecay", "inharm"],
        "noise" => &["level_db", "a", "h", "d", "s", "hp", "lp"],
        "drum" => &["level_db", "freq", "ratio", "sweep_t", "decay", "a", "shape", "drive", "click_db", "click_decay"],
        "metal" => &["level_db", "base", "bp", "q", "hp", "a", "d", "d2", "split", "noise"],
        "modal" => &["level_db", "ha", "glide", "glide_t", "period", "hit_db", "click_db", "click_decay", "click_hp"],
        _ => &[],
    };
    for n in names {
        let v = l.get(n);
        l.set(n, v);
    }
}

pub(crate) fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Scales layer `k` so that, rendered alone, its RMS over `range` matches `target`.
pub(crate) fn calibrate(i: usize, p: &mut Params, k: usize, target: f32, range: std::ops::Range<usize>) {
    let mut solo = p.clone();
    solo.layers = vec![p.layers[k].clone()];
    let x = gmgen::render_wave(i, &solo);
    let r = rms(&x[range.start.min(x.len())..range.end.min(x.len())]);
    if r > 1e-9 && target > 1e-9 {
        let cur = p.layers[k].get("level_db");
        p.layers[k].set("level_db", (cur + 20.0 * (target / r).log10()).clamp(-80.0, 24.0));
    }
}

/// Envelope `a h d s` (relative to the level at `until`) from the 2 ms RMS curve over `[0, until)`.
fn envelope(x: &[f32], rate: f32, until: usize, l: &mut Layer, reference: f32) {
    let frame = (0.002 * rate) as usize;
    let e: Vec<f32> = rms_env(&x[..until.min(x.len())], frame).iter().map(|d| 10f32.powf(d / 20.0) / reference.max(1e-9)).collect();
    if e.is_empty() {
        return;
    }
    let (kp, peak) = e.iter().enumerate().fold((0, 0.0f32), |b, (k, v)| if *v > b.1 { (k, *v) } else { b });
    // Attack = rise to 70 % of the peak, on a 0.5 ms envelope (2 ms frames cannot resolve an organ's instant
    // start); the rest up to the peak is hold.
    let ft = frame as f32 / rate;
    let fine_frame = ((0.0005 * rate) as usize).max(1);
    let fine: Vec<f32> =
        rms_env(&x[..until.min(x.len())], fine_frame).iter().map(|d| 10f32.powf(d / 20.0) / reference.max(1e-9)).collect();
    let ka = fine.iter().position(|v| *v >= 0.7 * peak).map_or(kp as f32 * ft, |k| (k as f32 + 0.5) * fine_frame as f32 / rate);
    let a = ka.max(0.0002).min(until as f32 / rate * 0.9);
    l.set("a", a);
    l.set("h", ((kp as f32 + 0.5) * ft - a).clamp(0.0, until as f32 / rate * 0.9));
    if peak > 1.05 {
        let s = 1.0 / peak;
        let target = s + (1.0 - s) / std::f32::consts::E;
        let k1 = e.iter().skip(kp).position(|v| v / peak <= target).map_or(e.len() - kp, |k| k);
        l.set("s", s.clamp(0.0, 1.0));
        l.set("d", (k1 as f32 * frame as f32 / rate).max(0.002));
    }
}

/// Energy between the harmonics (bow/breath noise, a section's smear) as a periodic `noise` sustain layer.
/// Measured in the middle of the gaps (≥ per/5 bins from every harmonic — closer bins hold vibrato sidebands the
/// model already makes): the original's minus the model's density, averaged per band of [`bands`] (signed, so
/// bin-level randomness cancels), shapes and levels the noise. Then 3 passes on the render move each band's gain
/// by the original/model far-gap ratio and re-level every harmonic so line + noise in its region = the
/// original's region total (a section's upper "harmonics" are mostly that floor). The noise is high-passed at
/// 0.7·f0 (band skirts and the loop pattern otherwise reach DC); its low-pass is picked by score from
/// {0.18, 0.23, 0.27, 0.32}·rate and open (one wide top band cannot follow a falling top octave).
/// Budget: the weakest `side` components (≤ −20 dB re their harmonic) go first, then the harmonic table's tail
/// (down to 16). Re-runnable: an existing noise sustain layer is replaced (call it again once the correction
/// tables have moved the harmonics). Only for an additive sustain with a pitched loop, when the noise is above
/// −40 dB re the loop. Returns whether `p` changed (only when the score does not drop by more than 0.005).
pub fn gap_noise(i: usize, orig: &[f32], m: &WaveMeta, p: &mut Params) -> bool {
    let Some((s, l)) = loop_of(m) else { return false };
    let is = |x: &Layer, f: &str| FAMILIES[x.family].name == f && x.region == Region::Sustain;
    let Some(k) = p.layers.iter().position(|x| is(x, "additive")) else { return false };
    let mut base = p.clone();
    base.layers.retain(|x| !is(x, "noise"));
    if base.layers.len() >= gmgen::schema::MAX_LAYERS {
        return false;
    }
    let rate = m.rate as f32;
    let Some(f0) = pitch(orig, s, rate) else { return false };
    let (mo, _) = dft(&orig[s..s + l]);
    // Periods per loop: the pitch estimate can be one off (vibrato), which puts the "gaps" on the upper
    // harmonics; take the candidate whose harmonic comb (h1–20) holds the most energy.
    let est = (f0 * l as f32 / rate).round() as usize;
    let comb = |q: usize| (1..=20).map(|h| h * q).take_while(|&b| b < mo.len()).map(|b| mo[b] * mo[b]).sum::<f32>();
    let per = (est.saturating_sub(2).max(1)..=est + 2).max_by(|a, b| comb(*a).total_cmp(&comb(*b))).unwrap_or(est);
    if per < 10 {
        return false;
    }
    let (mg, _) = dft(&gmgen::render_wave(i, &base)[s..s + l]);
    let nb = mo.len().min(mg.len());
    let far = |b: usize| {
        let o = b % per;
        o.min(per - o) >= per / 5
    };
    let hz = |b: usize| b as f32 * rate / l as f32;
    // Band edges as in `bands`.
    let (lo, hi) = (120.0f32, rate * 0.45);
    let edges: Vec<f32> = (0..=8).map(|j| lo * (hi / lo).powf(j as f32 / 8.0)).collect();
    let mut dens = vec![0.0f32; nb];
    let mut noise = 0.0f32;
    for j in 0..8 {
        let bins: Vec<usize> = (1..nb).filter(|&b| hz(b) >= edges[j] && hz(b) < edges[j + 1]).collect();
        let f: Vec<usize> = bins.iter().copied().filter(|&b| far(b)).collect();
        if f.is_empty() {
            continue;
        }
        let d = (f.iter().map(|&b| mo[b] * mo[b] - mg[b] * mg[b]).sum::<f32>() / f.len() as f32).max(0.0);
        for &b in &bins {
            dens[b] = d;
        }
        noise += d * bins.len() as f32;
    }
    let total: f32 = mo.iter().skip(1).map(|v| v * v).sum();
    if noise < total * 1e-4 {
        return false;
    }
    // Periodicity of the loop in a band (design D4k): FFT band-pass of the (periodic) loop, then the mean over 50 ms
    // frames of the best normalised autocorrelation at one period ±4 % (vibrato). 1 = pitched, ~0 = noise.
    let periodicity = |x: &[f32], (a, b): (f32, f32)| -> f32 {
        let lp: Vec<f32> = x[s..s + l].to_vec();
        let mut fft = FftPlanner::<f32>::new();
        let mut buf: Vec<Complex32> = lp.iter().map(|v| Complex32::new(*v, 0.0)).collect();
        fft.plan_fft_forward(l).process(&mut buf);
        for (j, c) in buf.iter_mut().enumerate() {
            let f = j.min(l - j) as f32 * rate / l as f32;
            if f < a || f >= b {
                *c = Complex32::new(0.0, 0.0);
            }
        }
        fft.plan_fft_inverse(l).process(&mut buf);
        let y: Vec<f32> = buf.iter().map(|c| c.re).collect();
        let p0 = l as f32 / per as f32;
        let (lag0, lag1) = ((p0 * 0.96) as usize, (p0 * 1.04).ceil() as usize);
        let w = (0.05 * rate) as usize;
        let at = |i: usize| y[i % l];
        let mut vals = Vec::new();
        let mut t0 = 0;
        while t0 + w <= l {
            let mut best = 0.0f32;
            for lag in lag0..=lag1 {
                let (mut xy, mut xx, mut yy) = (0.0f32, 0.0f32, 0.0f32);
                for i in t0..t0 + w {
                    let (u, v) = (at(i), at(i + lag));
                    xy += u * v;
                    xx += u * u;
                    yy += v * v;
                }
                best = best.max(xy / (xx * yy).sqrt().max(1e-20));
            }
            vals.push(best);
            t0 += w;
        }
        vals.iter().sum::<f32>() / vals.len().max(1) as f32
    };
    let base_render = gmgen::render_wave(i, &base);
    let build = |lp: Option<f32>| -> Option<Params> {
        let mut nz = Layer::new(family("noise").unwrap(), Region::Sustain);
        // dens: half spectrum, bin b at b·rate/l. Bands far below the strongest add nothing audible.
        nz.tables[0] = bands(&dens, rate).chunks(3).filter(|r| r[2] > -40.0).flatten().copied().collect();
        nz.set("a", base.layers[k].get("a"));
        nz.set("level_db", -20.0);
        nz.set("hp", 0.7 * per as f32 * rate / l as f32);
        if let Some(v) = lp {
            nz.set("lp", v);
        }
        let mut q = base.clone();
        q.layers.push(nz);
        let n = q.layers.len() - 1;
        // Budget: weakest sideband components first, then the harmonic tail.
        while q.numbers() > MAX_NUMBERS {
            let side = &mut q.layers[k].tables[4];
            let weakest = side.chunks(4).enumerate().filter(|(_, r)| r[2] <= -20.0).min_by(|a, b| a.1[2].total_cmp(&b.1[2])).map(|(j, _)| j);
            if let Some(j) = weakest {
                side.drain(4 * j..4 * j + 4);
            } else if q.layers[k].tables[0].len() > 16 {
                q.layers[k].tables[0].pop();
            } else {
                return None;
            }
        }
        calibrate(i, &mut q, n, (noise / 2.0).sqrt(), s..s + l);
        let ranges: Vec<(f32, f32)> = q.layers[n].tables[0]
            .chunks(3)
            .map(|r| (r[0] / (hi / lo).powf(1.0 / 16.0), r[0] * (hi / lo).powf(1.0 / 16.0)))
            .collect();
        // Noise share per band by periodicity, not by mid-gap energy: a section's mid-gap energy is partly pitched
        // smear (405 at 3–6 kHz: periodicity 0.80, the energy-matched noise gave 0.56 — audible hiss). Wanted
        // aperiodic share = (1 − p_orig) − (1 − p_model without noise), current = (1 − p_model) − (1 − p_without).
        let (po, pn): (Vec<f32>, Vec<f32>) = ranges.iter().map(|&r| (periodicity(orig, r), periodicity(&base_render, r))).unzip();
        for _ in 0..3 {
            let g = gmgen::render_wave(i, &q);
            let (mq, _) = dft(&g[s..s + l]);
            for (j, &r) in ranges.iter().enumerate() {
                let want = pn[j] - po[j];
                let cur = pn[j] - periodicity(&g, r);
                let gain = &mut q.layers[n].tables[0][3 * j + 2];
                *gain = if want <= 0.005 {
                    -60.0
                } else {
                    (*gain + (10.0 * (want / cur.max(0.005)).log10()).clamp(-12.0, 12.0)).clamp(-60.0, 24.0)
                };
            }
            for (j, h) in q.layers[k].tables[0].iter_mut().enumerate() {
                let c = (j + 1) * per;
                if c + per / 2 >= nb {
                    break;
                }
                let r = c - per / 2..=c + per / 2;
                let fb: Vec<usize> = r.clone().filter(|&b| far(b)).collect();
                let dq = fb.iter().map(|&b| mq[b] * mq[b]).sum::<f32>() / fb.len().max(1) as f32;
                let (eo, eq) = r.fold((0.0f32, 0.0f32), |a, b| (a.0 + mo[b] * mo[b], a.1 + mq[b] * mq[b]));
                let nq = dq * (per + 1) as f32;
                let line = eq - nq;
                if line > 1e-20 {
                    *h += (10.0 * ((eo - nq).max(0.01 * eo) / line).log10()).clamp(-40.0, 3.0);
                }
            }
        }
        q.validate().is_ok().then_some(q)
    };
    let before = crate::fit::score_of(orig, i, p);
    let mut best: Option<(f32, Params)> = None;
    for lp in [Some(0.18 * rate), Some(0.23 * rate), Some(0.27 * rate), Some(0.32 * rate), None] {
        if let Some(q) = build(lp) {
            let sc = crate::fit::score_of(orig, i, &q);
            if std::env::var_os("GMFIT_TRACE").is_some() {
                eprintln!("gap_noise lp {lp:?}: {sc}");
            }
            if best.as_ref().is_none_or(|b| sc > b.0) {
                best = Some((sc, q));
            }
        }
    }
    // GMFIT_DUMP=<file>: write the best candidate even when it is not kept (listening comparisons).
    if let (Ok(path), Some((_, q))) = (std::env::var("GMFIT_DUMP"), best.as_ref()) {
        let wf = gmgen::params_json::WaveFile { wave: i as u32, params: q.clone(), placeholder: false, note: String::new() };
        let _ = std::fs::write(path, gmgen::params_json::to_json(&wf));
    }
    match best {
        Some((sc, q)) if sc + 0.005 >= before => {
            *p = q;
            true
        }
        _ => false,
    }
}

/// Noise loop (design D4n): a loop that is a noise wash (cymbal, brush: fine contrast ≤ `desc::WASH_DB`) but
/// modelled by an additive comb (copy ≥ `LINES_DB` peakier, flag `lines`) gets a loop-length `noise` sustain that
/// fills every loop bin the way the original does. A real cymbal loop sits between noise and comb (dense ringing
/// partials), so the comb is kept at 0 / −6 / −12 / −18 dB, dropped, or cut to its prominent partials (≥ 4 / 6 dB over
/// the median of ±4 neighbours; the metallic ring — a uniformly lowered comb loses it), with or without `spread`;
/// the noise fills the rest: bands from
/// the loop's power spectrum, then 3 passes moving each band by the original/model band energy ratio and setting
/// the noise level so comb + noise = the loop's energy. Returns the candidates (the caller keeps the best by
/// rank, `crate::rank`); empty when the wave does not qualify.
pub fn noise_loop(i: usize, orig: &[f32], m: &WaveMeta, p: &Params) -> Vec<Params> {
    let Some((s, l)) = loop_of(m) else { return vec![] };
    let is = |x: &Layer, f: &str| FAMILIES[x.family].name == f && x.region == Region::Sustain;
    let Some(k) = p.layers.iter().position(|x| is(x, "additive")) else { return vec![] };
    if l < 512 {
        return vec![];
    }
    let rate = m.rate as f32;
    let tile = |x: &[f32]| -> Vec<f32> { x[s..s + l].iter().cycle().take(l.max(3 * 8192)).copied().collect() };
    let lo = tile(orig);
    let (c_orig, c_diff) = crate::desc::contrast_pair(&lo, &tile(&gmgen::render_wave(i, p)), rate);
    if c_orig > crate::desc::WASH_DB || c_diff < crate::score::LINES_DB {
        return vec![];
    }
    let (mo, _) = dft(&orig[s..s + l]);
    let po: Vec<f32> = mo.iter().map(|v| v * v).collect();
    // Third octaves: 8 bands cannot follow a cymbal's resonances (the comb's cluster levels can, and win).
    const NB: usize = 20;
    let (blo, bhi) = (120.0f32, rate * 0.45);
    let edges: Vec<f32> = (0..=NB).map(|j| blo * (bhi / blo).powf(j as f32 / NB as f32)).collect();
    let hz = |b: usize| b as f32 * rate / l as f32;
    let band_e = |x: &[f32]| -> Vec<f32> {
        (0..NB).map(|j| x.iter().enumerate().filter(|(b, _)| hz(*b) >= edges[j] && hz(*b) < edges[j + 1]).map(|(_, v)| v).sum::<f32>()).collect()
    };
    let eo = band_e(&po);
    // Loop energy (Σ a² over bins; `dft` gives amplitudes) of a layer rendered alone.
    let solo_e = |q: &Params, n: usize| -> f32 {
        let mut solo = q.clone();
        solo.layers = vec![q.layers[n].clone()];
        let (ms, _) = dft(&gmgen::render_wave(i, &solo)[s..s + l]);
        ms.iter().skip(1).map(|v| v * v).sum()
    };
    let total_o: f32 = po.iter().skip(1).sum();
    let mut out = Vec::new();
    enum Keep {
        Drop,
        Gain(f32),
        Peaks(f32, bool),
    }
    let keeps = [
        Keep::Drop, Keep::Gain(0.0), Keep::Gain(-6.0), Keep::Gain(-12.0), Keep::Gain(-18.0),
        Keep::Peaks(4.0, true), Keep::Peaks(6.0, true), Keep::Peaks(4.0, false), Keep::Peaks(6.0, false),
    ];
    for keep in keeps {
        let mut q = p.clone();
        let a = q.layers[k].get("a");
        let dly = q.layers[k].try_get("dly").unwrap_or(0.0);
        q.layers.retain(|x| !is(x, "noise"));
        let mut kk = q.layers.iter().position(|x| is(x, "additive")).unwrap();
        match keep {
            Keep::Drop => {
                q.layers.remove(kk);
                kk = usize::MAX;
            }
            Keep::Gain(g) => {
                let v = q.layers[kk].get("level_db");
                q.layers[kk].set("level_db", (v + g).clamp(-80.0, 24.0));
            }
            Keep::Peaks(thr, keep_spread) => {
                let h = q.layers[kk].tables[0].clone();
                if h.len() < 9 {
                    continue;
                }
                let prominent = |j: usize| {
                    let mut nb: Vec<f32> = h[j.saturating_sub(4)..(j + 5).min(h.len())].to_vec();
                    nb.sort_by(f32::total_cmp);
                    h[j] - nb[nb.len() / 2] >= thr
                };
                // −99 dB, not lower: the additive renderer stops at the first harmonic below −100 dB.
                let cut: Vec<f32> = (0..h.len()).map(|j| if prominent(j) { h[j] } else { -99.0 }).collect();
                if cut.iter().all(|v| *v <= -99.0) {
                    continue;
                }
                q.layers[kk].tables[0] = cut;
                // Past the table the tail would continue the last row: keep it silent.
                q.layers[kk].set("tail", -48.0);
                if !keep_spread {
                    q.layers[kk].set("spread", 0.0);
                }
            }
        }
        if q.layers.len() >= gmgen::schema::MAX_LAYERS {
            continue;
        }
        let comb_e = if kk == usize::MAX { 0.0 } else { solo_e(&q, kk) };
        if comb_e >= 0.9 * total_o {
            continue;
        }
        let mut nz = Layer::new(family("noise").unwrap(), Region::Sustain);
        nz.tables[0] = bands_n(&po, rate, NB, 4.3);
        nz.set("a", a);
        if dly > 0.0 {
            nz.set("dly", dly);
        }
        // Band skirts leak where the original is silent: high-pass at the first band within 40 dB of the
        // strongest, low-pass at the top edge (a hat loop rolls off sharply near 0.45·rate).
        let top_e = eo.iter().copied().fold(0.0f32, f32::max);
        let first = eo.iter().position(|&e| e > top_e * 1e-4).unwrap_or(0);
        nz.set("hp", edges[first]);
        nz.set("lp", edges[NB]);
        nz.set("level_db", -12.0);
        q.layers.push(nz);
        let n = q.layers.len() - 1;
        let set_level = |q: &mut Params| {
            let en = solo_e(q, n);
            if en > 1e-20 {
                let v = q.layers[n].get("level_db");
                q.layers[n].set("level_db", (v + 10.0 * ((total_o - comb_e).max(1e-20) / en).log10()).clamp(-80.0, 24.0));
            }
        };
        set_level(&mut q);
        for _ in 0..3 {
            let (mq, _) = dft(&gmgen::render_wave(i, &q)[s..s + l]);
            let eq = band_e(&mq.iter().map(|v| v * v).collect::<Vec<_>>());
            let t = &mut q.layers[n].tables[0];
            for j in 0..NB {
                if eo[j] > 0.0 && eq[j] > 0.0 {
                    t[3 * j + 2] = (t[3 * j + 2] + (10.0 * (eo[j] / eq[j]).log10()).clamp(-12.0, 12.0)).clamp(-60.0, 24.0);
                }
            }
            // Shape only (level is `level_db`): strongest band at 0 dB.
            let top = (0..NB).map(|j| t[3 * j + 2]).fold(f32::MIN, f32::max);
            for j in 0..NB {
                t[3 * j + 2] = (t[3 * j + 2] - top).max(-60.0);
            }
            set_level(&mut q);
        }
        if q.validate().is_ok() {
            out.push(q);
        }
    }
    out
}

/// Keeps the best-ranked of `p` and its `noise_loop` candidates (rank: score − 0.05 per character flag), each
/// ranked after its attack tables (`tf`, `env`) are refitted — `p` usually has tuned tables, a fresh candidate not.
/// Returns whether `p` changed.
pub fn pick_noise_loop(i: usize, orig: &[f32], m: &WaveMeta, p: &mut Params) -> bool {
    let cands = noise_loop(i, orig, m, p);
    if cands.is_empty() {
        return false;
    }
    let mut best = (crate::rank_of(orig, i, p), None);
    for (j, q) in cands.into_iter().enumerate() {
        let (q, _) = crate::fit::correct_tf(orig, i, &q);
        let (q, _) = crate::fit::correct_env(orig, i, &q);
        let r = crate::rank_of(orig, i, &q);
        if std::env::var_os("GMFIT_TRACE").is_some() {
            eprintln!("noise_loop cand {j}: rank {r} (current {})", best.0);
        }
        if let Ok(path) = std::env::var("GMFIT_DUMP") {
            let wf = WaveFile { wave: i as u32, params: q.clone(), placeholder: false, note: String::new() };
            let _ = std::fs::write(format!("{path}.{j}"), gmgen::params_json::to_json(&wf));
        }
        if r > best.0 {
            best = (r, Some(q));
        }
    }
    match best.1 {
        Some(q) => {
            *p = q;
            true
        }
        None => false,
    }
}

/// Ensemble/vibrato sustain (an additive layer with `vib_rate`): untabled harmonics get seeded random phases —
/// Schroeder phases on a steady upper spectrum buzz. Picks the seed (0 = default phases, 1–16) whose
/// per-band [`crate::score::buzz_band`] (1.5–2.5, 2.5–4, 4–7, 7–10 kHz) is closest to the original's.
pub fn pick_phase_seed(i: usize, orig: &[f32], m: &WaveMeta, p: &mut Params) {
    let Some(k) = p.layers.iter().position(|l| FAMILIES[l.family].name == "additive" && l.try_get("vib_rate").unwrap_or(0.0) > 0.0) else {
        return;
    };
    let Some((ls, _)) = loop_of(m) else { return };
    let Some(f) = pitch(orig, ls, m.rate as f32) else { return };
    let rate = m.rate as f32;
    let per = (rate / f).round() as usize;
    let played = |x: &[f32]| crate::audio::played(x, m, 0.5);
    let skip = (0.1 * rate) as usize;
    let bands = [(1500.0, Some(2500.0)), (2500.0, Some(4000.0)), (4000.0, Some(7000.0)), (7000.0, Some(10000.0f32.min(0.45 * rate)))];
    let b = |x: &[f32]| -> Vec<f32> {
        let y = played(x);
        let y = &y[skip.min(y.len())..];
        bands.iter().map(|&(lo, hi)| crate::score::buzz_band(y, rate, per, lo, hi)).collect()
    };
    let target = b(orig);
    let mut best = (f32::MAX, 0.0);
    for seed in 0..=16 {
        let mut q = p.clone();
        q.layers[k].set("phase_seed", seed as f32);
        let bg = b(&gmgen::render_wave(i, &q));
        let d: f32 = bg.iter().zip(&target).map(|(g, o)| (g - o).abs()).sum();
        if std::env::var_os("GMFIT_TRACE").is_some() {
            eprintln!("phase_seed {seed}: {bg:.2?} vs {target:.2?} → {d:.2}");
        }
        if d < best.0 - 0.05 {
            best = (d, seed as f32);
        }
    }
    p.layers[k].set("phase_seed", best.1);
}

/// Initial parameters for wave `i`.
pub fn analyze(i: usize, orig: &[f32], m: &WaveMeta) -> WaveFile {
    let prior = prior_for_wave(i);
    let fam = priors::family(&prior);
    let rate = m.rate as f32;
    let mut p = Params::default();
    let unity_hz = 440.0 * 2f32.powf((m.unity as f32 - 69.0) / 12.0);
    match loop_of(m) {
        Some((s, l)) => {
            let lp = &orig[s..s + l];
            let est = pitch(&crate::audio::played(orig, m, 0.3), s, rate).unwrap_or(unity_hz);
            // A loop with vibrato: its mean pitch sets the periods per loop (autocorrelation misses it).
            let vib = loop_vibrato(lp, rate, est);
            let est = vib.map_or(est, |v| v.0);
            let per = ((l as f32 * est / rate).round() as usize).max(1);
            let f0 = per as f32 * rate / l as f32;
            p.wave[1] = (69.0 + 12.0 * (f0 / 440.0).log2()).clamp(0.0, 139.0);
            p.wave_explicit[1] = true;
            // Attack layer from the prior for percussive kinds.
            let attack_family = match prior.kind {
                Kind::Drum | Kind::Snare => Some("drum"),
                Kind::Metal(_) => Some("metal"),
                Kind::Shaker | Kind::Noise => Some("noise"),
                _ => None,
            };
            if let Some(af) = attack_family.filter(|_| s > (0.005 * rate) as usize) {
                let mut a = attack_layer(af, &prior, orig, rate, Region::Attack, s);
                a.set("level_db", -12.0);
                p.layers.push(a);
            }
            // Sustain: harmonic table from the loop.
            let (mag, phs) = dft(lp);
            let mut sus = Layer::new(family("additive").unwrap(), Region::Sustain);
            let budget = MAX_NUMBERS - 40;
            let kmax = ((l / 2) / per).min(240).min(budget);
            let mut harm = Vec::new();
            let mut bs = Vec::new();
            let w = if per >= 3 { per / 2 } else { 0 };
            let cluster = |c: usize| c.saturating_sub(w)..=(c + w).min(l / 2);
            // Detuned voices: two strong peaks around the low harmonics → unison 2 with their distance in cents.
            let mut det = Vec::new();
            for k in 1..=kmax.min(4) {
                let c = k * per;
                let mut peaks: Vec<(usize, f32)> = cluster(c)
                    .filter(|&b| b > 0 && b < l / 2 && mag[b] >= mag[b - 1] && mag[b] >= mag[b + 1])
                    .map(|b| (b, mag[b]))
                    .collect();
                peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
                if peaks.len() >= 2 && peaks[1].1 > 0.5 * peaks[0].1 && peaks[0].1 > 1e-3 {
                    let (b1, b2) = (peaks[0].0.min(peaks[1].0) as f32, peaks[0].0.max(peaks[1].0) as f32);
                    det.push(1200.0 * (b2 / b1).log2());
                }
            }
            // Stiff strings: centre each cluster on the stretched partial (generator stretch
            // √((1 + B·k²)/(1 + B)), B re-estimated as partials are found), or high partials leave their cluster.
            let mut b_run = 0.0f32;
            let mut centres = Vec::new();
            for k in 1..=kmax {
                let kf = k as f32;
                let c = if prior.inharm > 0.0 {
                    ((k * per) as f32 * ((1.0 + b_run * kf * kf) / (1.0 + b_run)).sqrt()).round() as usize
                } else {
                    k * per
                };
                if c + w > l / 2 {
                    break;
                }
                centres.push(c);
                // Energy of the whole cluster (unison / beating partials land beside k·per).
                let e: f32 = cluster(c).map(|b| mag[b] * mag[b]).sum();
                let (bin, a) = if prior.inharm > 0.0 && per >= 3 {
                    let pk = cluster(c).map(|b| (b, mag[b])).fold((c, 0.0f32), |x, y| if y.1 > x.1 { y } else { x });
                    (pk.0, e.sqrt())
                } else {
                    (c, e.sqrt())
                };
                harm.push(20.0 * a.max(1e-6).log10());
                if prior.inharm > 0.0 && k >= 3 && a > 1e-4 {
                    let r = bin as f32 / (k * per) as f32;
                    let b = (r * r - 1.0) / (kf * kf - r * r);
                    if b > 0.0 && b < 0.02 {
                        bs.push(b);
                        let mut v = bs.clone();
                        v.sort_by(f32::total_cmp);
                        b_run = v[v.len() / 2];
                    }
                }
            }
            // Ensemble: wide clusters (median ≥ 4 peaks within 10 dB, sd ≥ 3 cents over harmonics 1–20) → `spread`
            // = median sd in cents; otherwise two strong peaks → unison 2.
            let mut sds = Vec::new();
            let mut counts = Vec::new();
            let peak_mag = mag.iter().skip(1).copied().fold(0.0f32, f32::max);
            for k in 1..=kmax.min(20) {
                let c = k * per;
                let (lo, hi) = (c.saturating_sub(w).max(1), (c + w).min(l / 2));
                let p: Vec<(f32, f32)> = (lo..=hi).map(|b| (b as f32, mag[b] * mag[b])).collect();
                let tot: f32 = p.iter().map(|x| x.1).sum();
                if per < 8 || tot.sqrt() < peak_mag * 1e-2 {
                    continue;
                }
                let mu = p.iter().map(|x| x.0 * x.1).sum::<f32>() / tot;
                let sd = (p.iter().map(|x| (x.0 - mu).powi(2) * x.1).sum::<f32>() / tot).sqrt();
                sds.push(1200.0 * ((mu + sd) / mu).log2());
                let top = p.iter().map(|x| x.1).fold(0.0f32, f32::max);
                counts.push(p.iter().filter(|x| x.1 > top * 0.1).count());
            }
            sds.sort_by(f32::total_cmp);
            counts.sort();
            let spread = (vib.is_none() && counts.len() >= 3 && counts[counts.len() / 2] >= 4 && sds[sds.len() / 2] >= 3.0).then(|| sds[sds.len() / 2]);
            if let Some(sp) = spread {
                sus.set("spread", sp.clamp(0.0, 50.0));
            } else if det.len() >= 2 && vib.is_none() {
                // (Vibrato sidebands also look like twin peaks; a loop with vibrato is one voice.)
                det.sort_by(f32::total_cmp);
                sus.set("unison", 2.0);
                sus.set("detune", det[det.len() / 2].clamp(0.0, 50.0));
            }
            while harm.len() > 1 && *harm.last().unwrap() < -90.0 {
                harm.pop();
            }
            if !bs.is_empty() {
                bs.sort_by(f32::total_cmp);
                sus.set("inharm", bs[bs.len() / 2]);
            }
            if let Some((_, n, depth, theta)) = vib {
                // Generator deviation ∝ cos(2π(vib_phase + n·i/L)) from sample 0; measured cos(2π·n·(i−s)/L − θ).
                let ph = -theta / std::f32::consts::TAU - (n * s % l) as f32 / l as f32;
                sus.set("vib_rate", n as f32 * rate / l as f32);
                sus.set("vib_cents", depth.clamp(0.0, 100.0));
                sus.set("vib_phase", ph - ph.floor());
                // Per-harmonic loudness modulation and the pitch curve's Fourier series, from each harmonic's
                // complex envelope in the loop (budget permitting: 4 numbers per harmonic + 12).
                let rows = harm.len().min(12).min((budget.saturating_sub(harm.len() + 40 + 12)) / 8);
                let mm = loop_modulation(lp, rate, &centres, w, n, rows);
                if rows >= 4 {
                    let shift = |p: f32, j: usize| {
                        let v = p - (j * n * s % l) as f32 / l as f32;
                        v - v.floor()
                    };
                    let mut am = Vec::new();
                    for (k, (cs, excess)) in mm.am.iter().enumerate() {
                        for (j, c) in cs.iter().enumerate() {
                            am.extend_from_slice(&[c.0.min(24.0), shift(c.1, j + 1)]);
                        }
                        // The table level is the unmodulated level: remove the modulation's power excess.
                        harm[k] -= excess;
                    }
                    // Sideband components (exact AM/FM of each harmonic): for harmonics 1–12 while the budget allows,
                    // the strongest loop bins of each cluster until 99.5 % of its energy (≤ 11), scaled to the
                    // cluster energy; they replace the `am` rows. (The small components shape the loudness dips:
                    // 97 % / ≤ 7 matched the spectrum but not the envelope.)
                    let mut side = Vec::new();
                    // Room: harmonic table, vib rows, the `am` rows kept for the harmonics past the sidebands, and
                    // ~60 numbers for scalars and correction tables.
                    let room = MAX_NUMBERS.saturating_sub(harm.len() + 12 + 60 + am.len());
                    for (k, &c) in centres.iter().enumerate().take(12) {
                        if side.len() + 4 * 11 > room {
                            break;
                        }
                        let lo = c.saturating_sub(w).max(1);
                        let bins: Vec<usize> = (lo..=(c + w).min(l / 2)).collect();
                        let e: f32 = bins.iter().map(|b| mag[*b] * mag[*b]).sum();
                        if e <= 0.0 {
                            continue;
                        }
                        let mut by: Vec<usize> = bins.clone();
                        by.sort_by(|a, b| mag[*b].total_cmp(&mag[*a]));
                        let mut got = 0.0;
                        let first = side.len();
                        for &b in by.iter().take(11) {
                            let ph = (phs[b] + std::f32::consts::FRAC_PI_2) / std::f32::consts::TAU - ((b * s) % l) as f32 / l as f32;
                            side.extend_from_slice(&[(k + 1) as f32, b as f32 - c as f32, 20.0 * (mag[b] / e.sqrt()).max(1e-6).log10(), ph - ph.floor()]);
                            got += mag[b] * mag[b];
                            if got >= 0.995 * e {
                                break;
                            }
                        }
                        // Scale the kept components to the whole cluster's energy.
                        let g = 10.0 * (e / got.max(1e-20)).log10();
                        side[first..].chunks_mut(4).for_each(|r| r[2] += g);
                    }
                    if harm.len() + side.len() + 12 + 60 <= MAX_NUMBERS {
                        // Sideband harmonics keep their full cluster level (no AM excess to remove).
                        let n_side = side.chunks(4).map(|r| r[0] as usize).max().unwrap_or(0);
                        for (k, (_, excess)) in mm.am.iter().enumerate().take(n_side) {
                            harm[k] += excess;
                        }
                        // Harmonics past the sideband rows keep their `am` rows (rows of the sideband harmonics are
                        // ignored by the generator, so only add rows when some harmonic needs them).
                        if mm.am.len() > n_side {
                            sus.tables[2] = am;
                        }
                        sus.tables[4] = side;
                    } else {
                        sus.tables[2] = am;
                    }
                    sus.tables[3] = mm.vib.iter().enumerate().flat_map(|(j, c)| [c.0.min(100.0), shift(c.1, j + 1)]).collect();
                } else {
                    let (am, th) = loop_am(lp, rate, n);
                    if am > 0.3 {
                        let ph = -th / std::f32::consts::TAU - (n * s % l) as f32 / l as f32;
                        sus.set("am_db", am.min(12.0));
                        sus.set("am_phase", ph - ph.floor());
                    }
                }
            } else if prior.vib_cents > 0.0 {
                sus.set("vib_rate", prior.vib_rate);
                sus.set("vib_cents", 0.0);
            }
            // Phases of the first 32 harmonics, in the generator's convention: partial k is
            // sin(2π(n_k·t/L + ph)), and the loop DFT phase φ refers to cos at t = s, so
            // ph = (φ + π/2)/2π − n_k·s/L (mod 1), with n_k = k·per.
            if prior.inharm == 0.0 && spread.is_none() && vib.is_none() && harm.len() + 32 <= budget {
                sus.tables[1] = (1..=harm.len().min(32))
                    .map(|k| {
                        let ph = (phs[(k * per).min(l / 2)] + std::f32::consts::FRAC_PI_2) / std::f32::consts::TAU
                            - ((k * per * s) % l) as f32 / l as f32;
                        ph - ph.floor()
                    })
                    .collect();
            }
            sus.tables[0] = harm;
            sus.set("tail", -48.0);
            sus.set("level_db", 0.0);
            let loop_rms = rms(lp);
            envelope(orig, rate, s, &mut sus, loop_rms);
            p.layers.push(sus);
            // Level: the sustain layer alone must give the loop's harmonic RMS (the envelope is frozen there).
            let harm_e: f32 = centres.iter().flat_map(|c| cluster(*c)).map(|b| mag[b] * mag[b]).sum();
            let harm_rms = (harm_e / 2.0).sqrt();
            let k = p.layers.len() - 1;
            calibrate(i, &mut p, k, harm_rms, s..s + l);
            // Residual between harmonics → periodic noise.
            let total_e: f32 = mag.iter().skip(1).map(|v| v * v).sum();
            let resid = (total_e - harm_e).max(0.0);
            if per >= 4 && resid > harm_e * 10f32.powf(-2.5) {
                let mut pw: Vec<f32> = mag.iter().map(|v| v * v).collect();
                for c in &centres {
                    for b in cluster(*c) {
                        pw[b] = 0.0;
                    }
                }
                let mut nz = Layer::new(family("noise").unwrap(), Region::Sustain);
                nz.tables[0] = bands(&pw, rate);
                let a = p.layers.last().unwrap().get("a");
                nz.set("a", a);
                nz.set("level_db", -20.0);
                p.layers.push(nz);
                let k = p.layers.len() - 1;
                calibrate(i, &mut p, k, (resid / 2.0).sqrt(), s..s + l);
            }
            // Struck / plucked strings: a free-running stiff-string attack layer (stretched partials, faster decay
            // of the upper ones) hands over to the exactly periodic sustain layer at the loop.
            let stringy = matches!(prior.kind, Kind::Plucked) || (matches!(prior.kind, Kind::Struck(_)) && prior.inharm > 0.0);
            if stringy
                && s > (0.05 * rate) as usize
                && p.layers.iter().all(|x| x.region != Region::Attack)
                && let Some(mut a) = string_attack(orig, rate, s, f0)
            {
                let k = p.layers.iter().position(|x| x.region == Region::Sustain && gmgen::schema::FAMILIES[x.family].name == "additive").unwrap();
                // Hand-over at the loop: the sustain layer fades in over the same last 10 ms in which the string
                // layer fades out (render.rs), so stretched and grid partials never overlap.
                let st = s as f32 / rate;
                p.layers[k].set("dly", (st - 0.01).max(0.0));
                p.layers[k].set("a", 0.01f32.min(st));
                p.layers[k].set("h", 0.0);
                p.layers[k].set("s", 1.0);
                a.set("level_db", -12.0);
                p.layers.insert(0, a);
                let k = p.layers.iter().position(|x| x.region == Region::Sustain && gmgen::schema::FAMILIES[x.family].name == "additive").unwrap();
                calibrate(i, &mut p, k, harm_rms, s..s + l);
                let head = (0.003 * rate) as usize..(s / 3).max((0.003 * rate) as usize + 1);
                calibrate(i, &mut p, 0, rms(&orig[head.clone()]), head);
                // Budget: shrink the sustain table first, keeping room for the correction tables.
                let k = p.layers.iter().position(|x| x.region == Region::Sustain && gmgen::schema::FAMILIES[x.family].name == "additive").unwrap();
                while p.numbers() > MAX_NUMBERS - 24 && p.layers[k].tables[0].len() > 8 {
                    p.layers[k].tables[0].pop();
                    let n = p.layers[k].tables[0].len();
                    p.layers[k].tables[1].truncate(n);
                }
            }
            // (A string attack layer carries the whole attack alone and was calibrated above.)
            let string_layer = p.layers.iter().any(|x| x.try_get("dly").unwrap_or(0.0) > 0.0);
            if let Some(k) = p.layers.iter().position(|x| x.region == Region::Attack).filter(|_| !string_layer) {
                // Attack layer covers what the sustain layer misses in the first part.
                let head = (0..s.min((0.03 * rate) as usize).max(1)).collect::<Vec<_>>();
                let target = rms(&orig[head[0]..*head.last().unwrap() + 1]) * 0.5;
                calibrate(i, &mut p, k, target, 0..head.len());
            }
        }
        None => {
            let f = if fam == "additive" { "noise" } else { fam };
            let region = Region::Whole;
            let mut l = attack_layer(f, &prior, orig, rate, region, orig.len());
            l.set("level_db", -12.0);
            p.layers.push(l);
            let peak_win = (0.05 * rate) as usize;
            let target = rms(&orig[..peak_win.min(orig.len())]);
            calibrate(i, &mut p, 0, target, 0..peak_win.min(orig.len()));
            if matches!(prior.kind, Kind::Metal(_)) {
                // Sampled cymbals/hats are broadband: a noise layer shaped by the measured spectrum beside the
                // metallic oscillators.
                let mut nz = attack_layer("noise", &prior, orig, rate, region, orig.len());
                nz.set("level_db", -12.0);
                p.layers.push(nz);
                calibrate(i, &mut p, 1, target * 0.7, 0..peak_win.min(orig.len()));
            }
            if matches!(prior.kind, Kind::Snare | Kind::Drum) && FAMILIES[p.layers[0].family].name == "drum" {
                // Snare wires / kick beater and shell texture: broadband noise.
                let mut nz = attack_layer("noise", &prior, orig, rate, region, orig.len());
                // The bands below twice the body frequency belong to the drum layer.
                let body = p.layers[0].get("freq");
                for b in nz.tables[0].chunks_mut(3) {
                    if b[0] < 2.0 * body {
                        b[2] = -60.0;
                    }
                }
                nz.set("level_db", -12.0);
                p.layers.push(nz);
                // Level: the original's share of energy above 2× body.
                let seg = &orig[..peak_win.min(orig.len())];
                let pw = avg_power(seg);
                let cut = ((2.0 * body / rate * 2048.0) as usize).min(pw.len() - 1);
                let frac = pw[cut..].iter().sum::<f32>() / pw.iter().sum::<f32>().max(1e-20);
                calibrate(i, &mut p, 1, target * frac.sqrt().max(1e-3), 0..seg.len());
            }
        }
    }
    for l in &mut p.layers {
        open_for_fit(l);
    }
    gap_noise(i, orig, m, &mut p);
    pick_phase_seed(i, orig, m, &mut p);
    pick_noise_loop(i, orig, m, &mut p);
    let _ = p.validate();
    let mut note = format!("analyze: prior {:?}", prior.kind);
    // Model choice by score (design D4l): every wave also tries `modal` (struck, rattle, chirp, sparse loop).
    let tonal = crate::modal::tonal_share(&orig[..orig.len().min((0.3 * rate) as usize)], rate);
    {
        let base = crate::fit::score_of(orig, i, &p);
        let best = crate::modal::candidates(i, orig, m)
            .into_iter()
            .enumerate()
            .map(|(c, mut q)| {
                for l in &mut q.layers {
                    open_for_fit(l);
                }
                if let Ok(dir) = std::env::var("GMFIT_MODAL_DUMP") {
                    let wf = WaveFile { wave: i as u32, params: q.clone(), placeholder: false, note: String::new() };
                    let _ = std::fs::write(format!("{dir}/modal-{i}-{c}.json"), gmgen::params_json::to_json(&wf));
                }
                let sc = crate::fit::score_of(orig, i, &q);
                (sc, q)
            })
            .max_by(|a, b| a.0.total_cmp(&b.0));
        if std::env::var_os("GMFIT_TRACE").is_some() {
            eprintln!("modal: tonal {tonal:.2}, prior model {base:.4}, best modal {:?}", best.as_ref().map(|b| b.0));
        }
        if let Some((sc, q)) = best.filter(|b| b.0 > base) {
            p = q;
            note = format!("analyze: modal (tonal {tonal:.2}, {sc:.3} vs prior {base:.3})");
        }
    }
    WaveFile { wave: i as u32, params: p, placeholder: false, note: note.chars().take(80).collect() }
}

/// Loop vibrato: pitch track of harmonic 1 over the loop (3 tiles, FFT band-pass 0.7–1.3·`est`, interpolated
/// upward zero crossings of the middle tile), then the best sinusoid at a whole number `n` of cycles per loop
/// (≤ 9 Hz). Returns `(mean Hz, n, peak deviation cents, θ)` with deviation ≈ depth·cos(2π·n·t/L − θ), t from the
/// loop start; `None` without a clear (≥ 4 cents, ≥ 30 % of the variance) periodic deviation.
fn loop_vibrato(lp: &[f32], rate: f32, est: f32) -> Option<(f32, usize, f32, f32)> {
    let l = lp.len();
    let n3 = 3 * l;
    let mut buf: Vec<Complex32> = (0..n3).map(|i| Complex32::new(lp[i % l], 0.0)).collect();
    FftPlanner::<f32>::new().plan_fft_forward(n3).process(&mut buf);
    for (k, c) in buf.iter_mut().enumerate() {
        let f = k.min(n3 - k) as f32 * rate / n3 as f32;
        if f < 0.7 * est || f > 1.3 * est {
            *c = Complex32::new(0.0, 0.0);
        }
    }
    FftPlanner::<f32>::new().plan_fft_inverse(n3).process(&mut buf);
    let y: Vec<f32> = buf.iter().map(|c| c.re).collect();
    let mut cross = Vec::new();
    for i in l..2 * l + 1 {
        if y[i - 1] < 0.0 && y[i] >= 0.0 {
            cross.push(i as f32 - 1.0 + y[i - 1] / (y[i - 1] - y[i]));
        }
    }
    if cross.len() < 8 {
        return None;
    }
    let pts: Vec<(f32, f32)> = cross.windows(2).map(|w| (((w[0] + w[1]) / 2.0 - l as f32) / l as f32, rate / (w[1] - w[0]))).collect();
    let mean = pts.iter().map(|p| p.1).sum::<f32>() / pts.len() as f32;
    let dev: Vec<(f32, f32)> = pts.iter().map(|p| (p.0, 1200.0 * (p.1 / mean).log2())).collect();
    let var = dev.iter().map(|d| d.1 * d.1).sum::<f32>() / dev.len() as f32;
    let tau = std::f32::consts::TAU;
    let best = (1..=8usize)
        .filter(|n| *n as f32 * rate / l as f32 <= 9.0)
        .map(|n| {
            let (mut a, mut b) = (0.0f32, 0.0f32);
            for (t, d) in &dev {
                a += d * (tau * n as f32 * t).cos();
                b += d * (tau * n as f32 * t).sin();
            }
            let (a, b) = (2.0 * a / dev.len() as f32, 2.0 * b / dev.len() as f32);
            (n, (a * a + b * b).sqrt(), b.atan2(a))
        })
        .max_by(|x, y| x.1.total_cmp(&y.1))?;
    (best.1 >= 4.0 && best.1 * best.1 / 2.0 >= 0.3 * var).then_some((mean, best.0, best.1, best.2))
}

/// Per-harmonic and pitch modulation of a loop (see `loop_modulation`).
struct LoopMod {
    /// Per harmonic: (dB, phase) at 1×…4× base, power excess dB of that modulation.
    am: Vec<([(f32, f32); 4], f32)>,
    /// Pitch curve: (cents, phase) at 1×…6× base.
    vib: Vec<(f32, f32)>,
}

/// Complex envelope of each harmonic (loop DFT bins within ±`w` of its centre, analytic, inverse DFT): its dB
/// envelope projected on the base rate `n` cycles/loop and 2n gives the `am` rows; harmonic 1's smoothed
/// instantaneous frequency (cents from the mean) projected on 1…6 × base gives the `vib` rows. (A median over
/// harmonics 1–4 flattened it: each harmonic's own loudness dips disturb its phase differently.)
/// Phases are cycles relative to the loop start (t = 0 at `s`); the caller moves them to sample 0.
fn loop_modulation(lp: &[f32], rate: f32, centres: &[usize], w: usize, n: usize, rows: usize) -> LoopMod {
    let l = lp.len();
    let mut spec: Vec<Complex32> = lp.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    FftPlanner::<f32>::new().plan_fft_forward(l).process(&mut spec);
    let inv = FftPlanner::<f32>::new().plan_fft_inverse(l);
    let tau = std::f32::consts::TAU;
    let project = |e: &[f32], m: usize| -> (f32, f32) {
        let (mut a, mut b) = (0.0f32, 0.0f32);
        for (i, v) in e.iter().enumerate() {
            let x = tau * (m * i % l) as f32 / l as f32;
            a += v * x.cos();
            b += v * x.sin();
        }
        let (a, b) = (2.0 * a / e.len() as f32, 2.0 * b / e.len() as f32);
        // e ≈ |c|·cos(2π·m·t + arg c), c = a − ib.
        ((a * a + b * b).sqrt(), (-b).atan2(a) / tau)
    };
    let envelope = |c: usize| -> Vec<Complex32> {
        let mut z = vec![Complex32::new(0.0, 0.0); l];
        for b in c.saturating_sub(w).max(1)..=(c + w).min(l / 2) {
            z[b] = spec[b] * 2.0;
        }
        inv.process(&mut z);
        z.iter().map(|v| v / l as f32).collect()
    };
    let mut am = Vec::new();
    let mut pitch: Vec<Vec<f32>> = Vec::new();
    for (k, &c) in centres.iter().enumerate().take(rows.max(1)) {
        let z = envelope(c);
        let e: Vec<f32> = z.iter().map(|v| 20.0 * v.norm().max(1e-9).log10()).collect();
        let mean = e.iter().sum::<f32>() / l as f32;
        let d: Vec<f32> = e.iter().map(|v| v - mean).collect();
        if k < rows {
            let cs = [project(&d, n), project(&d, 2 * n), project(&d, 3 * n), project(&d, 4 * n)];
            let model = |i: usize| {
                let t = i as f32 / l as f32;
                cs.iter().enumerate().map(|(j, c)| c.0 * (tau * ((j + 1) as f32 * n as f32 * t + c.1)).cos()).sum::<f32>()
            };
            let excess = 10.0 * ((0..l).map(|i| 10f32.powf(model(i) / 10.0)).sum::<f32>() / l as f32).log10();
            am.push((cs, excess));
        }
        if k < 1 {
            // Instantaneous frequency (cents from the harmonic's mean), smoothed over 300 samples.
            let f: Vec<f32> = (0..l).map(|i| (z[(i + 1) % l] * z[i].conj()).arg() * rate / tau / (k + 1) as f32).collect();
            let m = f.iter().sum::<f32>() / l as f32;
            let h = 150usize;
            let sm: Vec<f32> = (0..l).map(|i| (0..2 * h + 1).map(|j| f[(i + l + j - h) % l]).sum::<f32>() / (2 * h + 1) as f32).collect();
            pitch.push(sm.iter().map(|v| 1200.0 * (v.max(1.0) / m.max(1.0)).log2()).collect());
        }
    }
    let med: Vec<f32> = (0..l)
        .map(|i| {
            let mut v: Vec<f32> = pitch.iter().map(|p| p[i]).collect();
            v.sort_by(f32::total_cmp);
            if v.is_empty() { 0.0 } else { (v[(v.len() - 1) / 2] + v[v.len() / 2]) / 2.0 }
        })
        .collect();
    let vib = (1..=6).map(|j| project(&med, j * n)).collect();
    LoopMod { am, vib }
}

/// Loop loudness modulation at `n` cycles per loop: RMS (dB) over 20 ms windows of the tiled loop, projected on
/// cos/sin. Returns `(depth dB, θ)` with level ≈ depth·cos(2π·n·t/L − θ), t from the loop start.
fn loop_am(lp: &[f32], rate: f32, n: usize) -> (f32, f32) {
    let l = lp.len();
    let w = (0.02 * rate) as usize;
    let tau = std::f32::consts::TAU;
    let pts: Vec<(f32, f32)> = (0..l)
        .step_by((w / 4).max(1))
        .map(|c| {
            let e = (0..w).map(|j| lp[(c + l - w / 2 + j) % l].powi(2)).sum::<f32>() / w as f32;
            (c as f32 / l as f32, 10.0 * e.max(1e-20).log10())
        })
        .collect();
    let mean = pts.iter().map(|p| p.1).sum::<f32>() / pts.len() as f32;
    let (mut a, mut b) = (0.0f32, 0.0f32);
    for (t, v) in &pts {
        a += (v - mean) * (tau * n as f32 * t).cos();
        b += (v - mean) * (tau * n as f32 * t).sin();
    }
    let (a, b) = (2.0 * a / pts.len() as f32, 2.0 * b / pts.len() as f32);
    ((a * a + b * b).sqrt(), b.atan2(a))
}

/// Stiff-string attack layer (`additive`, attack region) from the wave's first `s` samples at fundamental `f0`:
/// partial peaks of two zero-padded Hann spectra (early and late) give the amplitude table, the inharmonicity B
/// (median over partials ≥ 4 of `(r² − 1)/(k² − r²)`, r = peak / k·f0), the fundamental's decay and the extra
/// decay per partial index (`pdecay`, median of `(r_k − r_1)/(k − 1)`). `None` when the attack is too short.
fn string_attack(x: &[f32], rate: f32, s: usize, f0: f32) -> Option<Layer> {
    let n = [4096usize, 2048, 1024].into_iter().find(|n| 2 * n <= s)?;
    let zp = 8 * n;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(zp);
    let spec = |at: usize| -> Vec<f32> {
        let mut b: Vec<Complex32> = (0..zp)
            .map(|k| {
                let v = if k < n { x[at + k] * (0.5 - 0.5 * (std::f32::consts::TAU * k as f32 / n as f32).cos()) } else { 0.0 };
                Complex32::new(v, 0.0)
            })
            .collect();
        fft.process(&mut b);
        b[..zp / 2].iter().map(|c| c.norm() * 4.0 / n as f32).collect()
    };
    let (t1, t2) = (0usize, s - n);
    let (e, l8) = (spec(t1), spec(t2));
    let hz = |bin: usize| bin as f32 * rate / zp as f32;
    let bin = |f: f32| ((f * zp as f32 / rate).round() as usize).min(zp / 2 - 1);
    let mut bs: Vec<f32> = Vec::new();
    let mut harm = Vec::new();
    let mut rates = Vec::new();
    let mut b_est = 0.0f32;
    let top = e.iter().copied().fold(0.0f32, f32::max).max(1e-9);
    for k in 1..=40usize {
        let kf = k as f32;
        let pred = kf * f0 * (1.0 + b_est * kf * kf).sqrt();
        if pred + 0.4 * f0 > 0.45 * rate {
            break;
        }
        let (lo, hi) = (bin(pred - 0.4 * f0), bin(pred + 0.4 * f0));
        let pk = (lo..=hi).max_by(|a, b| e[*a].total_cmp(&e[*b]))?;
        let (a1, a2) = (e[pk], l8[(lo..=hi).max_by(|a, b| l8[*a].total_cmp(&l8[*b]))?]);
        let r = hz(pk) / (kf * f0);
        if k >= 4 && a1 > top * 1e-3 {
            let b = (r * r - 1.0) / (kf * kf - r * r);
            if b > 0.0 && b < 0.02 {
                bs.push(b);
                let mut v = bs.clone();
                v.sort_by(f32::total_cmp);
                b_est = v[v.len() / 2];
            }
        }
        let rate_k = (20.0 * (a1.max(1e-9) / a2.max(1e-9)).log10()) / (8.686 * (t2 - t1) as f32 / rate);
        rates.push((k, rate_k, a1 > top * 1e-3));
        harm.push(20.0 * a1.max(1e-7).log10());
    }
    if harm.len() < 2 {
        return None;
    }
    let r1 = rates[0].1.max(0.05);
    let mut pd: Vec<f32> = rates.iter().skip(1).filter(|r| r.2).map(|r| (r.1 - r1) / (r.0 - 1) as f32).collect();
    pd.sort_by(f32::total_cmp);
    let mut l = Layer::new(family("additive").unwrap(), Region::Attack);
    while harm.len() > 1 && *harm.last().unwrap() < 20.0 * (top * 1e-4).log10() {
        harm.pop();
    }
    l.tables[0] = harm;
    l.set("inharm", b_est.clamp(0.0, 0.02));
    l.set("pdecay", pd.get(pd.len() / 2).copied().unwrap_or(0.0).clamp(0.0, 10.0));
    l.set("a", 0.001);
    l.set("h", 0.0);
    l.set("d", (1.0 / r1).clamp(0.01, 30.0));
    l.set("s", 0.0);
    l.set("tail", -48.0);
    Some(l)
}

/// A transient layer of family `f` from the wave's onset (`until` = end of the analysed part).
pub(crate) fn attack_layer(f: &str, prior: &Prior, x: &[f32], rate: f32, region: Region, until: usize) -> Layer {
    let mut l = Layer::new(family(f).unwrap(), region);
    let seg = &x[..until.min(x.len())];
    let frame = (0.002 * rate) as usize;
    let env = rms_env(seg, frame);
    // Body peak: the loudest frame after the first 5 ms (an onset click must not set the decay).
    let skip = ((0.005 * rate) as usize / frame).min(env.len().saturating_sub(1));
    let (kp, peak) = env.iter().enumerate().skip(skip).fold((skip, f32::MIN), |b, (k, v)| if *v > b.1 { (k, *v) } else { b });
    // Exponential decay by a least-squares line through the dB envelope from the peak down to peak − 40 dB.
    let pts: Vec<(f32, f32)> = env
        .iter()
        .enumerate()
        .skip(kp)
        .take_while(|(_, v)| **v > peak - 40.0)
        .map(|(k, v)| (k as f32 * frame as f32 / rate, *v))
        .collect();
    let decay = if pts.len() >= 3 {
        let n = pts.len() as f32;
        let (mx, my) = (pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n);
        let slope = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f32>() / pts.iter().map(|p| (p.0 - mx).powi(2)).sum::<f32>().max(1e-12);
        if slope < -1e-3 { (-8.686 / slope).clamp(0.003, 5.0) } else { 2.0 }
    } else {
        0.05
    };
    match f {
        "drum" => {
            // Body pitch glide from zero crossings below 400 Hz: end = median of the last half, start = first.
            let tr = zero_cross_track(seg, rate, 400.0);
            let (late, early, tau) = if tr.len() >= 3 {
                let mut tail: Vec<f32> = tr[tr.len() / 2..].iter().map(|x| x.1).collect();
                tail.sort_by(f32::total_cmp);
                let end = tail[tail.len() / 2];
                let start = tr[0].1.max(end);
                let target = end + (start - end) / std::f32::consts::E;
                let tau = tr.iter().find(|x| x.1 <= target).map_or(0.03, |x| x.0.max(0.002));
                (end, start, tau)
            } else {
                let f = pitch(seg, 0, rate).unwrap_or(60.0);
                (f, f * 1.5, 0.03)
            };
            l.set("freq", late.clamp(20.0, 2000.0));
            l.set("ratio", (early / late).clamp(1.0, 8.0));
            l.set("sweep_t", tau.clamp(0.001, 1.0));
            l.set("decay", decay.clamp(0.005, 10.0));
            l.set("a", ((kp as f32 + 0.5) * frame as f32 / rate).clamp(0.0, 0.05));
            l.set("click_db", if prior.kind == Kind::Snare { -20.0 } else { -30.0 });
        }
        "metal" => {
            let pw = avg_power(seg);
            let nb = pw.len() - 1;
            let (num, den) = pw.iter().enumerate().fold((0.0f32, 0.0f32), |(a, b), (k, v)| (a + k as f32 * v, b + v));
            let centroid = (num / den.max(1e-20)) * rate / (2.0 * nb as f32);
            l.set("bp", centroid.clamp(500.0, 15000.0));
            l.set("hp", (centroid * 0.5).clamp(200.0, 12000.0));
            l.set("q", 1.0);
            l.set("d", decay.clamp(0.002, 10.0));
            if let Kind::Metal(f) = prior.kind {
                l.set("base", f[0]);
                l.tables[0] = f.iter().map(|x| x / f[0]).collect();
            }
        }
        _ => {
            l.tables[0] = bands(&avg_power(seg), rate);
            l.set("a", ((kp as f32 + 0.5) * frame as f32 / rate).clamp(0.0, 2.0));
            l.set("d", decay.clamp(0.001, 30.0));
            if region != Region::Sustain {
                l.set("s", 0.0);
            }
        }
    }
    l
}
