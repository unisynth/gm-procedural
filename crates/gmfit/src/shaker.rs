//! Shaker analysis (design D4o): PhISEM candidates for sounds made of many tiny collisions (tambourine jingles,
//! maracas, applause, a stream, bubbles). Measures on the original (the loop when looped, else the whole wave):
//! collision onsets on a 0.5 ms envelope (rate, burst decay, level spread), up to 3 resonances by peak-picking the
//! smoothed spectrum (Q from the −3 dB width, each peak's band-pass response removed before the next), and the
//! envelope (one-shot: attack + decay time; loop: steady). The onset count is approximate (dense events merge),
//! so candidates span rate ×0.5/1/2, chirp 0/+1 octave (bubbles rise), detune 0/300 cents and resonance Q ×1/0.25
//! (jingles click broadband rather than ring); the caller keeps the
//! best by rank.
use crate::analyze::{avg_power, calibrate, open_for_fit, rms};
use crate::audio::loop_of;
use gmgen::params::{Layer, Params};
use gmgen::render::WaveMeta;
use gmgen::schema::{family, Region};
use rustfft::num_complex::Complex32;

/// Collision onsets: (sample, dB) on a 0.5 ms RMS envelope (1 ms window) — rises ≥ 4 dB within 2 ms to a local
/// maximum ≥ 4 dB above the median of the surrounding 20 ms, above peak − 40 dB, ≥ 1.5 ms apart. (Finer frames
/// count a noise burst's own fluctuations: 2.5× the true rate.)
pub fn collisions(x: &[f32], rate: f32) -> (Vec<(usize, f32)>, Vec<f32>, usize) {
    let hop = ((0.0005 * rate) as usize).max(1);
    let e: Vec<f32> = (0..x.len() / hop)
        .map(|k| {
            let s = &x[k * hop..(k * hop + 2 * hop).min(x.len())];
            10.0 * (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32 + 1e-12).log10()
        })
        .collect();
    let top = e.iter().copied().fold(f32::MIN, f32::max);
    let (rise, ctx_n) = (4usize, (0.010 * rate / hop as f32) as usize);
    let gap = (0.0015 * rate) as usize;
    let mut out: Vec<(usize, f32)> = Vec::new();
    for k in 1..e.len().saturating_sub(1) {
        if e[k] < e[k - 1] || e[k] < e[k + 1] || e[k] < top - 40.0 {
            continue;
        }
        let lo = e[k.saturating_sub(rise)..k].iter().copied().fold(f32::MAX, f32::min);
        if e[k] - lo < 4.0 {
            continue;
        }
        let mut nb: Vec<f32> = e[k.saturating_sub(ctx_n)..(k + ctx_n).min(e.len())].to_vec();
        nb.sort_by(f32::total_cmp);
        if e[k] - nb[nb.len() / 2] < 4.0 {
            continue;
        }
        if out.last().is_some_and(|l| k * hop < l.0 + gap) {
            continue;
        }
        out.push((k * hop, e[k]));
    }
    (out, e, hop)
}

/// Up to 3 resonances [Hz, Q, gain dB re the strongest] from a 2048-point power spectrum (bins 0..=1024).
pub fn resonances(power: &[f32], rate: f32) -> Vec<f32> {
    let nb = power.len() - 1;
    let hz = |b: usize| b as f32 * rate / (2.0 * nb as f32);
    // 1/12-octave smoothing.
    let sm: Vec<f32> = (0..=nb)
        .map(|b| {
            let w = ((b as f32 * 0.03) as usize).max(1);
            let r = b.saturating_sub(w)..=(b + w).min(nb);
            let n = r.clone().count() as f32;
            r.map(|k| power[k]).sum::<f32>() / n
        })
        .collect();
    let mut rest = sm.clone();
    let mut out = Vec::new();
    let mut top = 0.0f32;
    for _ in 0..3 {
        let Some(pk) = (1..nb).filter(|&b| hz(b) >= 150.0 && hz(b) <= 0.45 * rate).max_by(|a, b| rest[*a].total_cmp(&rest[*b])) else { break };
        let v = rest[pk];
        if v <= 0.0 || (top > 0.0 && v < top * 1e-3) {
            break;
        }
        top = top.max(v);
        let half = v * 0.5;
        let lo = (1..pk).rev().find(|&b| rest[b] < half).unwrap_or(1);
        let hi = (pk..nb).find(|&b| rest[b] < half).unwrap_or(nb);
        let f = hz(pk);
        let q = (f / (hz(hi) - hz(lo)).max(1.0)).clamp(0.7, 60.0);
        out.extend([f, q, 10.0 * (v / top).log10()]);
        // Remove this resonance's band-pass power response before the next peak.
        for (b, r) in rest.iter_mut().enumerate() {
            let x = hz(b) / f;
            let h = if x > 0.0 { 1.0 / (1.0 + q * q * (x - 1.0 / x).powi(2)) } else { 0.0 };
            *r = (*r - v * h).max(0.0);
        }
    }
    out
}

/// One third-octave band's ringing: centre Hz, ring index (envelope-power autocorrelation width × bandwidth; plain
/// noise reads `NOISE_INDEX` in every band, ringing events more), the event ring τ (s, amplitude 1/e) beyond what
/// the band itself imposes, and mean power.
#[derive(Clone, Debug)]
pub struct BandRing {
    pub hz: f32,
    pub index: f32,
    pub tau: f32,
    pub power: f32,
}

const THIRD: f32 = 1.259_921;
/// Ring index of Gaussian noise (measured 0.47–0.58 over a 0.37 s loop).
pub const NOISE_INDEX: f32 = 0.5;
/// Index from which a band rings (droplets, struck rings) rather than hisses.
pub const RING_INDEX: f32 = 0.75;

/// Mean power of `x` between `lo` and `hi` Hz.
fn band_power(spec: &[Complex32], rate: f32, lo: f32, hi: f32) -> f32 {
    let n = spec.len();
    (0..n)
        .filter(|&k| {
            let f = k.min(n - k) as f32 * rate / n as f32;
            f >= lo && f < hi
        })
        .map(|k| spec[k].norm_sqr())
        .sum::<f32>()
        / (n as f32 * n as f32)
}

fn spectrum(x: &[f32]) -> Vec<Complex32> {
    let mut s: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    crate::plan::forward(s.len()).process(&mut s);
    s
}

/// Ring of every third-octave band from 500 Hz of `x` (a loop: circular). The band's analytic envelope power, its
/// circular autocorrelation, the lag where it falls to 1/e (w). Noise decorrelates within ≈ 0.5 / bandwidth; a
/// texture of ringing events (amplitude decay τ: energy autocorrelation e^{−2|lag|/τ}) stays correlated ≈ τ/2.
/// Water rings (big drops low, small droplets high) over a hissing bed; counting onsets cannot tell: a 6 dB rise
/// in 2 ms happens all the time in band-limited noise.
pub fn bands(x: &[f32], rate: f32) -> Vec<BandRing> {
    let n = x.len();
    if n < (0.02 * rate) as usize {
        return Vec::new();
    }
    let spec = spectrum(x);
    let (fwd, inv) = (crate::plan::forward(n), crate::plan::inverse(n));
    let mut out = Vec::new();
    let mut c = 500.0f32;
    while c < 0.45 * rate {
        let (lo, hi) = (c / THIRD.sqrt(), (c * THIRD.sqrt()).min(0.45 * rate));
        // Analytic band signal: positive frequencies in the band, doubled.
        let mut z: Vec<Complex32> = (0..n)
            .map(|k| {
                let f = k as f32 * rate / n as f32;
                if k < n.div_ceil(2) && f >= lo && f < hi { spec[k] * 2.0 } else { Complex32::new(0.0, 0.0) }
            })
            .collect();
        inv.process(&mut z);
        let e: Vec<f32> = z.iter().map(|v| v.norm_sqr() / (n as f32 * n as f32)).collect();
        let power = e.iter().sum::<f32>() / n as f32 / 2.0;
        let mean = 2.0 * power;
        let mut ac: Vec<Complex32> = e.iter().map(|v| Complex32::new(v - mean, 0.0)).collect();
        fwd.process(&mut ac);
        for v in &mut ac {
            *v = Complex32::new(v.norm_sqr(), 0.0);
        }
        inv.process(&mut ac);
        let a0 = ac[0].re.max(1e-30);
        let at = |k: usize| ac[k].re / a0;
        let th = (-1.0f32).exp();
        let w = (1..n / 2).find(|&k| at(k) < th).map_or(n as f32 / 2.0, |k| k as f32 - 1.0 + (at(k - 1) - th) / (at(k - 1) - at(k)).max(1e-9)) / rate;
        let bw = hi - lo;
        out.push(BandRing { hz: c, index: w * bw, tau: 2.0 * (w * w - (NOISE_INDEX / bw).powi(2)).max(0.0).sqrt(), power });
        c *= THIRD;
    }
    out
}

/// A band that rings: index ≥ `RING_INDEX` within 20 dB of the loudest band (`top`).
pub fn ringing(b: &BandRing, top: f32) -> bool {
    b.power > top * 0.01 && b.index >= RING_INDEX
}

/// `bands` restricted to the ringing ones.
pub fn band_rings(x: &[f32], rate: f32) -> Vec<BandRing> {
    let mut out = bands(x, rate);
    let top = out.iter().map(|b| b.power).fold(0.0, f32::max);
    out.retain(|b| ringing(b, top));
    out
}

/// Splits the bands into ≤ `k` contiguous groups of like ring time (least squared spread of ln ring; DP).
pub fn ring_groups(b: &[BandRing], k: usize) -> Vec<std::ops::Range<usize>> {
    let m = b.len();
    let k = k.min(m);
    if k == 0 {
        return Vec::new();
    }
    let l: Vec<f64> = b.iter().map(|r| (r.tau.max(1e-4) as f64).ln()).collect();
    let cost = |a: usize, z: usize| {
        let mean = l[a..z].iter().sum::<f64>() / (z - a) as f64;
        l[a..z].iter().map(|v| (v - mean).powi(2)).sum::<f64>()
    };
    // best[g][j]: least cost of the first j bands in g groups; cut[g][j]: start of the last group.
    let mut best = vec![vec![f64::INFINITY; m + 1]; k + 1];
    let mut cut = vec![vec![0usize; m + 1]; k + 1];
    best[0][0] = 0.0;
    for g in 1..=k {
        for j in g..=m {
            for a in g - 1..j {
                let c = best[g - 1][a] + cost(a, j);
                if c < best[g][j] {
                    best[g][j] = c;
                    cut[g][j] = a;
                }
            }
        }
    }
    let mut out = Vec::new();
    let mut j = m;
    for g in (1..=k).rev() {
        out.push(cut[g][j]..j);
        j = cut[g][j];
    }
    out.reverse();
    out
}

/// Multi-layer droplet candidates: 1–3 ring groups, each its own shaker layer — one resonance at the group's
/// centre, scattered across the group (`fjit`), Q = π·f·τ from the measured ring, the collision rate ×1/2/4 (dense
/// droplets overlap: the measured onset rate undercounts) — levelled per group against the original's band power
/// (3 passes). Hissing bands are left to the current model's noise (`ring_combos`).
pub fn ring_layers(i: usize, seg: &[f32], base: &Layer, range: std::ops::Range<usize>, rate: f32, r_est: f32) -> Vec<Params> {
    let b = band_rings(seg, rate);
    if std::env::var_os("GMFIT_TRACE").is_some() {
        for r in &b {
            eprintln!("  band {:6.0} Hz: index {:4.2} τ {:5.2} ms power {:6.1} dB", r.hz, r.index, 1000.0 * r.tau, 10.0 * r.power.log10());
        }
    }
    let want = spectrum(seg);
    let mut out = Vec::new();
    for k in 1..=2usize.min(b.len()) {
        let groups = ring_groups(&b, k);
        for mul in [1.0f32, 2.0, 4.0] {
            let mut p = Params::default();
            let mut edges = Vec::new();
            for g in &groups {
                let v = &b[g.clone()];
                let w: f32 = v.iter().map(|r| r.power).sum::<f32>().max(1e-20);
                let fc = (v.iter().map(|r| r.power * r.hz.ln()).sum::<f32>() / w).exp();
                let (lo, hi) = (v[0].hz / THIRD.sqrt(), v[v.len() - 1].hz * THIRD.sqrt());
                let mut q: Vec<f32> = v.iter().map(|r| std::f32::consts::PI * r.hz * r.tau).collect();
                q.sort_by(f32::total_cmp);
                let mut l = base.clone();
                l.tables[0] = vec![fc, q[q.len() / 2].clamp(2.0, 200.0), 0.0];
                l.set("fjit", (600.0 * (hi / lo).log2()).clamp(100.0, 1200.0));
                l.set("rate", (r_est * mul).clamp(1.0, 4000.0));
                l.set("chirp", 0.0);
                l.set("level_db", -20.0);
                open_for_fit(&mut l);
                p.layers.push(l);
                edges.push((lo, hi));
            }
            for _ in 0..3 {
                let x = crate::render_wave(i, &p);
                let got = spectrum(&x[range.start.min(x.len())..range.end.min(x.len())]);
                for (l, &(lo, hi)) in p.layers.iter_mut().zip(&edges) {
                    let (o, g) = (band_power(&want, rate, lo, hi), band_power(&got, rate, lo, hi));
                    if o > 0.0 && g > 0.0 {
                        let v = l.get("level_db");
                        l.set("level_db", (v + 10.0 * (o / g).log10()).clamp(-80.0, 24.0));
                    }
                }
            }
            if p.validate().is_ok() {
                out.push(p);
            }
        }
        if std::env::var_os("GMFIT_TRACE").is_some() {
            let d: Vec<String> = groups
                .iter()
                .map(|g| {
                    let v = &b[g.clone()];
                    format!("{:.0}–{:.0} Hz τ {:.2} ms", v[0].hz, v[v.len() - 1].hz, 1000.0 * v[v.len() / 2].tau)
                })
                .collect();
            eprintln!("shaker ring layers {k}: {}", d.join(" | "));
        }
    }
    out
}

/// Analysed starting point for wave `i`: base shaker layer (resonances, burst decay, level spread, envelope), the
/// analysed range, collision rate and burst decay. `None` when the wave is too short or silent.
struct Setup {
    base: Layer,
    range: std::ops::Range<usize>,
    r_est: f32,
    sd: f32,
}

fn setup(orig: &[f32], m: &WaveMeta) -> Option<Setup> {
    let rate = m.rate as f32;
    let (region, range) = match loop_of(m) {
        Some((s, l)) if l >= (0.05 * rate) as usize => (Region::Sustain, s..s + l),
        Some(_) => return None,
        None => (Region::Whole, 0..orig.len()),
    };
    let seg = &orig[range.clone()];
    if seg.len() < (0.02 * rate) as usize || rms(seg) < 1e-4 {
        return None;
    }
    let (ons, e, hop) = collisions(seg, rate);
    let top = e.iter().copied().fold(f32::MIN, f32::max);
    let active = e.iter().filter(|v| **v > top - 30.0).count().max(1) as f32 * hop as f32 / rate;
    let r_est = (ons.len() as f32 / active).clamp(5.0, 4000.0);
    // Burst decay: median fall to −10 dB after each onset (amplitude e^{−t/sd}: −10 dB at 1.15·sd).
    let mut falls: Vec<f32> = ons
        .iter()
        .filter_map(|&(at, db)| {
            let k = at / hop;
            (k..e.len().min(k + (0.05 * rate) as usize / hop)).find(|&j| e[j] < db - 10.0).map(|j| (j - k) as f32 * hop as f32 / rate)
        })
        .collect();
    falls.sort_by(f32::total_cmp);
    let sd = falls.get(falls.len() / 2).map_or(0.003, |t| t / 1.15).clamp(0.0002, 0.05);
    let lv: Vec<f32> = ons.iter().map(|o| o.1).collect();
    let mean = lv.iter().sum::<f32>() / lv.len().max(1) as f32;
    let gj = ((lv.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / lv.len().max(1) as f32).sqrt() * 3f32.sqrt()).clamp(0.0, 18.0);
    let res = resonances(&avg_power(seg), rate);
    let mut base = Layer::new(family("shaker").unwrap(), region);
    base.tables[0] = res;
    base.set("sdecay", sd);
    base.set("gjit", gj);
    base.set("follow", 0.0);
    if region == Region::Whole {
        // One-shot envelope: attack to the 2 ms RMS peak, exponential decay to 1/e.
        let frame = ((0.002 * rate) as usize).max(1);
        let env: Vec<f32> = seg.chunks(frame).map(rms).collect();
        let (kp, pk) = env.iter().enumerate().fold((0, 0.0f32), |b, (k, v)| if *v > b.1 { (k, *v) } else { b });
        let k1 = env[kp..].iter().position(|v| *v < pk / std::f32::consts::E).map_or(env.len() - kp, |k| k);
        base.set("a", ((kp as f32 + 0.5) * frame as f32 / rate).max(0.0002));
        base.set("h", 0.0);
        base.set("s", 0.0);
        base.set("d", (k1 as f32 * frame as f32 / rate).max(0.002));
    }
    if std::env::var_os("GMFIT_TRACE").is_some() {
        eprintln!("shaker: {} onsets / {active:.3} s → rate {r_est:.0}, sdecay {sd:.4}, gjit {gj:.1}, res {:?}", ons.len(), base.tables[0]);
    }
    Some(Setup { base, range, r_est, sd })
}

/// PhISEM candidates for wave `i` (empty when the wave is too short or silent).
pub fn candidates(i: usize, orig: &[f32], m: &WaveMeta) -> Vec<Params> {
    let rate = m.rate as f32;
    let Some(Setup { base, range, r_est, sd }) = setup(orig, m) else { return Vec::new() };
    let seg = &orig[range.clone()];
    let mut out = Vec::new();
    // Droplets (water): each collision a narrow ringing tone (Q × 4) at a widely scattered pitch (±900–1200 cents),
    // sweeping up half an octave — a stream heard as clicks without them ("not watery").
    let drops = [0.5f32, 1.0, 2.0].into_iter().flat_map(|m| [900.0f32, 1200.0].map(|f| (m, 0.5f32, f, 4.0f32)));
    for (mul, chirp, fj, qm) in [0.5f32, 1.0, 2.0]
        .into_iter()
        .flat_map(|m| [0.0f32, 1.0].into_iter().flat_map(move |c| [0.0f32, 300.0].into_iter().flat_map(move |f| [1.0f32, 0.25].map(|q| (m, c, f, q)))))
        .chain(drops)
    {
        {
            {
                let mut l = base.clone();
                // Q × 0.25: jingles click broadband (high percussive share) rather than ring as tones.
                for r in l.tables[0].chunks_mut(3) {
                    r[1] = (r[1] * qm).clamp(0.7, 60.0);
                }
                if qm > 1.0 {
                    l.set("chirp_t", (2.0 * sd).clamp(0.0005, 0.5));
                }
                l.set("rate", (r_est * mul).clamp(1.0, 4000.0));
                l.set("chirp", chirp);
                l.set("fjit", fj);
                open_for_fit(&mut l);
                let mut p = Params::default();
                p.layers.push(l);
                calibrate(i, &mut p, 0, rms(seg), range.clone());
                if p.validate().is_ok() {
                    out.push(p);
                }
            }
        }
    }
    out.extend(ring_layers(i, seg, &base, range.clone(), rate, r_est));
    out
}

/// The current model `cur` with its shaker layers replaced by ring layers (`ring_layers`), sharing the energy:
/// ring layers `share` of it, the rest (its hissing noise bed) 1 − `share` — water is droplets over hiss. When the
/// layers do not fit, the model keeps only its noise layers. Empty for pitched or unanalysable waves.
pub fn ring_combos(i: usize, orig: &[f32], m: &WaveMeta, cur: &Params) -> Vec<Params> {
    let Some(Setup { base, range, r_est, .. }) = setup(orig, m) else { return Vec::new() };
    let rings = ring_layers(i, &orig[range.clone()], &base, range, m.rate as f32, r_est);
    let keep: Vec<Layer> = cur.layers.iter().filter(|l| Some(l.family) != family("shaker")).cloned().collect();
    let noise: Vec<Layer> = keep.iter().filter(|l| Some(l.family) == family("noise")).cloned().collect();
    let mut out = Vec::new();
    for r in &rings {
        for bed in [&keep, &noise] {
            if bed.is_empty() || bed.len() + r.layers.len() > gmgen::schema::MAX_LAYERS {
                continue;
            }
            for share in [0.5f32, 0.8] {
                let mut p = cur.clone();
                p.layers = bed.clone();
                for l in &mut p.layers {
                    let v = l.get("level_db");
                    l.set("level_db", (v + 10.0 * (1.0 - share).log10()).clamp(-80.0, 24.0));
                }
                for l in &r.layers {
                    let mut l = l.clone();
                    let v = l.get("level_db");
                    l.set("level_db", (v + 10.0 * share.log10()).clamp(-80.0, 24.0));
                    p.layers.push(l);
                }
                if p.validate().is_ok() {
                    out.push(p);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The analysis recovers a rendered shaker: resonance within 10 %, onset rate within a factor 2 (dense events
    /// merge — the candidates span ×0.5/1/2 for that).
    #[test]
    fn recovers_rate_and_resonance() {
        let m = WaveMeta { rate: 22050, len: 22050, unity: 60, fine: 0, looped: None };
        let mut l = Layer::new(family("shaker").unwrap(), Region::Whole);
        l.set("rate", 150.0);
        l.set("d", 30.0);
        l.set("sdecay", 0.001);
        l.tables[0] = vec![3000.0, 8.0, 0.0];
        let mut p = Params::default();
        p.layers.push(l);
        let x = gmgen::render::render(&m, &p, 9);
        let (ons, e, hop) = collisions(&x, 22050.0);
        let top = e.iter().copied().fold(f32::MIN, f32::max);
        let active = e.iter().filter(|v| **v > top - 30.0).count() as f32 * hop as f32 / 22050.0;
        let r = ons.len() as f32 / active;
        assert!((75.0..300.0).contains(&r), "rate {r}");
        let res = resonances(&avg_power(&x), 22050.0);
        assert!((res[0] - 3000.0).abs() < 300.0, "resonance {:?}", res);
    }

    /// Ring index: white noise reads ≈ `NOISE_INDEX` in every band; droplets ringing at Q 35 read above `RING_INDEX`
    /// in their band.
    #[test]
    fn ring_index_tells_droplets_from_hiss() {
        let mut r = gmgen::math::Rng::new(3);
        let noise: Vec<f32> = (0..9000).map(|_| r.white()).collect();
        for b in bands(&noise, 24000.0) {
            assert!((0.35..0.7).contains(&b.index), "noise {} Hz index {}", b.hz, b.index);
        }
        let m = WaveMeta { rate: 24000, len: 9000, unity: 60, fine: 0, looped: None };
        let mut l = Layer::new(family("shaker").unwrap(), Region::Whole);
        l.set("rate", 300.0);
        l.set("follow", 0.0);
        l.set("d", 30.0);
        l.set("sdecay", 0.0003);
        l.set("fjit", 100.0);
        l.tables[0] = vec![4000.0, 35.0, 0.0];
        let mut p = Params::default();
        p.layers.push(l);
        let x = gmgen::render::render(&m, &p, 9);
        let b = bands(&x, 24000.0);
        let at4k = b.iter().find(|b| (3500.0..4500.0).contains(&b.hz)).unwrap();
        assert!(at4k.index > RING_INDEX, "droplets index {}", at4k.index);
        let q = std::f32::consts::PI * 4000.0 * at4k.tau;
        assert!((15.0..80.0).contains(&q), "droplets Q {q}");
    }

    /// Two droplet layers — big drops ringing long around 2 kHz, small ones ringing short around 8 kHz — read as two
    /// ring groups split between them, the low one ringing longer.
    #[test]
    fn separates_ring_layers() {
        let m = WaveMeta { rate: 24000, len: 24000, unity: 60, fine: 0, looped: None };
        let mut p = Params::default();
        for (hz, q, rate) in [(2000.0f32, 100.0f32, 60.0f32), (8000.0, 60.0, 150.0)] {
            let mut l = Layer::new(family("shaker").unwrap(), Region::Whole);
            l.set("rate", rate);
            l.set("follow", 0.0);
            l.set("d", 30.0);
            l.set("sdecay", 0.0003);
            l.set("fjit", 250.0);
            l.tables[0] = vec![hz, q, 0.0];
            p.layers.push(l);
        }
        let x = gmgen::render::render(&m, &p, 9);
        let b = band_rings(&x, 24000.0);
        let g = ring_groups(&b, 2);
        assert_eq!(g.len(), 2, "{g:?}");
        let (lo, hi) = (&b[g[0].clone()], &b[g[1].clone()]);
        let mid = |v: &[BandRing]| v.iter().map(|r| r.tau).sum::<f32>() / v.len() as f32;
        assert!(lo.iter().any(|r| (1600.0..2600.0).contains(&r.hz)), "low group {:?}", lo.iter().map(|r| r.hz).collect::<Vec<_>>());
        assert!(hi.iter().any(|r| (6500.0..10000.0).contains(&r.hz)), "high group {:?}", hi.iter().map(|r| r.hz).collect::<Vec<_>>());
        assert!(mid(lo) > 1.5 * mid(hi), "rings low {} high {}", mid(lo), mid(hi));
    }
}
