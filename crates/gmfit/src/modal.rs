//! Character analysis for struck, rattling and chirping sounds (design D4l), feeding the `modal` family:
//! - [`onsets`]: strike times and levels from a 2 ms RMS envelope (→ the `hits` table, levels re-fitted).
//! - [`partials`]: the strongest prominent spectral peaks of one hit, each with its starting level and decay
//!   (single-frequency tracker over Hann frames) — a few pure tones, not noise.
//! - [`glide`]: the strongest partial's frequency from the onset: start offset (cents) and time constant.
//! - [`sparse_loop`]: the exact loop-DFT bins holding 99.5 % of the loop energy, with phases (cymbal/belltree loops).
//! - wavelet (`crate::wavelet`): trill of the strongest partial (FM → `trill_hz`/`trill_cents`, sidebands
//!   dropped) and strikes inside its band (→ `hits`), design D4m.
//! - [`tonal_share`]: energy in the 8 strongest peaks / total (1 = a few pure tones).
//!
//! [`candidates`] turns these into whole-wave `modal` parameter sets; `analyze` keeps the best scoring model.
use gmgen::params::{Layer, Params};
use gmgen::render::WaveMeta;
use gmgen::schema::{family, Region};
use rustfft::num_complex::Complex32;

fn power_spectrum(x: &[f32], nfft: usize) -> Vec<f32> {
    let n = x.len().min(nfft);
    let mut buf: Vec<Complex32> = (0..nfft)
        .map(|k| {
            let w = if k < n { 0.5 - 0.5 * (std::f32::consts::TAU * (k as f32 + 0.5) / n as f32).cos() } else { 0.0 };
            Complex32::new(if k < n { x[k] * w } else { 0.0 }, 0.0)
        })
        .collect();
    crate::plan::forward(nfft).process(&mut buf);
    buf[..nfft / 2 + 1].iter().map(|c| c.norm_sqr()).collect()
}

/// RMS envelope in dB, `hop`-sample frames, window 2·hop.
fn env_db(x: &[f32], hop: usize) -> Vec<f32> {
    (0..x.len() / hop)
        .map(|k| {
            let s = &x[k * hop..(k * hop + 2 * hop).min(x.len())];
            10.0 * (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32 + 1e-12).log10()
        })
        .collect()
}

/// Onset times (samples) and levels (dB): rises ≥ 5 dB within 4 ms, above peak − 35 dB, ≥ 10 ms apart; the onset
/// sits at the envelope maximum of the following 6 ms.
pub fn onsets(x: &[f32], rate: f32) -> Vec<(usize, f32)> {
    let hop = ((0.002 * rate) as usize).max(1);
    let e = env_db(x, hop);
    if e.is_empty() {
        return Vec::new();
    }
    let top = e.iter().copied().fold(f32::MIN, f32::max);
    let first = e.iter().position(|v| *v > top - 35.0).unwrap_or(0);
    let mut out: Vec<(usize, f32)> = Vec::new();
    let peak_after = |k: usize| (k..(k + 4).min(e.len())).max_by(|a, b| e[*a].total_cmp(&e[*b])).unwrap();
    let k0 = peak_after(first);
    out.push((first * hop, e[k0]));
    let gap = (0.010 * rate / hop as f32) as usize;
    let mut last = first;
    for k in 2..e.len() {
        if e[k] > top - 35.0 && e[k] - e[k - 2] >= 5.0 && k >= last + gap {
            let kp = peak_after(k);
            out.push(((k - 1) * hop, e[kp]));
            last = k;
        }
    }
    out
}

/// Energy in the 8 strongest local peaks (±2 bins) / total, over `x` (200 Hz – 0.45·rate).
pub fn tonal_share(x: &[f32], rate: f32) -> f32 {
    let nfft = x.len().next_power_of_two().clamp(1024, 65536);
    let p = power_spectrum(x, nfft);
    let hz = |k: usize| k as f32 * rate / nfft as f32;
    let band: Vec<usize> = (3..p.len() - 3).filter(|&k| hz(k) > 200.0 && hz(k) < 0.45 * rate).collect();
    let tot: f32 = band.iter().map(|&k| p[k]).sum();
    let mut pk: Vec<usize> = band.iter().copied().filter(|&k| (k - 3..=k + 3).all(|j| p[j] <= p[k])).collect();
    pk.sort_by(|a, b| p[*b].total_cmp(&p[*a]));
    let w = (nfft / x.len().max(1)).max(1) * 2;
    pk.iter().take(8).map(|&k| p[k.saturating_sub(w)..(k + w + 1).min(p.len())].iter().sum::<f32>()).sum::<f32>() / tot.max(1e-20)
}

/// Amplitude of a sinusoid at `f` Hz in `x[a..a+n]` (Hann-windowed single-frequency DFT).
fn amp_at(x: &[f32], a: usize, n: usize, f: f32, rate: f32) -> f32 {
    let (mut re, mut im, mut ws) = (0.0f32, 0.0f32, 0.0f32);
    for k in 0..n.min(x.len().saturating_sub(a)) {
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * (k as f32 + 0.5) / n as f32).cos();
        let ph = std::f32::consts::TAU * f * k as f32 / rate;
        re += x[a + k] * w * ph.cos();
        im += x[a + k] * w * ph.sin();
        ws += w;
    }
    2.0 * (re * re + im * im).sqrt() / ws.max(1e-9)
}

/// Strongest prominent peaks of `x` (one hit): `[Hz, dB start amplitude, decay s, 0]`, at most `max`.
pub fn partials(x: &[f32], rate: f32, max: usize) -> Vec<[f32; 4]> {
    let len = x.len();
    if len < 64 {
        return Vec::new();
    }
    let nfft = (4 * len).next_power_of_two().clamp(4096, 65536);
    let p: Vec<f32> = power_spectrum(x, nfft).iter().map(|v| 10.0 * (v + 1e-20).log10()).collect();
    let hz = |k: f32| k * rate / nfft as f32;
    let top = p.iter().copied().fold(f32::MIN, f32::max);
    let w = ((150.0 / hz(1.0)) as usize).max(4);
    let mut pk: Vec<(f32, f32)> = Vec::new();
    for k in 3..p.len() - 3 {
        if hz(k as f32) < 40.0 || hz(k as f32) > 0.47 * rate || p[k] < top - 50.0 {
            continue;
        }
        if !(k - 3..=k + 3).all(|j| p[j] <= p[k]) {
            continue;
        }
        // Floor: 20th percentile over ±max(150 Hz, 10 % of f) — a tone's own sidebands must not raise it.
        let wk = w.max((0.1 * k as f32) as usize).max(6 * nfft / len);
        let mut win: Vec<f32> = p[k.saturating_sub(wk)..(k + wk).min(p.len())].to_vec();
        win.sort_by(f32::total_cmp);
        if p[k] - win[win.len() / 5] < 10.0 {
            continue;
        }
        // Parabolic interpolation of the peak.
        let (a, b, c) = (p[k - 1], p[k], p[k + 1]);
        let d = 0.5 * (a - c) / (a - 2.0 * b + c).min(-1e-6);
        pk.push((hz(k as f32 + d.clamp(-0.5, 0.5)), p[k]));
    }
    pk.sort_by(|a, b| b.1.total_cmp(&a.1));
    // Peaks closer than 1.5 % merge into the stronger.
    let mut keep: Vec<(f32, f32)> = Vec::new();
    for q in pk {
        if keep.iter().all(|k| (k.0 / q.0).ln().abs() > 0.015) {
            keep.push(q);
        }
        if keep.len() >= max {
            break;
        }
    }
    // Start amplitude and decay per peak: dB track on Hann frames, least squares from the track maximum down to −40 dB.
    let n = (len / 4).next_power_of_two().clamp(128, 1024).min(len);
    let hop = (n / 4).max(16);
    keep.iter()
        .map(|&(f, _)| {
            let tr: Vec<(f32, f32)> = (0..)
                .map(|j| j * hop)
                .take_while(|a| a + n <= len)
                .map(|a| ((a + n / 2) as f32 / rate, 20.0 * (amp_at(x, a, n, f, rate) + 1e-9).log10()))
                .collect();
            if tr.is_empty() {
                return [f, 20.0 * (amp_at(x, 0, len, f, rate) + 1e-9).log10(), 0.05, 0.0];
            }
            let km = (0..tr.len()).max_by(|a, b| tr[*a].1.total_cmp(&tr[*b].1)).unwrap();
            let pts: Vec<(f32, f32)> = tr[km..].iter().copied().take_while(|t| t.1 > tr[km].1 - 40.0).collect();
            let (slope, icpt) = if pts.len() >= 2 {
                let m = pts.len() as f32;
                let mx = pts.iter().map(|t| t.0).sum::<f32>() / m;
                let my = pts.iter().map(|t| t.1).sum::<f32>() / m;
                let sxy: f32 = pts.iter().map(|t| (t.0 - mx) * (t.1 - my)).sum();
                let sxx: f32 = pts.iter().map(|t| (t.0 - mx).powi(2)).sum();
                let sl = (sxy / sxx.max(1e-12)).min(-1.0);
                (sl, my - sl * mx)
            } else {
                (-200.0, tr[km].1)
            };
            // dB slope → time constant (amplitude e-folding): 8.686 dB per τ.
            [f, icpt.min(tr[km].1 + 12.0), (-8.686 / slope).clamp(0.0005, 30.0), 0.0]
        })
        .collect()
}

/// Glide of the strongest partial `f` over the first 80 ms of `x` (instantaneous frequency of the band around it):
/// `(cents at the onset, time constant s)`;
/// `None` when it moves less than 30 cents.
pub fn glide(x: &[f32], rate: f32, f: f32) -> Option<(f32, f32)> {
    // Instantaneous frequency: FFT band-pass 0.6–2·f over the first 200 ms, interpolated upward zero crossings.
    let n = x.len().min((0.2 * rate) as usize);
    if n < 64 {
        return None;
    }
    let nfft = (2 * n).next_power_of_two();
    let mut buf: Vec<Complex32> = (0..nfft).map(|k| Complex32::new(if k < n { x[k] } else { 0.0 }, 0.0)).collect();
    crate::plan::forward(nfft).process(&mut buf);
    for (k, c) in buf.iter_mut().enumerate() {
        let hz = k.min(nfft - k) as f32 * rate / nfft as f32;
        if hz < 0.6 * f || hz > 2.0 * f {
            *c = Complex32::new(0.0, 0.0);
        }
    }
    crate::plan::inverse(nfft).process(&mut buf);
    let y: Vec<f32> = buf[..n].iter().map(|c| c.re).collect();
    let mut cross: Vec<f32> = Vec::new();
    for k in 1..n {
        if y[k - 1] <= 0.0 && y[k] > 0.0 {
            cross.push((k - 1) as f32 + y[k - 1] / (y[k - 1] - y[k]));
        }
    }
    let end = 0.08 * rate;
    let tr: Vec<(f32, f32)> = cross
        .windows(2)
        .filter(|w| w[1] < end && w[0] >= rate / f)
        .map(|w| ((w[0] + w[1]) / 2.0 / rate, 1200.0 * (rate / (w[1] - w[0]) / f).log2()))
        .collect();
    // Start: the median of the first three intervals (one off interval must not make a glide).
    let first = {
        let mut v: Vec<(f32, f32)> = tr.iter().take(3).copied().collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        *v.get(v.len() / 2)?
    };
    let first = &first;
    if first.1.abs() < 30.0 {
        return None;
    }
    // A real glide settles steadily; beating between close partials or vibrato sidebands does not.
    let settle = tr.iter().position(|t| t.1.abs() < first.1.abs() / std::f32::consts::E)?;
    if settle == 0 || tr[..=settle].windows(2).any(|w| w[1].1.abs() > w[0].1.abs() + 25.0) {
        return None;
    }
    // Time constant: first frame below 1/e of the first deviation; extrapolate the deviation back to the onset.
    let tau = tr.iter().find(|t| t.1.abs() < first.1.abs() / std::f32::consts::E).map_or(0.02, |t| (t.0 - first.0).max(0.001));
    Some(((first.1 * (first.0 / tau).exp()).clamp(-4800.0, 4800.0), tau.clamp(0.0005, 2.0)))
}

/// Sparse loop: `[Hz, dB, decay, phase]` rows for the strongest exact loop bins (≤ `max`, up to 99.5 % of the
/// energy), phase in the renderer's sine convention at the loop start.
pub fn sparse_loop(lp: &[f32], rate: f32, max: usize) -> Vec<[f32; 4]> {
    let l = lp.len();
    let mut buf: Vec<Complex32> = lp.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    crate::plan::forward(l).process(&mut buf);
    // Prominent bins only (≥ 10 dB over the median of ±25 bins); the residual noise layer carries the rest.
    let pw: Vec<f32> = buf[..l / 2 + 1].iter().map(|c| c.norm_sqr()).collect();
    let prominent = |k: usize| {
        let mut w: Vec<f32> = pw[k.saturating_sub(25)..(k + 26).min(pw.len())].to_vec();
        w.sort_by(f32::total_cmp);
        pw[k] >= 10.0 * w[w.len() / 2]
    };
    let mut bins: Vec<usize> = (1..l / 2).filter(|&k| (k as f32 * rate / l as f32) < 0.49 * rate && prominent(k)).collect();
    bins.sort_by(|a, b| buf[*b].norm_sqr().total_cmp(&buf[*a].norm_sqr()));
    let tot: f32 = bins.iter().map(|&k| buf[k].norm_sqr()).sum();
    let mut acc = 0.0;
    let mut out = Vec::new();
    for &k in &bins {
        if acc >= 0.995 * tot || out.len() >= max {
            break;
        }
        acc += buf[k].norm_sqr();
        let a = 2.0 * buf[k].norm() / l as f32;
        let ph = buf[k].arg() / std::f32::consts::TAU + 0.25;
        out.push([k as f32 * rate / l as f32, 20.0 * a.max(1e-9).log10(), 1.0, ph - ph.floor()]);
    }
    out
}

fn layer(region: Region, modes: Vec<[f32; 4]>) -> Layer {
    let mut l = Layer::new(family("modal").unwrap(), region);
    l.tables[0] = modes.into_iter().flatten().collect();
    for n in ["level_db", "ha", "click_db", "click_decay", "click_hp", "click_lp"] {
        let v = l.get(n);
        l.set(n, v);
    }
    l
}

/// One-hit (or hit-train) layer from segment `x` starting at its first onset.
fn struck(x: &[f32], rate: f32, region: Region, max: usize) -> Option<Layer> {
    let on = onsets(x, rate);
    let t0 = on.first()?.0;
    // The first hit's own segment ends at the next onset.
    let one_end = on.get(1).map_or(x.len(), |o| o.0);
    let seg = &x[t0..one_end.max(t0 + 64).min(x.len())];
    let mut modes = partials(seg, rate, max);
    if modes.is_empty() {
        return None;
    }
    let mut l = layer(region, Vec::new());
    if let Some((c, tau)) = glide(seg, rate, modes[0][0]) {
        l.set("glide", c);
        l.set("glide_t", tau);
        for m in &mut modes {
            m[0] = m[0].max(1.0);
        }
    }
    // Trill (wavelet, design D4m): FM of the strongest partial. Its carrier is the mean instantaneous frequency;
    // modes at carrier ± n·rate are sidebands of the trill, not modes.
    let trilled = crate::wavelet::trill(&x[t0..], rate, modes[0][0]);
    if let Some((tr, cents, _, carrier)) = trilled {
        modes[0][0] = carrier;
        // Decay from the wavelet amplitude smoothed over a trill period (a fixed-frequency tracker loses the
        // moving partial and reads a fast decay); all modes share it.
        let d = crate::wavelet::decay(&x[t0..], rate, carrier, 1.0 / tr);
        for m in &mut modes {
            m[2] = d;
        }
        let c = carrier;
        modes = modes
            .into_iter()
            .enumerate()
            // Everything the trill spreads over (±3·rate + the deviation) belongs to the carrier.
            .filter(|(k, m)| *k == 0 || (m[0] - c).abs() > 3.0 * tr + c * (2f32.powf(cents / 1200.0) - 1.0))
            .map(|(_, m)| m)
            .collect();
        l.set("trill_hz", tr);
        l.set("trill_cents", cents.min(1200.0));
    }
    // Strikes: in the band of the strongest partial (wavelet) when that finds more than the broadband envelope.
    // (Not with a trill: its loudness dips look like strikes.)
    let banded = if trilled.is_some() { Vec::new() } else { crate::wavelet::band_onsets(&x[t0..], rate, modes[0][0]) };
    let on: Vec<(usize, f32)> = if banded.len() > on.len() { banded.iter().map(|o| (o.0 + t0, o.1)).collect() } else { on };
    // Two or more strikes: the measured onsets as the `hits` table (times from the first, levels re the first).
    if on.len() >= 2 {
        l.tables[1] = on.iter().take(32).flat_map(|o| [(o.0 - t0) as f32 / rate, (o.1 - on[0].1).clamp(-60.0, 24.0)]).collect();
    }
    if t0 > 0 {
        l.set("dly", t0 as f32 / rate);
    }
    l.set("a", 0.0);
    l.tables[0] = modes.into_iter().flatten().collect();
    Some(l)
}

/// Re-levels each row of the `hits` table of layer `k` (times from sample `base`) to the original's 2 ms envelope
/// peak within 6 ms of the hit (3 passes, ±12 dB per pass).
fn relevel_hits(i: usize, orig: &[f32], rate: f32, p: &mut Params, k: usize, base: usize) {
    let w = ((0.002 * rate) as usize).max(1);
    let peak = |x: &[f32], a: usize| {
        (a..(a + 3 * w).min(x.len().saturating_sub(w)))
            .step_by((w / 2).max(1))
            .map(|j| x[j..j + w].iter().map(|v| v * v).sum::<f32>())
            .fold(0.0f32, f32::max)
    };
    for _ in 0..3 {
        let y = crate::render_wave(i, p);
        let rows = p.layers[k].tables[1].len() / 2;
        for r in 0..rows {
            let a = base + (p.layers[k].tables[1][2 * r] * rate) as usize;
            if a + w >= orig.len() {
                continue;
            }
            let (po, py) = (peak(orig, a), peak(&y, a));
            if po > 1e-12 && py > 1e-12 {
                let g = &mut p.layers[k].tables[1][2 * r + 1];
                *g = (*g + (10.0 * (po / py).log10()).clamp(-12.0, 12.0)).clamp(-60.0, 24.0);
            }
        }
    }
}

/// Re-levels each mode so that, rendered, its start amplitude matches the original's (2 passes), then the click.
fn relevel(i: usize, orig: &[f32], rate: f32, p: &mut Params, k: usize, t0: usize, n: usize) {
    // Measure once the glide has settled (the modes sit at their final frequency); bounded steps, and never far
    // above the analysed level (a mode the original barely has must not be blown up).
    let gl = p.layers[k].get("glide");
    let t0 = if gl.abs() > 0.0 { t0 + (3.0 * p.layers[k].get("glide_t") * rate) as usize } else { t0 };
    if t0 + n > orig.len() {
        return;
    }
    let start: Vec<f32> = p.layers[k].tables[0].chunks(4).map(|r| r[1]).collect();
    for _ in 0..2 {
        let y = crate::render_wave(i, p);
        let rows = p.layers[k].tables[0].len() / 4;
        for r in 0..rows {
            let f = p.layers[k].tables[0][4 * r];
            let (ao, ay) = (amp_at(orig, t0, n, f, rate), amp_at(&y, t0, n, f, rate));
            if ao > 1e-7 && ay > 1e-7 {
                let g = &mut p.layers[k].tables[0][4 * r + 1];
                *g = (*g + (20.0 * (ao / ay).log10()).clamp(-12.0, 12.0)).clamp(-120.0, start[r] + 6.0);
            }
        }
    }
}

/// Click per hit of modal layer `k` from what the modes leave out of `orig[range]`: band = residual centroid
/// ±1 octave, decay = median envelope fall after the onsets (8.686 dB per e-fold), level = residual energy.
fn set_click(i: usize, orig: &[f32], rate: f32, p: &mut Params, k: usize, range: std::ops::Range<usize>) {
    use crate::analyze::avg_power;
    if range.len() < 64 {
        return;
    }
    p.layers[k].set("click_db", -80.0);
    let y = crate::render_wave(i, p);
    let (po, py) = (avg_power(&orig[range.clone()]), avg_power(&y[range.clone()]));
    let res: Vec<f32> = po.iter().zip(&py).map(|(a, b)| (a - b).max(0.0)).collect();
    let tot: f32 = res.iter().sum();
    if tot <= 0.0 {
        return;
    }
    let hz = |b: usize| b as f32 * rate / (2.0 * (res.len() - 1) as f32);
    let c = res.iter().enumerate().map(|(b, v)| hz(b) * v).sum::<f32>() / tot;
    // Decay: 1 ms envelope, from each onset's maximum to 10 dB below it.
    let hop = ((0.001 * rate) as usize).max(1);
    let e = env_db(&orig[range.clone()], hop);
    let mut falls: Vec<f32> = Vec::new();
    for (t, _) in onsets(&orig[range.clone()], rate) {
        let k0 = t / hop;
        if let Some(km) = (k0..(k0 + 6).min(e.len())).max_by(|a, b| e[*a].total_cmp(&e[*b])) {
            if let Some(d) = e[km..].iter().position(|v| *v < e[km] - 10.0) {
                falls.push(d as f32 * hop as f32 / rate / 1.151);
            }
        }
    }
    falls.sort_by(f32::total_cmp);
    let cd = falls.get(falls.len() / 2).copied().unwrap_or(0.003).clamp(0.0002, 0.1);
    let l = &mut p.layers[k];
    l.set("click_hp", (c / 2.0).clamp(20.0, 10000.0));
    l.set("click_lp", (c * 2.0).clamp(100.0, 0.49 * rate));
    l.set("click_decay", cd);
    // Level: click alone at 0 dB vs the residual energy.
    let mut solo = p.clone();
    solo.layers = vec![p.layers[k].clone()];
    solo.layers[0].tables[0].clear();
    solo.layers[0].set("click_db", 0.0);
    let z = crate::render_wave(i, &solo);
    let ez: f32 = z[range.clone()].iter().map(|v| v * v).sum();
    let er: f32 = orig[range.clone()].iter().map(|v| v * v).sum::<f32>() - y[range.clone()].iter().map(|v| v * v).sum::<f32>();
    if ez > 1e-12 && er > 0.0 {
        p.layers[k].set("click_db", (10.0 * (er / ez).log10()).clamp(-80.0, 12.0));
    }
}

/// Adds a noise layer for what the model leaves out of `orig[range]` (per-bin power excess → 8 bands), with the
/// original's envelope (whole/attack) or periodic (sustain), calibrated to the residual RMS. Skipped under 3 %.
fn add_residual(i: usize, orig: &[f32], rate: f32, p: &mut Params, region: Region, range: std::ops::Range<usize>) {
    use crate::analyze::{attack_layer, avg_power, bands, calibrate, open_for_fit, prior_for_wave, rms};
    if p.layers.len() >= gmgen::schema::MAX_LAYERS || range.len() < 64 {
        return;
    }
    let y = crate::render_wave(i, p);
    let (po, py) = (avg_power(&orig[range.clone()]), avg_power(&y[range.clone()]));
    let res: Vec<f32> = po.iter().zip(&py).map(|(a, b)| (a - b).max(0.0)).collect();
    let (tr, to) = (res.iter().sum::<f32>(), po.iter().sum::<f32>());
    if tr < 0.03 * to {
        return;
    }
    let mut nz = match region {
        Region::Sustain => {
            let mut l = Layer::new(family("noise").unwrap(), Region::Sustain);
            l.set("a", 0.0);
            l
        }
        r => attack_layer("noise", &prior_for_wave(i), orig, rate, r, orig.len()),
    };
    nz.tables[0] = bands(&res, rate);
    nz.set("level_db", -20.0);
    open_for_fit(&mut nz);
    p.layers.push(nz);
    let k = p.layers.len() - 1;
    calibrate(i, p, k, rms(&orig[range.clone()]) * (tr / to).sqrt(), range);
}

/// Whole-wave `modal` candidates for wave `i` (unlooped: a struck/rattle layer; looped: steady sparse loop or a
/// folded hit train, each with a struck attack layer when the attack is long enough).
pub fn candidates(i: usize, orig: &[f32], m: &WaveMeta) -> Vec<Params> {
    let rate = m.rate as f32;
    let mut out = Vec::new();
    let lp = m.looped.map(|(s, l)| (s as usize, l as usize)).filter(|&(s, l)| l > 0 && s + l <= orig.len());
    match lp {
        None => {
            if let Some(l) = struck(orig, rate, Region::Whole, 24) {
                let mut p = Params::default();
                p.layers.push(l);
                let t0 = (p.layers[0].get("dly") * rate) as usize;
                // Modes within the first hit only.
                let first = p.layers[0].tables[1].get(2).map_or(orig.len() - t0, |t| (t * rate) as usize);
                let n = (first / 4).next_power_of_two().clamp(128, 1024).min(first.max(64));
                relevel(i, orig, rate, &mut p, 0, t0, n);
                if !p.layers[0].tables[1].is_empty() {
                    relevel_hits(i, orig, rate, &mut p, 0, t0);
                }
                let mut r = p.clone();
                add_residual(i, orig, rate, &mut r, Region::Whole, 0..orig.len());
                out.push(r);
                out.push(p.clone());
                // With a click per hit for the strike / scrape noise.
                let mut q = p.clone();
                set_click(i, orig, rate, &mut q, 0, 0..orig.len());
                out.push(q.clone());
                add_residual(i, orig, rate, &mut q, Region::Whole, 0..orig.len());
                out.push(q);
            }
        }
        Some((s, l)) => {
            let lpx = &orig[s..s + l];
            // Attack layer when the attack is ≥ 10 ms.
            let attack = (s >= (0.01 * rate) as usize).then(|| struck(&orig[..s], rate, Region::Attack, 16)).flatten();
            // (a) steady sparse loop
            let mut sus = layer(Region::Sustain, sparse_loop(lpx, rate, 32));
            sus.set("a", 0.0);
            let mut p = Params::default();
            if let Some(a) = &attack {
                p.layers.push(a.clone());
            }
            p.layers.push(sus);
            if let Some(k) = p.layers.iter().position(|x| x.region == Region::Attack) {
                let n = (s / 4).next_power_of_two().clamp(128, 1024).min(s);
                relevel(i, orig, rate, &mut p, k, 0, n);
            }
            let mut r = p.clone();
            add_residual(i, orig, rate, &mut r, Region::Sustain, s..s + l);
            out.push(r);
            out.push(p.clone());
            // (b) folded hit train: onsets over three tiles of the loop.
            let tiled: Vec<f32> = lpx.iter().chain(lpx).chain(lpx).copied().collect();
            let mut on: Vec<(usize, f32)> = onsets(&tiled, rate).into_iter().filter(|o| o.0 >= l && o.0 < 2 * l).collect();
            // Strikes in the band of the loop's strongest partial (wavelet), when that finds more.
            if let Some(f) = partials(lpx, rate, 1).first().map(|m| m[0]) {
                let b: Vec<(usize, f32)> = crate::wavelet::band_onsets(&tiled, rate, f).into_iter().filter(|o| o.0 >= l && o.0 < 2 * l).collect();
                if b.len() > on.len() {
                    on = b;
                }
            }
            if on.len() >= 2 {
                let first = on[0].0;
                let seg: Vec<f32> = tiled[first..first + l].to_vec();
                if let Some(mut h) = struck(&seg, rate, Region::Sustain, 16) {
                    h.set("dly", 0.0);
                    // Hit table: onsets of one loop tile, times from the loop start.
                    let lv0 = on[0].1;
                    h.tables[1] = on.iter().take(32).flat_map(|o| [(o.0 - l) as f32 / rate, (o.1 - lv0).clamp(-60.0, 24.0)]).collect();
                    let mut q = Params::default();
                    if let Some(a) = &attack {
                        q.layers.push(a.clone());
                    }
                    q.layers.push(h);
                    let k = q.layers.len() - 1;
                    let n = ((l / on.len()) / 2).next_power_of_two().clamp(128, 1024);
                    relevel(i, orig, rate, &mut q, k, s + (first - l), n);
                    relevel_hits(i, orig, rate, &mut q, k, s);
                    let mut r = q.clone();
                    add_residual(i, orig, rate, &mut r, Region::Sustain, s..s + l);
                    out.push(r);
                    let mut c = q.clone();
                    set_click(i, orig, rate, &mut c, k, s..s + l);
                    out.push(c.clone());
                    add_residual(i, orig, rate, &mut c, Region::Sustain, s..s + l);
                    out.push(c);
                    out.push(q);
                }
            }
            // (c) rattle (vibraslap): measured strikes of the loop, short rings, a broadband click each.
            out.extend(rattle(i, orig, rate, s, l, attack.as_ref()));
        }
    }
    out.into_iter().filter(|p| p.validate().is_ok()).collect()
}

/// Strikes of a loop: (sample from the loop start, dB) of 0.5 ms envelope peaks that are maxima over ±4 ms and within
/// 20 dB of the loudest (loop tiled ×3 so the ends wrap).
pub fn strikes(lpx: &[f32], rate: f32) -> Vec<(usize, f32)> {
    let l = lpx.len();
    let tiled: Vec<f32> = lpx.iter().chain(lpx).chain(lpx).copied().collect();
    let fr = ((0.0005 * rate) as usize).max(1);
    let d: Vec<f32> = (0..tiled.len().saturating_sub(2 * fr)).step_by(fr).map(|i| {
        let c = &tiled[i..i + 2 * fr];
        10.0 * (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).max(1e-20).log10()
    }).collect();
    let top = d.iter().copied().fold(f32::MIN, f32::max);
    let w = ((0.004 * rate) as usize / fr).max(1);
    (w..d.len().saturating_sub(w))
        .filter(|&k| d[k] > top - 20.0 && d[k - w..=k + w].iter().all(|v| *v <= d[k]))
        .map(|k| (k * fr, d[k]))
        .filter(|&(i, _)| i >= l && i < 2 * l)
        .map(|(i, v)| (i - l, v))
        .collect()
}

/// Rattle candidates for a loop of `l` samples at `s`: the measured strikes as the hit table, the loop's 3 strongest
/// partials ringing with the strikes' decay (median −15 dB fall / 1.73), a broadband click per strike (0 / +6 dB
/// above 1 kHz), levelled to the loop. A vibraslap is 8 strikes per 109 ms loop, each gone within ~8 ms: rings that
/// last longer merge the strikes into a steady tone ("like a motor").
fn rattle(i: usize, orig: &[f32], rate: f32, s: usize, l: usize, attack: Option<&Layer>) -> Vec<Params> {
    let lpx = &orig[s..s + l];
    let st = strikes(lpx, rate);
    if st.len() < 3 || st.len() > 32 {
        return Vec::new();
    }
    let fr = ((0.0005 * rate) as usize).max(1);
    let tiled: Vec<f32> = lpx.iter().chain(lpx).copied().collect();
    let env: Vec<f32> = tiled.chunks(fr).map(|c| 10.0 * (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).max(1e-20).log10()).collect();
    let mut falls: Vec<f32> = st
        .iter()
        .filter_map(|&(at, db)| (at / fr..env.len()).find(|&k| env[k] < db - 15.0).map(|k| (k * fr - at) as f32 / rate))
        .collect();
    falls.sort_by(f32::total_cmp);
    let dec = falls.get(falls.len() / 2).map_or(0.005, |t| t / 1.73).clamp(0.001, 0.05);
    let top = st.iter().map(|x| x.1).fold(f32::MIN, f32::max);
    // The 3 strongest partials, dropping any within 5 % of a stronger one (two close modes beat).
    let mut ps: Vec<[f32; 4]> = Vec::new();
    for p in partials(lpx, rate, 6) {
        if ps.len() < 3 && ps.iter().all(|q| (p[0] / q[0] - 1.0).abs() > 0.05) {
            ps.push(p);
        }
    }
    let modes: Vec<f32> = ps.iter().flat_map(|p| [p[0], p[1], dec, 0.0]).collect();
    if modes.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<(f32, Params)> = Vec::new();
    // Grid: click level and band (a vibraslap's strikes are bright: 4–10 kHz carries as much as the 1.25 kHz ring),
    // mode attack (an instant onset splatters below the ring); the best two by the fit objective.
    for (click, hp, ha) in [0.0f32, 6.0, 12.0, 18.0].into_iter().flat_map(|c| [1000.0f32, 3000.0].into_iter().flat_map(move |h| [0.0002f32, 0.001].map(|a| (c, h, a)))) {
        let mut h = layer(Region::Sustain, Vec::new());
        h.tables[0] = modes.clone();
        let m0 = modes[1];
        for r in h.tables[0].chunks_mut(4) {
            r[1] -= m0;
        }
        h.tables[1] = st.iter().flat_map(|&(at, db)| [at as f32 / rate, (db - top).clamp(-60.0, 24.0)]).collect();
        h.set("hits", st.len() as f32);
        h.set("click_db", click);
        h.set("click_decay", 0.002);
        h.set("click_hp", hp);
        h.set("ha", ha);
        h.set("click_lp", 20000.0);
        h.set("a", 0.0);
        crate::analyze::open_for_fit(&mut h);
        let mut q = Params::default();
        if let Some(a) = attack {
            q.layers.push(a.clone());
        }
        q.layers.push(h);
        let k = q.layers.len() - 1;
        if attack.is_some() {
            // The attack layer covers the head: the train starts at the loop (5 ms fade-in).
            let f = 0.005f32.min(s as f32 / rate);
            q.layers[k].set("dly", s as f32 / rate - f);
            q.layers[k].set("a", f.max(0.0005));
        }
        // The click does not follow `level_db`: settle the level in 3 passes.
        for _ in 0..3 {
            crate::analyze::calibrate(i, &mut q, k, crate::analyze::rms(lpx), s..s + l);
        }
        if q.validate().is_ok() {
            out.push((crate::fit::score_of(orig, i, &q), q));
        }
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    let out: Vec<Params> = out.into_iter().take(2).map(|x| x.1).collect();
    if std::env::var_os("GMFIT_TRACE").is_some() {
        eprintln!("modal rattle: {} strikes, ring decay {dec:.4} s, modes {modes:?}", st.len());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    const SR: f32 = 22050.0;

    fn decaying(fs: &[(f32, f32, f32)], secs: f32) -> Vec<f32> {
        (0..(secs * SR) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                fs.iter().map(|(f, a, d)| a * (-t / d).exp() * (std::f32::consts::TAU * f * t).sin()).sum()
            })
            .collect()
    }

    #[test]
    fn partials_find_frequency_level_and_decay() {
        let x = decaying(&[(732.0, 0.5, 0.08), (1119.0, 0.2, 0.04)], 0.3);
        let p = partials(&x, SR, 8);
        assert!((p[0][0] - 732.0).abs() < 3.0, "{p:?}");
        assert!((p[0][1] - 20.0 * 0.5f32.log10()).abs() < 2.0, "level {p:?}");
        assert!((p[0][2] / 0.08 - 1.0).abs() < 0.2, "decay {p:?}");
        assert!(p.iter().any(|m| (m[0] - 1119.0).abs() < 4.0), "{p:?}");
    }

    #[test]
    fn glide_measures_a_falling_tone() {
        // 150 Hz · (1 + 0.3·e^{−t/0.02}) → starts ≈ +454 cents.
        let (f, r, tau) = (150.0f32, 1.3f32, 0.02f32);
        let x: Vec<f32> = (0..(0.2 * SR) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                (std::f32::consts::TAU * f * (t + (r - 1.0) * tau * (1.0 - (-t / tau).exp()))).sin()
            })
            .collect();
        let (c, t) = glide(&x, SR, f).expect("glide");
        assert!(c > 200.0 && c < 700.0, "cents {c}");
        assert!(t > 0.005 && t < 0.05, "tau {t}");
        assert!(glide(&decaying(&[(150.0, 0.5, 1.0)], 0.2), SR, 150.0).is_none());
    }

    #[test]
    fn onsets_find_each_strike() {
        let hit = decaying(&[(1248.0, 0.5, 0.004)], 0.03);
        let mut x = vec![0.0f32; (0.3 * SR) as usize];
        for k in [0usize, 30, 52, 80, 120] {
            let a = (k as f32 * 0.001 * SR) as usize;
            for (j, v) in hit.iter().enumerate() {
                if a + j < x.len() {
                    x[a + j] += v;
                }
            }
        }
        let on = onsets(&x, SR);
        assert_eq!(on.len(), 5, "{on:?}");
    }

    #[test]
    fn strikes_of_a_rattle_loop() {
        let hit = decaying(&[(1250.0, 0.5, 0.004)], 0.012);
        let l = (0.109 * SR) as usize;
        let mut x = vec![0.0f32; l];
        let times = [2usize, 15, 29, 42, 56, 69, 83, 98];
        for k in times {
            let a = (k as f32 * 0.001 * SR) as usize;
            for (j, v) in hit.iter().enumerate() {
                x[(a + j) % l] += v;
            }
        }
        let st = strikes(&x, SR);
        assert_eq!(st.len(), 8, "{st:?}");
        for (s, t) in st.iter().zip(times) {
            assert!((s.0 as f32 / SR * 1000.0 - t as f32).abs() < 1.5, "{st:?}");
        }
    }

    #[test]
    fn sparse_loop_keeps_the_prominent_bins() {
        let l = 2000usize;
        let x: Vec<f32> = (0..l)
            .map(|n| {
                let c = |k: f32, a: f32, ph: f32| a * (std::f32::consts::TAU * (k * n as f32 / l as f32 + ph)).sin();
                c(311.0, 0.3, 0.1) + c(457.0, 0.1, 0.7)
            })
            .collect();
        let m = sparse_loop(&x, SR, 8);
        assert_eq!(m.len(), 2, "{m:?}");
        assert!((m[0][0] - 311.0 * SR / l as f32).abs() < 0.01);
        assert!((m[0][1] - 20.0 * 0.3f32.log10()).abs() < 0.1);
        assert!(((m[0][3] - 0.1).rem_euclid(1.0)).min((0.1 - m[0][3]).rem_euclid(1.0)) < 0.01, "phase {}", m[0][3]);
    }

    #[test]
    fn periodicity_separates_tone_from_noise() {
        let tone = decaying(&[(440.0, 0.5, 10.0)], 0.3);
        let mut r = 12345u32;
        let noise: Vec<f32> = (0..tone.len())
            .map(|_| {
                r ^= r << 13;
                r ^= r >> 17;
                r ^= r << 5;
                (r >> 8) as f32 / 8_388_608.0 - 1.0
            })
            .collect();
        let (pt, pn) = (crate::score::periodicity(&tone, SR).unwrap(), crate::score::periodicity(&noise, SR).unwrap());
        assert!(pt > 0.95 && pn < 0.3, "tone {pt}, noise {pn}");
    }
}
