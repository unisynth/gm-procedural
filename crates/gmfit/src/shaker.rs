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

/// PhISEM candidates for wave `i` (empty when the wave is too short or silent).
pub fn candidates(i: usize, orig: &[f32], m: &WaveMeta) -> Vec<Params> {
    let rate = m.rate as f32;
    let (region, range) = match loop_of(m) {
        Some((s, l)) if l >= (0.05 * rate) as usize => (Region::Sustain, s..s + l),
        Some(_) => return Vec::new(),
        None => (Region::Whole, 0..orig.len()),
    };
    let seg = &orig[range.clone()];
    if seg.len() < (0.02 * rate) as usize || rms(seg) < 1e-4 {
        return Vec::new();
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
    if std::env::var_os("GMFIT_TRACE").is_some() {
        eprintln!("shaker: {} onsets / {active:.3} s → rate {r_est:.0}, sdecay {sd:.4}, gjit {gj:.1}, res {:?}", ons.len(), base.tables[0]);
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
}
