//! `ks` analysis (task 7.1): plucked-string candidates. The fundamental's decay gives `t60`; pick position, damping,
//! pluck dynamics, dispersion and burst length are found by coordinate search on the fit objective (2 passes), the
//! level recalibrated each step. Unlooped waves: one whole `ks` layer. Looped waves: a `ks` attack layer up to the
//! loop start, handing over to the current model's sustain layers.
use crate::analyze::{calibrate, open_for_fit, rms};
use crate::audio::loop_of;
use gmgen::params::{Layer, Params};
use gmgen::render::WaveMeta;
use gmgen::schema::{family, Region};
use rustfft::num_complex::Complex32;

/// 60 dB decay time (s) of the band 0.8–1.25·`f0` of `x`: least-squares slope of its 5 ms dB envelope from 10 ms after
/// the peak to 40 dB below it (or the end). `None` when it does not decay.
pub fn t60(x: &[f32], rate: f32, f0: f32) -> Option<f32> {
    let n = x.len();
    if n < (0.05 * rate) as usize {
        return None;
    }
    let mut b: Vec<Complex32> = x.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    crate::plan::forward(n).process(&mut b);
    for (k, c) in b.iter_mut().enumerate() {
        let f = k.min(n - k) as f32 * rate / n as f32;
        if f < 0.8 * f0 || f > 1.25 * f0 {
            *c = Complex32::new(0.0, 0.0);
        }
    }
    crate::plan::inverse(n).process(&mut b);
    let hop = ((0.005 * rate) as usize).max(1);
    let e: Vec<f32> =
        b.chunks(hop).map(|c| 10.0 * (c.iter().map(|v| v.re * v.re).sum::<f32>() / c.len() as f32 / (n * n) as f32).max(1e-20).log10()).collect();
    let (kp, pk) = e.iter().enumerate().fold((0, f32::MIN), |a, (k, v)| if *v > a.1 { (k, *v) } else { a });
    let from = kp + 2;
    let to = e[from.min(e.len())..].iter().position(|v| *v < pk - 40.0).map_or(e.len(), |j| from + j);
    if to < from + 4 {
        return None;
    }
    let pts: Vec<(f32, f32)> = (from..to).map(|k| (k as f32 * hop as f32 / rate, e[k])).collect();
    let m = pts.len() as f32;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / m, sy / m);
    let slope = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f32>() / pts.iter().map(|p| (p.0 - mx).powi(2)).sum::<f32>().max(1e-12);
    (slope < -0.5).then(|| (-60.0 / slope).clamp(0.02, 30.0))
}

/// Plucked-string candidates for wave `i` from the current model `cur` (empty for unpitched waves).
pub fn candidates(i: usize, orig: &[f32], m: &WaveMeta, cur: &Params) -> Vec<Params> {
    let rate = m.rate as f32;
    let f0 = gmgen::render::f0(m, cur);
    if !(20.0..0.4 * rate).contains(&f0) {
        return Vec::new();
    }
    let (region, range) = match loop_of(m) {
        Some((s, _)) if s >= (0.01 * rate) as usize => (Region::Attack, 0..s),
        Some(_) => return Vec::new(),
        None => (Region::Whole, 0..orig.len()),
    };
    // Pitched: the onset's noise defeats the detector, so it reads the middle of the analysed part.
    if crate::score::pitch(orig, range.end / 2, rate).is_none() {
        return Vec::new();
    }
    let target = rms(&orig[range.clone()]);
    if target < 1e-4 {
        return Vec::new();
    }
    let mut l = Layer::new(family("ks").unwrap(), region);
    l.set("t60", t60(&orig[range.clone()], rate, f0).unwrap_or(4.0));
    l.set("a", 0.0005);
    l.set("h", 0.0);
    l.set("d", 30.0);
    l.set("s", 1.0);
    l.set("level_db", -12.0);
    open_for_fit(&mut l);
    let mut q = cur.clone();
    if region == Region::Whole {
        q.layers.clear();
        q.correction.clear();
        q.tf.clear();
        q.env.clear();
    } else {
        q.layers.retain(|x| x.region != Region::Attack);
        // Hand-over at the loop (as `string_attack`; their own decay kept): sustain layers fade in over the last 10 ms of the attack while
        // the string fades out (render.rs), so the free-running string and the grid partials never overlap.
        let st = range.end as f32 / rate;
        for x in q.layers.iter_mut().filter(|x| x.region == Region::Sustain) {
            x.set("dly", (st - 0.01).max(0.0));
            x.set("a", 0.01f32.min(st));
        }
        // The delayed envelope freezes at the loop start elsewhere on its curve: relevel the sustain on the loop.
        if let Some((s, l)) = loop_of(m) {
            let x = crate::render_wave(i, &q);
            let (want, got) = (rms(&orig[s..s + l]), rms(&x[s.min(x.len())..(s + l).min(x.len())]));
            if want > 1e-9 && got > 1e-9 {
                let d = 20.0 * (want / got).log10();
                for x in q.layers.iter_mut().filter(|x| x.region == Region::Sustain) {
                    let v = x.get("level_db");
                    x.set("level_db", (v + d).clamp(-80.0, 24.0));
                }
            }
        }
    }
    if q.layers.len() >= gmgen::schema::MAX_LAYERS {
        return Vec::new();
    }
    q.layers.push(l);
    let k = q.layers.len() - 1;
    // The string carries the attack alone: level on its head (3 ms … a third of it) for looped waves.
    let head = if region == Region::Attack { (0.003 * rate) as usize..(range.end / 3).max((0.003 * rate) as usize + 1) } else { range.clone() };
    let target = rms(&orig[head.clone()]);
    let eval = |q: &mut Params| -> f32 {
        calibrate(i, q, k, target, head.clone());
        if q.validate().is_err() { -1.0 } else { crate::fit::score_of(orig, i, q) }
    };
    let mut best = eval(&mut q);
    // Brightness first, jointly: damping and pluck dynamics only darken a plucked bass enough together.
    for damp in [0.05f32, 0.3, 0.6, 1.0] {
        for dynamics in [150.0f32, 300.0, 600.0, 1200.0, 3000.0, 8000.0, 20000.0] {
            let mut c = q.clone();
            c.layers[k].set("damp", damp);
            c.layers[k].set("dyn", dynamics);
            let s = eval(&mut c);
            if s > best {
                best = s;
                q = c;
            }
        }
    }
    let grid: [(&str, &[f32]); 5] = [
        ("pick", &[0.0, 0.08, 0.15, 0.25, 0.5]),
        ("damp", &[0.05, 0.15, 0.3, 0.5, 0.8, 1.0]),
        ("dyn", &[150.0, 300.0, 500.0, 1000.0, 2000.0, 4000.0, 10000.0, 20000.0]),
        ("stretch", &[0.0, 0.2, 0.5]),
        ("exc", &[0.5, 1.0, 2.0, 4.0]),
    ];
    for _ in 0..2 {
        for (name, vals) in grid {
            for &v in vals {
                let mut c = q.clone();
                c.layers[k].set(name, v);
                let s = eval(&mut c);
                if std::env::var_os("GMFIT_KS_TRACE").is_some() {
                    eprintln!("  ks {name} {v}: {s:.4} level {:.1}", c.layers[k].get("level_db"));
                }
                if s > best {
                    best = s;
                    q = c;
                }
            }
        }
    }
    if std::env::var_os("GMFIT_TRACE").is_some() {
        let l = &q.layers[k];
        eprintln!(
            "ks: f0 {f0:.1} t60 {:.2} pick {} damp {} dyn {} stretch {} exc {} → objective {best:.4}",
            l.get("t60"),
            l.get("pick"),
            l.get("damp"),
            l.get("dyn"),
            l.get("stretch"),
            l.get("exc")
        );
    }
    if let Ok(dir) = std::env::var("GMFIT_KS_DUMP") {
        let wf = gmgen::params_json::WaveFile { wave: i as u32, params: q.clone(), placeholder: false, note: String::new() };
        let _ = std::fs::write(format!("{dir}/ks-{i}.json"), gmgen::params_json::to_json(&wf));
    }
    vec![q]
}

#[cfg(test)]
mod tests {
    /// `t60` reads a 1 s exponential decay of a 220 Hz tone within 10 %.
    #[test]
    fn t60_of_a_decaying_tone() {
        let rate = 22050.0;
        let x: Vec<f32> = (0..44100).map(|n| {
            let t = n as f32 / rate;
            (std::f32::consts::TAU * 220.0 * t).sin() * 10f32.powf(-3.0 * t / 1.0)
        }).collect();
        let t = super::t60(&x, rate, 220.0).unwrap();
        assert!((t - 1.0).abs() < 0.1, "t60 {t}");
    }
}
