//! `subtractive` analysis (task 7.4): synth leads, pads and basses as two oscillators through a resonant filter.
//! Waveform × cutoff jointly, then resonance, a second oscillator (detuned unison or ±1 octave), and the filter
//! envelope, each step on the fit objective with the level recalibrated. Looped waves: one sustain layer (the
//! sustain region renders the whole wave, envelope frozen at the loop); unlooped: one whole layer with the measured
//! decay as its envelope.
use crate::analyze::{calibrate, open_for_fit, rms};
use crate::audio::loop_of;
use gmgen::params::{Layer, Params};
use gmgen::render::WaveMeta;
use gmgen::schema::{family, Region};

/// Harmonic-profile fit of a loop (see `profile_fit`).
#[derive(Debug, Clone)]
pub struct ProfileFit {
    pub wave: f32,
    pub pw: f32,
    pub cutoff: f32,
    pub res: f32,
    /// Beating pair inside harmonic 1: (bins between the lines, second line's level re the first, dB).
    pub pair: Option<(usize, f32)>,
    /// RMS dB error of the fitted profile.
    pub err: f32,
}

/// Harmonic amplitude (linear) of waveform `w` with pulse width `pw` (as the renderer's `amp`).
fn template(w: f32, pw: f32, k: usize) -> f32 {
    let kf = k as f32;
    match w as i32 {
        1 => (2.0 * (std::f32::consts::PI * kf * pw).sin() / (std::f32::consts::PI * kf)).abs(),
        2 => if k % 2 == 0 { 0.0 } else { 8.0 / (std::f32::consts::PI * std::f32::consts::PI * kf * kf) },
        _ => 2.0 / (std::f32::consts::PI * kf),
    }
}

/// Least-squares fit (dB) of the loop's harmonic levels to waveform × pulse width × low-pass (cutoff, Q), over the
/// harmonics within 60 dB of the strongest; a level offset is free. Harmonic groups as `score::loop_distance`
/// (± half a harmonic spacing), so a detuned pair counts once. `None` without at least 4 harmonics.
pub fn profile_fit(lp: &[f32], rate: f32, f0: f32) -> Option<ProfileFit> {
    use rustfft::num_complex::Complex32;
    let n = lp.len();
    let mut b: Vec<Complex32> = lp.iter().map(|v| Complex32::new(*v, 0.0)).collect();
    crate::plan::forward(n).process(&mut b);
    let pw: Vec<f32> = b.iter().take(n / 2 + 1).map(|c| c.norm_sqr()).collect();
    let m = ((n as f32 * f0 / rate).round() as usize).max(1);
    let kmax = ((0.45 * rate / f0) as usize).min(n / 2 / m);
    if m < 2 || kmax < 4 {
        return None;
    }
    let h: Vec<f32> = (1..=kmax)
        .map(|k| 10.0 * pw[(k * m).saturating_sub(m / 2).max(1)..=(k * m + m / 2).min(n / 2)].iter().sum::<f32>().max(1e-20).log10())
        .collect();
    let top = h.iter().copied().fold(f32::MIN, f32::max);
    let used: Vec<usize> = (0..kmax).filter(|&k| h[k] > top - 60.0).collect();
    let mut best: Option<ProfileFit> = None;
    let waves: Vec<(f32, f32)> = std::iter::once((0.0, 0.5))
        .chain([0.5f32, 0.48, 0.465, 0.45, 0.42, 0.4, 0.35, 0.3, 0.25, 0.2, 0.15, 0.1].into_iter().map(|p| (1.0, p)))
        .chain(std::iter::once((2.0, 0.5)))
        .collect();
    for &(w, p) in &waves {
        for mul in [1.5f32, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 40.0, 1000.0] {
            let cutoff = (f0 * mul).min(0.45 * rate);
            for res in [0.5f32, 0.707, 1.2, 2.0, 4.0] {
                let bq = gmgen::render::Biquad::new(gmgen::render::Kind::Lowpass, cutoff, res, rate);
                let model: Vec<f32> = used
                    .iter()
                    .map(|&k| {
                        let (re, im) = bq.response((k + 1) as f32 * f0 / rate);
                        20.0 * template(w, p, k + 1).max(1e-6).log10() + 10.0 * (re * re + im * im).max(1e-20).log10()
                    })
                    .collect();
                // Errors floored like the score: nothing below top − 60 dB matters.
                let off = used.iter().zip(&model).map(|(&k, m)| h[k] - m).sum::<f32>() / used.len() as f32;
                let err = (used
                    .iter()
                    .zip(&model)
                    .map(|(&k, m)| (h[k] - (m + off).max(top - 60.0)).powi(2))
                    .sum::<f32>()
                    / used.len() as f32)
                    .sqrt();
                if best.as_ref().is_none_or(|b| err < b.err) {
                    best = Some(ProfileFit { wave: w, pw: p, cutoff, res, pair: None, err });
                }
            }
        }
    }
    // Beating pair: the two strongest bins inside harmonic 1's group, within 6 dB and ≥ 1 bin apart, both well above
    // the group's floor.
    let (lo, hi) = (m.saturating_sub(m / 2).max(1), (m + m / 2).min(n / 2));
    let mut idx: Vec<usize> = (lo..=hi).collect();
    idx.sort_by(|a, b| pw[*b].total_cmp(&pw[*a]));
    let pair = (idx.len() >= 2).then(|| (idx[0], idx[1])).and_then(|(a, c)| {
        let d = 10.0 * (pw[c] / pw[a].max(1e-20)).log10();
        let floor = 10.0 * (pw[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32).max(1e-20).log10();
        let gap = a.abs_diff(c);
        (d > -6.0 && gap >= 1 && 10.0 * pw[c].max(1e-20).log10() > floor + 10.0).then(|| (gap * if c > a { 1 } else { 1 }, d))
    });
    best.map(|mut b| {
        b.pair = pair;
        b
    })
}

/// Subtractive candidates for wave `i` (empty for unpitched or silent waves). `cur` lends its pitch.
pub fn candidates(i: usize, orig: &[f32], m: &WaveMeta, cur: &Params) -> Vec<Params> {
    let rate = m.rate as f32;
    let f0 = gmgen::render::f0(m, cur);
    if !(20.0..0.3 * rate).contains(&f0) || rms(orig) < 1e-4 {
        return Vec::new();
    }
    let lp = loop_of(m);
    let mid = lp.map_or(orig.len() / 2, |(s, l)| s + l / 2);
    if crate::score::pitch(orig, mid.saturating_sub(1024), rate).is_none() {
        return Vec::new();
    }
    let (region, range) = match lp {
        Some((s, l)) => (Region::Sustain, s..s + l),
        None => (Region::Whole, 0..orig.len()),
    };
    let mut l = Layer::new(family("subtractive").unwrap(), region);
    l.set("a", 0.002);
    l.set("h", 0.0);
    if region == Region::Whole {
        // One-shot: decay time constant from the time the 5 ms envelope takes from its peak to −20 dB (= 2.3 τ).
        let frame = ((0.005 * rate) as usize).max(1);
        let e: Vec<f32> = orig.chunks(frame).map(rms).collect();
        let (kp, pk) = e.iter().enumerate().fold((0, 0.0f32), |a, (k, v)| if *v > a.1 { (k, *v) } else { a });
        let k20 = e[kp..].iter().position(|v| *v < 0.1 * pk).map_or(e.len() - kp, |k| k);
        l.set("a", ((kp as f32 + 0.5) * frame as f32 / rate).max(0.001));
        l.set("d", (k20 as f32 * frame as f32 / rate / 2.3).max(0.005));
        l.set("s", 0.0);
    } else {
        l.set("d", 1.0);
        l.set("s", 1.0);
    }
    l.set("level_db", -12.0);
    open_for_fit(&mut l);
    let mut q = cur.clone();
    q.layers = vec![l];
    q.correction.clear();
    q.tf.clear();
    q.env.clear();
    let target = rms(&orig[range.clone()]);
    let eval = |q: &mut Params| -> f32 {
        calibrate(i, q, 0, target, range.clone());
        if q.validate().is_err() { -1.0 } else { crate::fit::score_of(orig, i, q) }
    };
    let mut best = f32::MIN;
    // Looped: the loop's harmonic profile decides waveform, pulse width and filter by least squares (no renders),
    // and a beating pair inside harmonic 1 sets oscillator 2. Blind render search cannot: without the beat the
    // envelope terms swamp the objective (Square Wave 386 picked a 50 % pulse — no even harmonics — at 2·f0).
    if let Some((s, len)) = lp {
        if let Some(fitp) = profile_fit(&orig[s..s + len], rate, f0) {
            let mut c = q.clone();
            let l = &mut c.layers[0];
            l.set("wave1", fitp.wave);
            l.set("pw1", fitp.pw);
            l.set("cutoff", fitp.cutoff);
            l.set("res", fitp.res);
            if let Some((bins, db_off)) = fitp.pair {
                let step = rate / len as f32;
                l.set("wave2", fitp.wave);
                l.set("pw2", fitp.pw);
                l.set("mix2", db_off);
                l.set("detune", (1200.0 * ((f0 + bins as f32 * step) / f0).log2()).clamp(-100.0, 100.0));
            }
            let phases: &[f32] = if fitp.pair.is_some() { &[0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875] } else { &[0.0] };
            for &ph in phases {
                let mut d = c.clone();
                d.layers[0].set("ph2", ph);
                let sc = eval(&mut d);
                if sc > best {
                    best = sc;
                    q = d;
                }
            }
            if std::env::var_os("GMFIT_TRACE").is_some() {
                eprintln!("subtractive profile: {fitp:?} → objective {best:.4}");
            }
        }
    }
    let steps: [&[(&str, f32)]; 11] = [
        &[("res", 2.0)],
        &[("res", 5.0)],
        &[("res", 10.0)],
        &[("mix2", 0.0), ("detune", 7.0)],
        &[("mix2", 0.0), ("detune", 15.0)],
        &[("mix2", -6.0), ("oct2", 12.0)],
        &[("mix2", -6.0), ("oct2", -12.0)],
        &[("fenv", 2.0), ("fd", 0.05)],
        &[("fenv", 2.0), ("fd", 0.3)],
        &[("fenv", 4.0), ("fd", 0.1)],
        &[("fenv", -2.0), ("fa", 0.2), ("fd", 1.0)],
    ];
    for set in steps {
        let mut c = q.clone();
        for &(k, v) in set {
            c.layers[0].set(k, v);
        }
        if set.iter().any(|s| s.0 == "mix2") {
            // Oscillator 2 is the same waveform (a detuned pair of squares cancels half a period apart: the GM square
            // wave's 15–20 dB beat dips).
            let (w, pw) = (c.layers[0].get("wave1"), c.layers[0].get("pw1"));
            c.layers[0].set("wave2", w);
            c.layers[0].set("pw2", pw);
        }
        // A second oscillator is worth only as much as its best placement: the beat's phase in the loop.
        let phases: &[f32] = if set.iter().any(|s| s.0 == "mix2") { &[0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875] } else { &[-1.0] };
        for &ph in phases {
            let mut c = c.clone();
            if ph >= 0.0 {
                c.layers[0].set("ph2", ph);
            }
            let s = eval(&mut c);
            if s > best {
                best = s;
                q = c;
            }
        }
    }
    if std::env::var_os("GMFIT_TRACE").is_some() {
        let l = &q.layers[0];
        eprintln!(
            "subtractive: wave {} pw {} cutoff {:.0} res {} mix2 {} detune {} oct2 {} fenv {} → objective {best:.4}",
            l.get("wave1"),
            l.get("pw1"),
            l.get("cutoff"),
            l.get("res"),
            l.get("mix2"),
            l.get("detune"),
            l.get("oct2"),
            l.get("fenv")
        );
    }
    vec![q]
}
