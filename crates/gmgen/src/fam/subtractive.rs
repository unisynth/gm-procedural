//! `subtractive`: two band-limited oscillators (saw / pulse / triangle as harmonic series) through a resonant RBJ
//! filter (low/high/band-pass) whose cutoff moves `fenv` octaves with its own attack `fa` and decay `fd` envelope.
//! The filter acts per harmonic (complex response at the harmonic's frequency), recomputed every `CR` samples and
//! interpolated between, so no harmonic ever crosses Nyquist and the result is free of filter-state clicks.
//! Sustain: cutoff frozen at the loop start, every partial on the loop grid (integer phasors) — exactly periodic.
//! `ph2` starts oscillator 2 off phase: with a one-bin detune a pair beats once per loop, and `ph2` places the beat.
use crate::math::{cos_cycles, db, exp, exp2, sin_cycles};
use crate::params::Layer;
use crate::render::{layer_env, Biquad, Ctx, Kind, Phasor};
use alloc::vec::Vec;

const CR: usize = 32;
/// Most harmonics per oscillator.
const MAX_K: u32 = 256;

/// Harmonic `k` amplitude of waveform `w` (0 saw, 1 pulse of width `pw`, 2 triangle); sine phase.
fn amp(w: f32, pw: f32, k: u32) -> f32 {
    let kf = k as f32;
    match w as i32 {
        1 => 2.0 * sin_cycles(0.5 * kf * pw.clamp(0.01, 0.99)) / (core::f32::consts::PI * kf) * 2.0,
        2 => {
            if k % 2 == 0 {
                0.0
            } else {
                let s = if (k / 2) % 2 == 0 { 1.0 } else { -1.0 };
                s * 8.0 / (core::f32::consts::PI * core::f32::consts::PI * kf * kf)
            }
        }
        _ => 2.0 / (core::f32::consts::PI * kf),
    }
}

fn kind(v: f32) -> Kind {
    match v as i32 {
        1 => Kind::Highpass,
        2 => Kind::Bandpass,
        _ => Kind::Lowpass,
    }
}

/// Filter-envelope value (0…1): linear attack `fa`, then exponential decay with time constant `fd` back to 0.
fn fenv(t: f32, fa: f32, fd: f32) -> f32 {
    if t < fa {
        t / fa.max(1e-6)
    } else {
        exp(-(t - fa) / fd.max(1e-6))
    }
}

/// Partials: (phasor, amplitude) for both oscillators.
fn partials(ctx: &Ctx, l: &Layer, f0: f32) -> Vec<(Phasor, f32)> {
    let mut out = Vec::new();
    let osc2 = db(l.get("mix2"));
    let f2 = f0 * exp2((l.get("oct2") * 100.0 + l.get("detune")) / 1200.0);
    for (f, w, pw, g, ph) in [(f0, l.get("wave1"), l.get("pw1"), 1.0, 0.0), (f2, l.get("wave2"), l.get("pw2"), osc2, l.get("ph2"))] {
        if g < 1e-4 || f <= 0.0 {
            continue;
        }
        let n = ctx.grid(f);
        if n == 0 {
            continue;
        }
        for k in 1..=MAX_K {
            let nk = n as u64 * k as u64;
            if ctx.hz(nk.min(u32::MAX as u64) as u32) >= 0.45 * ctx.rate {
                break;
            }
            let a = amp(w, pw, k) * g;
            if a.abs() > 1e-6 {
                let mut p = Phasor::new((nk % ctx.den as u64) as u32, ctx.den);
                // Start phase: harmonic k of an oscillator starting at `ph` cycles starts at k·ph.
                p.acc = (crate::math::fract(ph * k as f32) * ctx.den as f32) as u32 % ctx.den;
                out.push((p, a));
            }
        }
    }
    out
}

pub fn render(ctx: &Ctx, l: &Layer, out: &mut [f32]) {
    let f0 = ctx.f0 * exp2(l.get("cents") / 1200.0);
    let mut ps = partials(ctx, l, f0);
    if ps.is_empty() {
        return;
    }
    let level = db(l.get("level_db"));
    let (cut, q, fk, amount, fa, fd) = (l.get("cutoff"), l.get("res"), kind(l.get("kind")), l.get("fenv"), l.get("fa"), l.get("fd"));
    let freqs: Vec<f32> = ps.iter().map(|p| p.0.n as f32 / p.0.den as f32).collect();
    // Complex gain per partial at time t (cutoff frozen at the loop start in the sustain region).
    let gains = |t: f32| -> Vec<(f32, f32)> {
        let hz = (cut * exp2(amount * fenv(t, fa, fd))).clamp(20.0, 0.45 * ctx.rate);
        let b = Biquad::new(fk, hz, q, ctx.rate);
        freqs.iter().map(|w| b.response(*w)).collect()
    };
    let mut g0 = gains(ctx.t(0));

    let n = out.len();
    let mut i = 0;
    while i < n {
        let end = (i + CR).min(n);
        let g1 = gains(ctx.t(end));
        let (e0, e1) = (layer_env(l, ctx.t(i)) * level, layer_env(l, ctx.t(end)) * level);
        for (j, o) in out[i..end].iter_mut().enumerate() {
            let u = j as f32 / CR as f32;
            let mut acc = 0.0;
            for (k, (p, a)) in ps.iter_mut().enumerate() {
                let ph = p.tick();
                // a·(re·sin + im·cos) = a·|H|·sin(phase + arg H)
                let (re, im) = (g0[k].0 + (g1[k].0 - g0[k].0) * u, g0[k].1 + (g1[k].1 - g0[k].1) * u);
                acc += *a * (re * sin_cycles(ph) + im * cos_cycles(ph));
            }
            *o += acc * (e0 + (e1 - e0) * u);
        }
        g0 = g1;
        i = end;
    }
}

#[cfg(test)]
mod tests {
    use crate::params::{Layer, Params};
    use crate::render::{render, WaveMeta};
    use crate::schema::{family, Region};

    pub(super) fn synth(looped: Option<(u32, u32)>, region: Region, set: &[(&str, f32)]) -> Vec<f32> {
        let m = WaveMeta { rate: 32000, len: 32000, unity: 43, fine: 0, looped }; // G2 ≈ 98 Hz
        let mut l = Layer::new(family("subtractive").unwrap(), region);
        // Below `render::soft_knee` (0.9): a resonant saw peaks above full scale at 0 dB.
        l.set("level_db", -12.0);
        for &(k, v) in set {
            l.set(k, v);
        }
        let mut p = Params::default();
        p.layers.push(l);
        render(&m, &p, 1)
    }

    pub(super) fn mag(x: &[f32], hz: f32, rate: f32) -> f32 {
        let n = x.len();
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, v) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (core::f64::consts::TAU * i as f64 / n as f64).cos();
            let ph = core::f64::consts::TAU * hz as f64 * i as f64 / rate as f64;
            re += w * *v as f64 * ph.cos();
            im -= w * *v as f64 * ph.sin();
        }
        (re * re + im * im).sqrt() as f32
    }

    /// Cutoff response: harmonic levels of the filtered saw over the unfiltered one follow the biquad's magnitude
    /// within 1 dB (low-pass 1 kHz, Q 2: the resonant bump and the −12 dB/oct slope).
    #[test]
    fn subtractive_cutoff_response() {
        let f0 = crate::math::midi_hz(43.0);
        let open = synth(None, Region::Whole, &[("cutoff", 15000.0), ("res", 0.5)]);
        let lp = synth(None, Region::Whole, &[("cutoff", 1000.0), ("res", 2.0)]);
        let b = crate::render::Biquad::new(crate::render::Kind::Lowpass, 1000.0, 2.0, 32000.0);
        let bo = crate::render::Biquad::new(crate::render::Kind::Lowpass, 15000.0, 0.5, 32000.0);
        for k in [3u32, 10, 20, 40] {
            let hz = f0 * k as f32;
            let got = 20.0 * (mag(&lp[4000..28000], hz, 32000.0) / mag(&open[4000..28000], hz, 32000.0)).log10();
            let (r, i) = b.response(hz / 32000.0);
            let (ro, io) = bo.response(hz / 32000.0);
            let want = 10.0 * ((r * r + i * i) / (ro * ro + io * io)).log10();
            assert!((got - want).abs() < 1.0, "harmonic {k} ({hz:.0} Hz): {got:.2} dB, biquad {want:.2} dB");
        }
    }

    /// The filter envelope sweeps: harmonics above 1 kHz relative to the fundamental are ≥ 20 dB stronger in the first
    /// 30 ms (cutoff ≈ 400·2^4 Hz) than 0.5 s later (≈ 400 Hz). (A saw's power centroid barely moves: power ∝ 1/k².)
    #[test]
    fn subtractive_filter_envelope_sweeps() {
        let x = synth(None, Region::Whole, &[("cutoff", 400.0), ("res", 1.0), ("fenv", 4.0), ("fa", 0.002), ("fd", 0.1)]);
        let f0 = crate::math::midi_hz(43.0);
        let high = |s: &[f32]| {
            let top: f32 = (11..40).map(|k| mag(s, f0 * k as f32, 32000.0).powi(2)).sum();
            10.0 * (top / mag(s, f0, 32000.0).powi(2)).log10()
        };
        let (early, late) = (high(&x[0..960]), high(&x[16000..17920]));
        assert!(early - late > 20.0, "highs re fundamental: early {early:.1} dB, late {late:.1} dB");
    }

    /// Sustain is exactly periodic over the loop, with a second detuned oscillator and a filter envelope.
    #[test]
    fn subtractive_sustain_loops_exactly() {
        let x = synth(
            Some((4000, 8000)),
            Region::Sustain,
            &[("wave1", 1.0), ("pw1", 0.3), ("mix2", -3.0), ("detune", 9.0), ("cutoff", 800.0), ("res", 4.0), ("fenv", 2.0), ("fd", 0.05)],
        );
        let d = (12000..20000).map(|i| (x[i] - x[i + 8000]).abs()).fold(0.0, f32::max);
        let peak = x[12000..20000].iter().map(|v| v.abs()).fold(0.0, f32::max);
        assert!(peak > 1e-3 && d < 1e-5 * peak.max(1.0) * 10.0, "loop diff {d}, peak {peak}");
    }
}
