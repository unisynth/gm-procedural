//! Hearing-model difference (design D4p): what of the difference between an original and a copy a listener hears.
//! Ear model after PEAQ (ITU-R BS.1387) and the MPEG psychoacoustic models, compared in the excitation domain (two
//! independent noises never match sample by sample): outer/middle-ear weighting (Terhardt), half-Bark bands,
//! frequency spreading (lower 27 dB/Bark, upper 24 + 230/f − 0.2·L dB/Bark), threshold in quiet as internal noise,
//! forward masking (τ 8–30 ms), Zwicker specific loudness.
//! - `added` / `missing`: loudness of the copy's excitation above / below the original's by more than a JND of 3 dB
//!   plus 1.4 × the original's own frame-to-frame fluctuation (median over 100 ms — so an independent noise of the
//!   same texture scores ≤ 0.05), as a share of the original's loudness.
//! - `flutter`: loudness-envelope change rate per band (PEAQ ModDiff), copy − original; `swell`: share of the
//!   envelope spectrum at 2–20 Hz (shimmer, swish), copy − original. Both loudness-weighted.
//! Calibrated with same-texture noise surrogates (max added + missing 0.047, |moddiff| 0.017).
use rustfft::num_complex::Complex32;
use serde_json::{json, Value};

const N: usize = 1024;
const HOP: usize = 256;
/// Noise-floor thresholds (same-texture surrogates).
pub const AUDIBLE: f32 = 0.05;
pub const MOD_AUDIBLE: f32 = 0.02;
/// Roughness difference (dB) below which a copy is as rough as the original (pilots ≤ 0.5).
pub const ROUGH_AUDIBLE: f32 = 1.0;
/// Ring difference (octaves) below which droplets/rings ring like the original's (cymbals 0.1–0.3).
pub const RING_AUDIBLE: f32 = 0.3;
/// Frame-aligned span at the start (s); level distributions after it.
const ATTACK_S: f32 = 0.15;
/// Tolerance factor on the original's fluctuation for the distribution part (aligned part: 1.4).
const DIST_TOL: f32 = 0.5;

fn bark(f: f32) -> f32 {
    13.0 * (0.00076 * f).atan() + 3.5 * (f / 7500.0).powi(2).atan()
}

/// Terhardt outer/middle-ear weighting (dB) and threshold in quiet (dB SPL) at `f` Hz.
fn ear_db(f: f32) -> (f32, f32) {
    let k = f.max(20.0) / 1000.0;
    let w = -3.64 * k.powf(-0.8) + 6.5 * (-0.6 * (k - 3.3).powi(2)).exp() - 1e-3 * k.powi(4);
    (w, -w)
}

pub struct Ear {
    pub centres: Vec<f32>,
    /// frames × bands excitation (power, ~SPL scale)
    pub exc: Vec<Vec<f32>>,
    /// per band threshold in quiet (power)
    pub quiet: Vec<f32>,
    /// frames × bands: the band holds sound itself (within 30 dB of the frame's loudest band before spreading),
    /// not only excitation spread from a neighbour — where a difference is reported.
    pub holds: Vec<Vec<bool>>,
}

/// Excitation patterns of `x` (frames of 46 ms every 11.6 ms at 22 kHz).
pub fn excitation(x: &[f32], rate: f32) -> Ear {
    let nb = N / 2 + 1;
    let hz: Vec<f32> = (0..nb).map(|k| k as f32 * rate / N as f32).collect();
    let z: Vec<f32> = hz.iter().map(|&f| bark(f)).collect();
    let zmax = bark(rate / 2.0);
    let centres_z: Vec<f32> = (0..).map(|j| 0.5 + 0.5 * j as f32).take_while(|c| *c < zmax - 0.25).collect();
    let band_of: Vec<Option<usize>> = z.iter().map(|&zz| centres_z.iter().position(|c| zz >= c - 0.25 && zz < c + 0.25)).collect();
    let nband = centres_z.len();
    let fc: Vec<f32> = centres_z
        .iter()
        .map(|&c| hz.iter().zip(&z).find(|(_, zz)| **zz >= c).map_or(rate / 2.0, |(f, _)| *f))
        .collect();
    let weight: Vec<f32> = hz.iter().map(|&f| 10f32.powf(ear_db(f).0 / 10.0)).collect();
    let quiet: Vec<f32> = fc.iter().map(|&f| 10f32.powf(ear_db(f).1 / 10.0)).collect();
    let win: Vec<f32> = (0..N).map(|k| 0.5 - 0.5 * (std::f32::consts::TAU * k as f32 / N as f32).cos()).collect();
    let fft = crate::plan::forward(N);
    let pad = N / 2;
    let total = x.len() + pad + N;
    let at = |i: usize| if i >= pad && i - pad < x.len() { x[i - pad] } else { 0.0 };
    let tau: Vec<f32> = fc.iter().map(|&f| 0.008 + 100.0 / f.max(50.0) * (0.03 - 0.008)).collect();
    let decay: Vec<f32> = tau.iter().map(|t| (-(HOP as f32) / rate / t).exp()).collect();
    let mut exc = Vec::new();
    let mut holds = Vec::new();
    let mut prev = vec![0.0f32; nband];
    let mut s = 0;
    while s + N <= total {
        let mut buf: Vec<Complex32> = (0..N).map(|k| Complex32::new(at(s + k) * win[k], 0.0)).collect();
        fft.process(&mut buf);
        let mut b = vec![1e-14f32; nband];
        for k in 0..nb {
            if let Some(j) = band_of[k] {
                b[j] += buf[k].norm_sqr() * weight[k];
            }
        }
        // ~dB SPL (full scale ≈ 96 dB), then spreading.
        let l: Vec<f32> = b.iter().map(|v| 10.0 * v.log10() + 96.0).collect();
        let top = l.iter().copied().fold(f32::MIN, f32::max);
        holds.push(l.iter().map(|v| *v > top - 30.0 && *v > 0.0).collect());
        let mut e = vec![0.0f32; nband];
        for j in 0..nband {
            let up = 24.0 + 230.0 / fc[j].max(50.0) - 0.2 * l[j];
            for (m, ev) in e.iter_mut().enumerate() {
                let dz = centres_z[m] - centres_z[j];
                let sp = if dz < 0.0 { 27.0 * dz } else { -up * dz };
                *ev += 10f32.powf((l[j] + sp) / 10.0);
            }
        }
        for j in 0..nband {
            let v = e[j] + quiet[j];
            prev[j] = v.max(decay[j] * prev[j] + (1.0 - decay[j]) * v);
        }
        exc.push(prev.clone());
        s += HOP;
    }
    Ear { centres: fc, exc, quiet, holds }
}

/// Zwicker-style specific loudness of excitation `e` over threshold `q`.
fn loud(e: f32, q: f32) -> f32 {
    ((q / 4.0).powf(0.23) * ((1.0 + e.max(0.0) / q).powf(0.23) - 1.0)).max(0.0)
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

#[derive(Debug, Clone)]
pub struct Heard {
    pub added: f32,
    pub missing: f32,
    /// (Hz, ms from, ms to, share) of the largest added / missing regions.
    pub added_at: Vec<(f32, f32, f32, f32)>,
    pub missing_at: Vec<(f32, f32, f32, f32)>,
    pub flutter: f32,
    pub moddiff: f32,
    pub swell: f32,
    /// band (Hz) with the largest flutter difference and its sign
    pub flutter_at: f32,
    /// `roughness` copy − original (dB): strikes blurred into a buzz read negative.
    pub rough: f32,
    /// `ring_diff` (octaves) of the loops, set by `listen` (0 from `hear` alone).
    pub ring: f32,
}

/// Ring difference (octaves) of two loops: per third-octave band within 30 dB of the original's loudest,
/// |log2(copy ring index / original ring index)| (`shaker::bands`; ≤ 2), weighted by the original's band power, signed
/// by the weighted direction (+ rings longer). Water is a hissing bed under droplets ringing at Q ≈ 35; a copy
/// ringing evenly everywhere, or with droplets ringing on, sounds "not watery" — the frame score and the ear model's
/// spectra cannot see it.
pub fn ring_diff(orig: &[f32], copy: &[f32], rate: f32) -> f32 {
    let (a, b) = (crate::shaker::bands(orig, rate), crate::shaker::bands(copy, rate));
    let top = a.iter().map(|r| r.power).fold(0.0, f32::max);
    let (mut w, mut sum, mut dir) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(&b) {
        if x.power < top * 1e-3 {
            continue;
        }
        let d = (y.index.max(0.2) / x.index.max(0.2)).log2().clamp(-2.0, 2.0);
        if std::env::var_os("GMFIT_RING_TRACE").is_some() {
            eprintln!("{:6.0} Hz  orig {:4.2} copy {:4.2}  {:+5.2} oct  {:6.1} dB", x.hz, x.index, y.index, d, 10.0 * x.power.log10());
        }
        w += x.power;
        sum += x.power * d.abs();
        dir += x.power * d;
    }
    if w <= 0.0 {
        0.0
    } else {
        sum / w * if dir >= 0.0 { 1.0 } else { -1.0 }
    }
}

/// `hear` on the played sounds at bank `speed`, plus the ring difference of the loops at the stored rate — for
/// unpitched originals (`pitched` false) with a loop ≥ 50 ms; 0 otherwise: single hits are the envelope's business,
/// and a periodic tone's envelope never decorrelates (its index reads harmonic beating — approved piano 293: 1.5).
pub fn listen(orig: &[f32], synth: &[f32], m: &gmgen::render::WaveMeta, speed: f32, pitched: bool) -> Heard {
    let (a, b) = (crate::audio::played(orig, m, 1.0), crate::audio::played(synth, m, 1.0));
    let rate = m.rate as f32;
    let mut h = hear(&at_speed(&a, speed), &at_speed(&b, speed), rate);
    if let Some((s, l)) = crate::audio::loop_of(m) {
        if !pitched && l as f32 >= 0.05 * rate && orig.len() >= s + l && synth.len() >= s + l {
            h.ring = ring_diff(&orig[s..s + l], &synth[s..s + l], rate);
        }
    }
    h
}

/// Fine-time roughness (dB): in 4 bands (0.3–0.8, 0.8–2, 2–4.5, 4.5–10 kHz), the RMS deviation of the 2 ms envelope
/// (dB) from its 50 ms moving mean over the frames within 40 dB of the band's peak, weighted by band energy. The
/// 11.6 ms frames of the ear model blur strikes 13 ms apart (a vibraslap heard "like a motor": the copy's strikes
/// had merged, 3.1 dB smoother; the approved pilot waves differ by ≤ 0.5 dB).
pub fn roughness(x: &[f32], rate: f32) -> f32 {
    let n = x.len();
    if n < (0.02 * rate) as usize {
        return 0.0;
    }
    let mut spec: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    crate::plan::forward(n).process(&mut spec);
    let inv = crate::plan::inverse(n);
    let fr = ((0.002 * rate) as usize).max(1);
    let (mut acc, mut wsum) = (0.0f32, 0.0f32);
    for (lo, hi) in [(300.0f32, 800.0f32), (800.0, 2000.0), (2000.0, 4500.0), (4500.0, 10000.0)] {
        let mut b: Vec<Complex32> = spec
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let f = k.min(n - k) as f32 * rate / n as f32;
                if f >= lo && f < hi { *c } else { Complex32::new(0.0, 0.0) }
            })
            .collect();
        inv.process(&mut b);
        let e: Vec<f32> = b.chunks(fr).filter(|c| c.len() == fr).map(|c| (c.iter().map(|v| v.re * v.re).sum::<f32>() / fr as f32).sqrt() / n as f32).collect();
        let energy: f32 = e.iter().map(|v| v * v).sum();
        let d: Vec<f32> = e.iter().map(|v| 20.0 * v.max(1e-12).log10()).collect();
        let k = 25.min(d.len()).max(1);
        let top = d.iter().copied().fold(f32::MIN, f32::max);
        let (mut s2, mut cnt) = (0.0f32, 0usize);
        for t in 0..d.len() {
            let (a, z) = (t.saturating_sub(k / 2), (t + k / 2 + 1).min(d.len()));
            let m = d[a..z].iter().sum::<f32>() / (z - a) as f32;
            if d[t] > top - 40.0 {
                s2 += (d[t] - m).powi(2);
                cnt += 1;
            }
        }
        if cnt > 10 {
            acc += (s2 / cnt as f32).sqrt() * energy;
            wsum += energy;
        }
    }
    acc / wsum.max(1e-30)
}

/// Band-limited resample to play at `speed` (pitch follows, as the sampler does): the spectrum is zero-padded
/// (slower) or truncated (faster) in one FFT. Linear interpolation mirrored content above the new band edge (a ×0.38
/// bird "missed" 8–10 kHz that neither version has).
pub fn at_speed(x: &[f32], speed: f32) -> Vec<f32> {
    if (speed - 1.0).abs() < 1e-4 || x.is_empty() {
        return x.to_vec();
    }
    let n = x.len();
    let m = ((n as f32 / speed) as usize).max(2);
    let mut buf: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    crate::plan::forward(n).process(&mut buf);
    let mut out = vec![Complex32::new(0.0, 0.0); m];
    let half = n.min(m) / 2;
    for k in 0..half {
        out[k] = buf[k];
        if k > 0 {
            out[m - k] = buf[n - k];
        }
    }
    crate::plan::inverse(m).process(&mut out);
    out.iter().map(|c| c.re / n as f32).collect()
}

/// What a listener hears of the difference between `orig` and `copy` (same rate).
pub fn hear(orig: &[f32], copy: &[f32], rate: f32) -> Heard {
    let (a, b) = (excitation(orig, rate), excitation(copy, rate));
    let n = a.exc.len().min(b.exc.len());
    let nb = a.centres.len();
    let la: Vec<Vec<f32>> = a.exc[..n].iter().map(|f| f.iter().map(|v| 10.0 * v.log10()).collect()).collect();
    let lb: Vec<Vec<f32>> = b.exc[..n].iter().map(|f| f.iter().map(|v| 10.0 * v.log10()).collect()).collect();
    // Tolerance: JND + 1.4 × robust frame-to-frame fluctuation of the original (median |Δ| over ±100 ms).
    let k = ((0.1 * rate / HOP as f32) as usize).max(1);
    let jump: Vec<Vec<f32>> = (0..n).map(|t| (0..nb).map(|j| if t == 0 { 0.0 } else { (la[t][j] - la[t - 1][j]).abs() }).collect()).collect();
    let mut tol = vec![vec![0.0f32; nb]; n];
    for j in 0..nb {
        for t in 0..n {
            let mut w: Vec<f32> = (t.saturating_sub(k)..(t + k + 1).min(n)).map(|u| jump[u][j]).collect();
            tol[t][j] = 3.0 + 1.4 * median(&mut w) * 1.48 / std::f32::consts::SQRT_2;
        }
    }
    let q = &a.quiet;
    let mut total = 0.0f32;
    let (mut add, mut miss) = (vec![vec![0.0f32; nb]; n], vec![vec![0.0f32; nb]; n]);
    // Attack (first `ATTACK_S`): frame-aligned — when it happens is heard. After it: per band, the level distribution
    // (sorted levels of both paired by rank) — a texture is heard by its statistics, not by when each grain falls
    // (McDermott & Simoncelli 2011); a reseeded shaker differs frame by frame by up to 0.9 of the loudness. Each pair is
    // filed at the original frame holding that rank, so locations read "when the original is that loud".
    let t_att = ((ATTACK_S * rate / HOP as f32) as usize).min(n);
    let mut pairs: Vec<Vec<(usize, f32, f32)>> = vec![Vec::with_capacity(n); nb]; // (frame, la, lb)
    for (j, pj) in pairs.iter_mut().enumerate() {
        pj.extend((0..t_att).map(|t| (t, la[t][j], lb[t][j])));
        let mut ia: Vec<usize> = (t_att..n).collect();
        ia.sort_by(|x, y| la[*x][j].total_cmp(&la[*y][j]));
        let mut vb: Vec<f32> = (t_att..n).map(|t| lb[t][j]).collect();
        vb.sort_by(f32::total_cmp);
        pj.extend(ia.iter().zip(&vb).map(|(&t, &v)| (t, la[t][j], v)));
    }
    for (j, pj) in pairs.iter().enumerate() {
        for &(t, va, vb) in pj {
            let ea = a.exc[t][j];
            total += loud(ea, q[j]);
            let tl = if t < t_att { tol[t][j] } else { 3.0 + DIST_TOL / 1.4 * (tol[t][j] - 3.0) };
            let d = vb - va;
            if d > tl {
                add[t][j] = loud(ea * (10f32.powf((d - tl) / 10.0) - 1.0), q[j]);
            } else if -d > tl {
                miss[t][j] = loud(ea * (1.0 - 10f32.powf(-(-d - tl) / 10.0)), q[j]);
            }
        }
    }
    let total = total.max(1e-9);
    let sum = |m: &Vec<Vec<f32>>| m.iter().flatten().sum::<f32>() / total;
    let fms = HOP as f32 / rate * 1000.0;
    // Largest regions: per band, the frames holding the middle 80 % of that band's share.
    // Only where either version holds sound itself: a loss at 3 kHz spreads its excitation far upward (shallow upper
    // slope at high level), and the top bands would collect the largest share.
    let held = |t: usize, j: usize| a.holds[t][j] || b.holds[t][j];
    let regions = |m: &Vec<Vec<f32>>| -> Vec<(f32, f32, f32, f32)> {
        let mut bands: Vec<(usize, f32)> =
            (0..nb).map(|j| (j, (0..n).filter(|&t| held(t, j)).map(|t| m[t][j]).sum::<f32>())).collect();
        bands.sort_by(|x, y| y.1.total_cmp(&x.1));
        bands
            .iter()
            .take(3)
            .filter(|b| b.1 > 0.0)
            .map(|&(j, s)| {
                let mut acc = 0.0;
                let (mut t0, mut t1) = (0, n);
                for t in 0..n {
                    acc += if held(t, j) { m[t][j] } else { 0.0 };
                    if acc < 0.1 * s {
                        t0 = t + 1;
                    }
                    if acc >= 0.9 * s && t1 == n {
                        t1 = t;
                    }
                }
                (a.centres[j], t0 as f32 * fms, (t1 + 1) as f32 * fms, s / total)
            })
            .collect()
    };
    // Modulation: loudness^0.3 per band.
    let prof = |e: &Ear| -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let nx: Vec<Vec<f32>> = e.exc[..n].iter().map(|f| f.iter().zip(&e.quiet).map(|(v, qq)| loud(*v, *qq).powf(0.3)).collect()).collect();
        let mut d = vec![0.0f32; nb];
        let mut m = vec![0.0f32; nb];
        let mut sw = vec![0.0f32; nb];
        for j in 0..nb {
            let col: Vec<f32> = nx.iter().map(|f| f[j]).collect();
            let mean = col.iter().sum::<f32>() / n.max(1) as f32 + 1e-6;
            m[j] = mean;
            d[j] = col.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (n.max(2) - 1) as f32 / mean;
            // Envelope spectrum shares (DFT of the band envelope).
            let len = col.len().next_power_of_two().max(2);
            let mut buf: Vec<Complex32> = (0..len).map(|t| Complex32::new(col.get(t).map_or(0.0, |v| v - mean), 0.0)).collect();
            crate::plan::forward(len).process(&mut buf);
            let fr = rate / HOP as f32;
            let (mut slow, mut all) = (0.0f32, 0.0f32);
            for (kk, c) in buf.iter().enumerate().take(len / 2) {
                let fm = kk as f32 * fr / len as f32;
                if fm >= 2.0 {
                    all += c.norm_sqr();
                    if fm < 20.0 {
                        slow += c.norm_sqr();
                    }
                }
            }
            sw[j] = slow / all.max(1e-20);
        }
        (d, sw, m)
    };
    let (da, sa, ma) = prof(&a);
    let (db, sb, _) = prof(&b);
    let wsum = ma.iter().sum::<f32>().max(1e-9);
    let w: Vec<f32> = ma.iter().map(|v| v / wsum).collect();
    let flutter = (0..nb).map(|j| w[j] * (db[j] - da[j])).sum();
    let moddiff = (0..nb).map(|j| w[j] * (db[j] - da[j]).abs() / (da[j] + 0.3)).sum();
    let swell = (0..nb).map(|j| w[j] * (sb[j] - sa[j])).sum();
    // Named band: the largest weighted difference among bands that hold sound themselves most of the time.
    let held_frac: Vec<f32> = (0..nb).map(|j| (0..n).filter(|&t| held(t, j)).count() as f32 / n.max(1) as f32).collect();
    let jf = (0..nb)
        .filter(|&j| held_frac[j] >= 0.5)
        .max_by(|x, y| (w[*x] * (db[*x] - da[*x]).abs()).total_cmp(&(w[*y] * (db[*y] - da[*y]).abs())))
        .unwrap_or(0);
    Heard {
        added: sum(&add),
        missing: sum(&miss),
        added_at: regions(&add),
        missing_at: regions(&miss),
        flutter,
        moddiff,
        swell,
        flutter_at: a.centres[jf] * if db[jf] >= da[jf] { 1.0 } else { -1.0 },
        rough: roughness(copy, rate) - roughness(orig, rate),
        ring: 0.0,
    }
}

impl Heard {
    pub fn json(&self) -> Value {
        let r = |v: &Vec<(f32, f32, f32, f32)>| -> Vec<Value> {
            v.iter().map(|x| json!({"hz": x.0.round(), "ms": [x.1.round(), x.2.round()], "share": crate::score::r4(x.3)})).collect()
        };
        json!({
            "added": crate::score::r4(self.added), "missing": crate::score::r4(self.missing),
            "added_at": r(&self.added_at), "missing_at": r(&self.missing_at),
            "flutter": crate::score::r4(self.flutter), "moddiff": crate::score::r4(self.moddiff), "swell": crate::score::r4(self.swell),
            "flutter_band_hz": self.flutter_at.abs().round(), "flutter_band_more": self.flutter_at >= 0.0,
            "rough": crate::score::r4(self.rough), "ring": crate::score::r4(self.ring),
            "verdict": self.verdict(),
        })
    }

    /// Plain-language findings above the noise floor.
    pub fn verdict(&self) -> Vec<String> {
        let mut v = Vec::new();
        let loc = |x: &Vec<(f32, f32, f32, f32)>| x.iter().map(|r| format!("{:.0} Hz {:.0}–{:.0} ms", r.0, r.1, r.2)).collect::<Vec<_>>().join(", ");
        if self.added > AUDIBLE {
            v.push(format!("audible extra sound ({:.0} % of the loudness) at {}", 100.0 * self.added, loc(&self.added_at)));
        }
        if self.missing > AUDIBLE {
            v.push(format!("audible missing sound ({:.0} %) at {}", 100.0 * self.missing, loc(&self.missing_at)));
        }
        if self.moddiff > MOD_AUDIBLE {
            v.push(format!(
                "{} loudness fluctuation (strongest at {:.0} Hz)",
                if self.flutter > 0.0 { "more" } else { "less" },
                self.flutter_at.abs()
            ));
        }
        if self.rough.abs() > ROUGH_AUDIBLE {
            v.push(if self.rough < 0.0 {
                format!("smoother by {:.1} dB: strikes or grains blurred together (a buzz, \"like a motor\")", -self.rough)
            } else {
                format!("rougher by {:.1} dB: grainier or clickier than the original", self.rough)
            });
        }
        if self.ring.abs() > RING_AUDIBLE {
            v.push(if self.ring < 0.0 {
                format!("rings {:.1} octaves too briefly: droplets/rings heard as hiss (\"not watery\")", -self.ring)
            } else {
                format!("rings {:.1} octaves too long: tones where the original hisses or patters", self.ring)
            });
        }
        if self.swell.abs() > 0.03 {
            v.push(format!("{} slow swell/shimmer (2–20 Hz)", if self.swell > 0.0 { "more" } else { "less" }));
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, seed: u32) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                s as f32 / u32::MAX as f32 * 2.0 - 1.0
            })
            .collect()
    }
    fn lowpass(x: &[f32]) -> Vec<f32> {
        let mut y = 0.0;
        x.iter().map(|v| {
            y += 0.3 * (v - y);
            y * 0.5
        }).collect()
    }

    #[test]
    fn resample_keeps_pitch_ratio_and_level() {
        let x: Vec<f32> = (0..22050).map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / 22050.0).sin()).collect();
        let y = at_speed(&x, 0.5);
        assert_eq!(y.len(), 44100);
        let zc = y[1000..23050].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((495..=505).contains(&zc), "{zc} upward crossings in 1 s (want 500)");
        let rms = (y[1000..43000].iter().map(|v| v * v).sum::<f32>() / 42000.0).sqrt();
        assert!((rms - 0.7071).abs() < 0.02, "rms {rms}");
    }

    #[test]
    fn identical_is_silent() {
        let x = lowpass(&noise(22050, 3));
        let h = hear(&x, &x, 22050.0);
        assert!(h.added < 1e-6 && h.missing < 1e-6 && h.moddiff < 1e-6, "{h:?}");
    }

    #[test]
    fn same_texture_noise_is_below_the_floor() {
        let (a, b) = (lowpass(&noise(22050, 3)), lowpass(&noise(22050, 99)));
        let h = hear(&a, &b, 22050.0);
        assert!(h.added + h.missing < AUDIBLE && h.moddiff < MOD_AUDIBLE, "{h:?}");
    }

    /// Sparse decaying 1.9 kHz pings at random times after the attack (a bubble texture).
    fn pings(seed: u32, gain: f32) -> Vec<f32> {
        let r = noise(64, seed);
        let mut x = vec![0.0f32; 22050];
        for (k, v) in r.iter().take(40).enumerate() {
            let at = 3400 + ((v * 0.5 + 0.5) * 18000.0) as usize;
            let amp = gain * (0.5 + 0.5 * r[(k + 40) % 64].abs());
            for t in 0..2000.min(22050 - at) {
                x[at + t] += amp * (-(t as f32) / 400.0).exp() * (std::f32::consts::TAU * 1900.0 * t as f32 / 22050.0).sin();
            }
        }
        x
    }

    #[test]
    fn texture_heard_by_statistics_not_grain_times() {
        let h = hear(&pings(3, 0.3), &pings(77, 0.3), 22050.0);
        assert!(h.added + h.missing < AUDIBLE, "{h:?}");
        let quiet = hear(&pings(3, 0.3), &pings(77, 0.1), 22050.0);
        assert!(quiet.missing > AUDIBLE, "{quiet:?}");
    }

    /// A strike train (13 ms apart, 4 ms decay) against the same strikes smeared into a wash: smoother by > 1 dB;
    /// the train against itself reseeded: under the threshold.
    #[test]
    fn roughness_sees_blurred_strikes() {
        let train = |seed: u32, decay: f32| {
            let r = noise(22050, seed);
            let mut x = vec![0.0f32; 22050];
            for k in 0..70 {
                let at = 300 + k * 287;
                for t in 0..1200.min(22050 - at) {
                    x[at + t] += r[at + t] * (-(t as f32) / (decay * 22050.0)).exp();
                }
            }
            x
        };
        let (a, b, wash) = (train(3, 0.004), train(9, 0.004), train(3, 0.03));
        let same = hear(&a, &b, 22050.0).rough;
        assert!(same.abs() < ROUGH_AUDIBLE, "reseeded train {same}");
        let h = hear(&a, &wash, 22050.0);
        assert!(h.rough < -ROUGH_AUDIBLE, "wash {}", h.rough);
        assert!(h.verdict().iter().any(|v| v.contains("smoother")), "{:?}", h.verdict());
    }

    #[test]
    fn missing_tone_is_found_where_it_is() {
        let base = lowpass(&noise(22050, 3));
        let with: Vec<f32> = base.iter().enumerate().map(|(i, v)| v + 0.2 * (std::f32::consts::TAU * 8000.0 * i as f32 / 22050.0).sin()).collect();
        let h = hear(&with, &base, 22050.0);
        // Audible, and located at the tone and the bands just above it (excitation spreads upward).
        assert!(h.missing > AUDIBLE, "{h:?}");
        assert!(h.missing_at.iter().all(|r| (6500.0..10500.0).contains(&r.0)), "{:?}", h.missing_at);
        assert!(h.added < AUDIBLE);
    }
}
