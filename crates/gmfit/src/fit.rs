//! `gmfit fit` (CMA-ES over the explicit scalars, then harmonic-table refinement) and `gmfit correct` (analytic
//! harmonic correction, design D4b). Both keep a result only when it scores higher than the starting point.
use crate::audio::loop_of;
use crate::cmaes::minimize;
use gmgen::params::Params;
use gmgen::schema::{Region, FAMILIES, MAX_CORRECTION, MAX_NUMBERS, WAVE};
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;

/// One free parameter: wave scalar (`layer = None`) or layer scalar.
#[derive(Clone, Copy, Debug)]
struct Var {
    layer: Option<usize>,
    idx: usize,
    lo: f32,
    hi: f32,
    log: bool,
}

impl Var {
    fn get(&self, p: &Params) -> f32 {
        match self.layer {
            None => p.wave[self.idx],
            Some(l) => p.layers[l].scalars[self.idx],
        }
    }
    fn set(&self, p: &mut Params, v: f32) {
        match self.layer {
            None => {
                p.wave[self.idx] = v;
                p.wave_explicit[self.idx] = true;
            }
            Some(l) => {
                p.layers[l].scalars[self.idx] = v;
                p.layers[l].explicit[self.idx] = true;
            }
        }
    }
    fn to_unit(&self, v: f32) -> f64 {
        if self.log {
            ((v.max(self.lo).ln() - self.lo.ln()) / (self.hi.ln() - self.lo.ln())) as f64
        } else {
            ((v - self.lo) / (self.hi - self.lo)) as f64
        }
        .clamp(0.0, 1.0)
    }
    fn from_unit(&self, u: f64) -> f32 {
        let u = u as f32;
        if self.log {
            (self.lo.ln() + u * (self.hi.ln() - self.lo.ln())).exp()
        } else {
            self.lo + u * (self.hi - self.lo)
        }
    }
}

/// Free variables: every explicit layer scalar; `only` (names like `0.d`, `gain_db`) narrows it or adds `gain_db`.
fn vars(p: &Params, only: Option<&[String]>) -> Vec<Var> {
    let mut v = Vec::new();
    let keep = |name: String| only.is_none_or(|o| o.contains(&name));
    for (l, layer) in p.layers.iter().enumerate() {
        for (k, s) in FAMILIES[layer.family].scalars.iter().enumerate() {
            if layer.explicit[k] && keep(format!("{l}.{}", s.name)) && s.name != "unison" {
                let log = s.min > 0.0 && s.max / s.min >= 100.0;
                // Narrow pitch-like ranges around the current value: the analysis is already close.
                let (lo, hi) = match s.name {
                    "cents" => ((layer.scalars[k] - 50.0).max(s.min), (layer.scalars[k] + 50.0).min(s.max)),
                    "freq" | "base" | "bp" | "hp" | "lp" => {
                        ((layer.scalars[k] / 2.0).max(s.min), (layer.scalars[k] * 2.0).min(s.max))
                    }
                    _ => (s.min, s.max),
                };
                v.push(Var { layer: Some(l), idx: k, lo, hi, log });
            }
        }
    }
    // `gain_db` duplicates the layer levels; only when asked for.
    if only.is_some_and(|o| o.iter().any(|x| x == "gain_db")) {
        v.push(Var { layer: None, idx: 0, lo: WAVE[0].min.max(-24.0), hi: WAVE[0].max.min(24.0), log: false });
    }
    v
}

/// Score of `p` for wave `i` (1 = identical).
pub fn score_of(orig: &[f32], i: usize, p: &Params) -> f32 {
    crate::evaluate(orig, i, p).0.score
}

fn par_scores(orig: &[f32], i: usize, ps: &[Params]) -> Vec<f32> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|sc| {
        let hs: Vec<_> = ps
            .chunks(ps.len().div_ceil(threads).max(1))
            .map(|c| sc.spawn(move || c.iter().map(|p| if p.validate().is_ok() { score_of(orig, i, p) } else { 0.0 }).collect::<Vec<_>>()))
            .collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

/// CMA-ES over the free scalars, then harmonic refinement. Returns the best parameters and their score.
pub fn fit(orig: &[f32], i: usize, start: &Params, iters: usize, seed: u64, only: Option<&[String]>) -> (Params, f32) {
    let vs = vars(start, only);
    let mut best = start.clone();
    let mut best_score = score_of(orig, i, start);
    if !vs.is_empty() && iters > 0 {
        let x0: Vec<f64> = vs.iter().map(|v| v.to_unit(v.get(start))).collect();
        let build = |x: &[f64]| {
            let mut p = start.clone();
            for (v, u) in vs.iter().zip(x) {
                v.set(&mut p, v.from_unit(*u));
            }
            p
        };
        let (x, f) = minimize(&x0, 0.15, seed, iters, |xs| {
            let ps: Vec<Params> = xs.iter().map(|x| build(x)).collect();
            par_scores(orig, i, &ps).iter().map(|s| 1.0 - *s as f64).collect()
        });
        if (1.0 - f) as f32 > best_score {
            best = build(&x);
            best_score = (1.0 - f) as f32;
        }
    }
    let (p, s) = refine_harmonics(orig, i, &best, best_score);
    refine_bands(orig, i, &p, s)
}

/// Mean power spectrum (Hann 2048, half overlap) of `x`.
fn avg_power(x: &[f32]) -> Vec<f32> {
    let n = 2048;
    let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
    let mut acc = vec![0.0f32; n / 2 + 1];
    let mut start = 0;
    let mut frames = 0;
    while start < x.len().max(1) {
        let mut buf: Vec<Complex32> = (0..n)
            .map(|k| {
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * k as f32 / n as f32).cos();
                Complex32::new(x.get(start + k).copied().unwrap_or(0.0) * w, 0.0)
            })
            .collect();
        fft.process(&mut buf);
        acc.iter_mut().zip(&buf).for_each(|(a, c)| *a += c.norm_sqr());
        frames += 1;
        start += n / 2;
    }
    acc.iter().map(|a| a / frames as f32).collect()
}

/// Moves each `noise` band gain by the band's average-spectrum level error (between neighbouring band centres,
/// geometric edges); 3 passes, each kept only if the score improves.
pub fn refine_bands(orig: &[f32], i: usize, start: &Params, start_score: f32) -> (Params, f32) {
    let m = gmgen::bank::meta(i).unwrap();
    let rate = m.rate as f32;
    let (mut best, mut best_score) = (start.clone(), start_score);
    let po = avg_power(&crate::audio::played(orig, &m, crate::score::PLAYED_SECS));
    for li in 0..best.layers.len() {
        if FAMILIES[best.layers[li].family].name != "noise" || best.layers[li].tables[0].is_empty() {
            continue;
        }
        for _ in 0..3 {
            let synth = gmgen::render_wave(i, &best);
            let pg = avg_power(&crate::audio::played(&synth, &m, crate::score::PLAYED_SECS));
            let mut p = best.clone();
            let centres: Vec<f32> = p.layers[li].tables[0].chunks(3).map(|b| b[0]).collect();
            let bin = |hz: f32| ((hz / rate * 2048.0) as usize).min(1024);
            for (k, b) in p.layers[li].tables[0].chunks_mut(3).enumerate() {
                let lo = if k == 0 { b[0] / 1.5 } else { (centres[k - 1] * b[0]).sqrt() };
                let hi = if k + 1 == centres.len() { (b[0] * 1.5).min(rate / 2.0) } else { (centres[k + 1] * b[0]).sqrt() };
                let (a, z) = (bin(lo), bin(hi).max(bin(lo) + 1));
                let (eo, eg) = (po[a..z].iter().sum::<f32>(), pg[a..z].iter().sum::<f32>());
                if eo > 0.0 && eg > 0.0 {
                    b[2] = (b[2] + (10.0 * (eo / eg).log10()).clamp(-12.0, 12.0)).clamp(-60.0, 24.0);
                }
            }
            let sc = score_of(orig, i, &p);
            if sc > best_score {
                best = p;
                best_score = sc;
            } else {
                break;
            }
        }
    }
    (best, best_score)
}

/// Loop DFT magnitudes (bins 0..=L/2).
fn loop_mag(x: &[f32]) -> Vec<f32> {
    let l = x.len();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(l);
    let mut buf: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    fft.process(&mut buf);
    buf[..=l / 2].iter().map(|c| c.norm()).collect()
}

/// Periods per loop for the wave's pitch.
fn periods(m: &gmgen::render::WaveMeta, p: &Params, l: usize) -> usize {
    let f0 = gmgen::render::f0(m, p);
    ((l as f32 * f0 / m.rate as f32).round() as usize).max(1)
}

/// Moves each sustain `additive` table entry by the loop's harmonic level error (3 passes, kept if better).
pub fn refine_harmonics(orig: &[f32], i: usize, start: &Params, start_score: f32) -> (Params, f32) {
    let m = gmgen::bank::meta(i).unwrap();
    let Some((s, l)) = loop_of(&m) else { return (start.clone(), start_score) };
    let Some(li) = start.layers.iter().position(|x| FAMILIES[x.family].name == "additive" && x.region == Region::Sustain) else {
        return (start.clone(), start_score);
    };
    let om = loop_mag(&orig[s..s + l]);
    let (mut best, mut best_score) = (start.clone(), start_score);
    for _ in 0..3 {
        let synth = gmgen::render_wave(i, &best);
        let gm = loop_mag(&synth[s..s + l]);
        let per = periods(&m, &best, l);
        let mut p = best.clone();
        let t = &mut p.layers[li].tables[0];
        for (k, v) in t.iter_mut().enumerate() {
            let b = ((k + 1) * per).min(l / 2);
            let d = 20.0 * (om[b].max(1e-7) / gm[b].max(1e-7)).log10();
            *v = (*v + d.clamp(-12.0, 12.0)).max(-120.0);
        }
        let sc = score_of(orig, i, &p);
        if sc > best_score {
            best = p;
            best_score = sc;
        } else {
            break;
        }
    }
    (best, best_score)
}

/// Analytic harmonic correction (D4b) on top of `start` (its own correction ignored); kept only if better.
pub fn correct(orig: &[f32], i: usize, start: &Params) -> (Params, f32) {
    let m = gmgen::bank::meta(i).unwrap();
    let start_score = score_of(orig, i, start);
    let mut base = start.clone();
    base.correction.clear();
    let synth = gmgen::render_wave(i, &base);
    let f0 = gmgen::render::f0(&m, &base);
    let rate = m.rate as f32;
    // Harmonic levels of original and model.
    let (oh, gh): (Vec<f32>, Vec<f32>) = match loop_of(&m) {
        Some((s, l)) => {
            let per = periods(&m, &base, l);
            let (om, gm) = (loop_mag(&orig[s..s + l]), loop_mag(&synth[s..s + l]));
            (1..=(l / 2) / per).map(|k| (om[k * per], gm[k * per])).unzip()
        }
        None => {
            let pw = |x: &[f32]| loop_mag(&x[..x.len().min(2048)]);
            let (om, gm) = (pw(orig), pw(&synth));
            let n = orig.len().min(2048) as f32;
            (1..)
                .map(|k| (k as f32 * f0 * n / rate).round() as usize)
                .take_while(|b| *b < om.len())
                .map(|b| (om[b], gm[b]))
                .unzip()
        }
    };
    let room = (MAX_NUMBERS - base.numbers()).min(MAX_CORRECTION);
    let top = oh.iter().chain(&gh).copied().fold(0.0f32, f32::max);
    let mut c: Vec<f32> = oh
        .iter()
        .zip(&gh)
        .take(room)
        .map(|(o, g)| {
            let d = (20.0 * (o.max(1e-7) / g.max(1e-7)).log10()).clamp(-24.0, 24.0);
            if d.abs() < 0.5 || o.max(*g) < top * 1e-4 { 0.0 } else { (d * 10.0).round() / 10.0 }
        })
        .collect();
    while c.last() == Some(&0.0) {
        c.pop();
    }
    let mut p = base;
    p.correction = c;
    let sc = score_of(orig, i, &p);
    if std::env::var_os("GMFIT_TRACE").is_some() {
        eprintln!("correct: {:?} → {sc} (from {start_score})", p.correction);
    }
    if sc > start_score { (p, sc) } else { (start.clone(), start_score) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gmgen::params::Layer;
    use gmgen::schema::family;

    /// Spec "Analytic correction": the analytic table raises the score of a deliberately mistimbred wave.
    /// Wave 289 (looped Piano) is the vehicle; the "original" is the model with a different harmonic table.
    #[test]
    fn correct_fixes_a_mistimbred_model() {
        let i = 289;
        let mut l = Layer::new(family("additive").unwrap(), Region::Sustain);
        l.set("level_db", -12.0);
        l.tables[0] = vec![0.0, -3.0, -6.0, -9.0, -12.0, -15.0, -18.0, -21.0];
        let mut target = Params::default();
        target.layers.push(l.clone());
        let orig = gmgen::render_wave(i, &target);
        l.tables[0] = vec![0.0, -12.0, 0.0, -20.0, -6.0, -15.0, -30.0, -21.0];
        let mut wrong = Params::default();
        wrong.layers.push(l);
        let before = score_of(&orig, i, &wrong);
        let (p, after) = correct(&orig, i, &wrong);
        assert!(!p.correction.is_empty());
        assert!(after > before + 0.05, "{before} → {after}");
    }
}
