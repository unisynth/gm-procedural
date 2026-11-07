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
    let top = 0.45 * rate;
    // Waveform × cutoff: the two decide the spectrum's shape together.
    for (w, pw) in [(0.0f32, 0.5f32), (1.0, 0.5), (1.0, 0.25), (1.0, 0.1), (2.0, 0.5)] {
        for mul in [2.0f32, 4.0, 8.0, 16.0, 40.0] {
            let mut c = q.clone();
            c.layers[0].set("wave1", w);
            c.layers[0].set("pw1", pw);
            c.layers[0].set("cutoff", (f0 * mul).min(top));
            let s = eval(&mut c);
            if s > best {
                best = s;
                q = c;
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
        let s = eval(&mut c);
        if s > best {
            best = s;
            q = c;
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
