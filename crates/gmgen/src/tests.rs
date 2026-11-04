//! Bank-level tests: every wave present, determinism, partial builds, sanity, seamless loops, structure.
use crate::bank::{build, committed, meta, wave_count};
use crate::render::render;
use dls::Dls;
use std::vec::Vec;

/// Spec "Every wave is generated": a missing parameter file fails here, naming the waves.
#[test]
fn every_wave_has_parameters() {
    let missing: Vec<usize> = committed().iter().enumerate().filter(|(_, p)| p.is_none()).map(|(i, _)| i).collect();
    assert!(missing.is_empty(), "waves without assets/gm/waves/NNN.json: {missing:?}");
    assert_eq!(wave_count(), 495);
}

#[test]
fn build_is_deterministic_and_parses() {
    let a = build(None);
    assert_eq!(a, build(None));
    let d = Dls::parse(&a).unwrap();
    assert_eq!((d.instrument_count(), d.wave_count()), (235, 495));
}

/// Spec "Subset for a song".
#[test]
fn partial_build_matches_full_build() {
    let only: Vec<u32> = (289..=298).collect();
    let (full, part) = (build(None), build(Some(&only)));
    let (f, p) = (Dls::parse(&full).unwrap(), Dls::parse(&part).unwrap());
    for i in 0..495 {
        let (wf, wp) = (f.wave(i).unwrap(), p.wave(i).unwrap());
        assert_eq!(wf.len(), wp.len(), "wave {i} length");
        assert_eq!(wf.wsmp, wp.wsmp);
        if only.contains(&i) {
            assert_eq!(wf.data, wp.data, "wave {i} differs from the full build");
        } else {
            assert!(wp.data.iter().all(|b| *b == 0), "wave {i} not silent");
        }
    }
}

/// Spec "Output sanity" + "Loop wrap", on the f32 output of every wave.
#[test]
fn every_wave_is_sane_and_loops_seamlessly() {
    let params = committed();
    let mut problems = Vec::new();
    for (i, p) in params.iter().enumerate() {
        let (Some(p), Some(m)) = (p, meta(i)) else { continue };
        let x = render(&m, p, i as u32);
        if x.iter().any(|v| !v.is_finite()) {
            problems.push(format!("{i}: non-finite"));
            continue;
        }
        let peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        if peak < crate::math::db(-40.0) {
            problems.push(format!("{i}: peak {:.1} dBFS", crate::math::to_db(peak)));
        }
        let mut run = 0;
        for v in &x {
            run = if v.abs() >= 32767.0 / 32768.0 { run + 1 } else { 0 };
            if run > 4 {
                problems.push(format!("{i}: clipping run"));
                break;
            }
        }
        if let Some((s, l)) = m.looped.map(|(s, l)| (s as usize, l as usize)).filter(|&(s, l)| l > 0 && s + l <= x.len()) {
            // Exact periodicity: render one loop past the end and compare with the loop.
            let mut m2 = m;
            m2.len = x.len().max(s + 2 * l);
            let y = render(&m2, p, i as u32);
            let worst = (0..l).map(|j| (y[s + l + j] - y[s + j]).abs()).fold(0.0f32, f32::max);
            if worst > 1e-5 {
                problems.push(format!("{i}: not periodic over the loop (max diff {worst:.2e})"));
            }
        }
    }
    assert!(problems.is_empty(), "{} waves fail:\n{}", problems.len(), problems.join("\n"));
}

/// Spec "Structure diff is empty" (needs `GM_DLS`).
/// Ensemble `spread`: the loop stays exactly periodic and harmonic 1's energy spreads over several grid bins.
#[test]
fn spread_loops_exactly_and_widens_harmonics() {
    use crate::params::{Layer, Params};
    use crate::schema::{family, Region};
    let m = meta(405).unwrap(); // Strings: 9710-sample loop, ≈ 97 periods
    let (s, l) = m.looped.map(|(a, b)| (a as usize, b as usize)).unwrap();
    let mut p = Params::default();
    let mut lay = Layer::new(family("additive").unwrap(), Region::Sustain);
    lay.tables[0] = std::vec![0.0, -6.0, -12.0];
    lay.set("a", 0.001);
    lay.set("level_db", -12.0);
    p.layers.push(lay);
    let plain = render(&m, &p, 405);
    p.layers[0].set("spread", 12.0);
    let x = render(&m, &p, 405);
    // Periodic: one loop length later the samples repeat.
    let ext = {
        let mut mm = m.clone();
        mm.len += l;
        render(&mm, &p, 405)
    };
    for i in s..s + l {
        assert!((ext[i] - ext[i + l]).abs() < 1e-5, "not periodic at {i}");
    }
    // Width: count bins around harmonic 1 within 10 dB of its peak (DFT of the loop).
    let bins = |y: &[f32]| {
        let per = 97usize;
        let mags: Vec<f32> = (per - 20..=per + 20)
            .map(|k| {
                let (mut re, mut im) = (0.0f32, 0.0f32);
                for (n, v) in y[s..s + l].iter().enumerate() {
                    let c = (k * n % l) as f32 / l as f32;
                    re += v * crate::math::cos_cycles(c);
                    im += v * crate::math::sin_cycles(c);
                }
                re * re + im * im
            })
            .collect();
        let top = mags.iter().copied().fold(0.0, f32::max);
        mags.iter().filter(|v| **v > top * 0.1).count()
    };
    assert_eq!(bins(&plain), 1);
    assert!(bins(&x) >= 3, "spread cluster only {} bins", bins(&x));
}

#[test]
fn structure_matches_windows_bank() {
    let Some(path) = std::env::var_os("GM_DLS").filter(|p| !p.is_empty()) else {
        eprintln!("skip: GM_DLS not set");
        return;
    };
    let orig = std::fs::read(path).unwrap();
    let mut a = dls::write::Bank::read(&Dls::parse(&orig).unwrap());
    let generated = build(None);
    let mut b = dls::write::Bank::read(&Dls::parse(&generated).unwrap());
    for (x, y) in a.waves.iter_mut().zip(&mut b.waves) {
        assert_eq!(x.samples.len(), y.samples.len());
        x.samples.clear();
        y.samples.clear();
    }
    assert_eq!(a, b);
}

#[cfg(feature = "std")]
mod pack_and_json {
    use crate::pack::pack;
    use crate::params::{Layer, Params};
    use crate::params_json::{from_json, to_json, WaveFile};
    use crate::schema::{family, Region};
    use dls::write::{Bank, WaveData};
    use dls::Wsmp;

    fn fixture(name: &str, waves: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gmgen-fixture-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("waves")).unwrap();
        let bank = Bank {
            instruments: vec![],
            waves: (0..2)
                .map(|_| WaveData { rate: 22050, wsmp: Wsmp { unity_note: 60, ..Default::default() }, samples: vec![0; 100] })
                .collect(),
        };
        std::fs::write(dir.join("structure.json"), crate::structure::to_json(&bank)).unwrap();
        for (file, text) in waves {
            std::fs::write(dir.join("waves").join(file), text).unwrap();
        }
        dir
    }

    #[test]
    fn over_budget_fails_naming_the_wave() {
        let harm: Vec<String> = (0..240).map(|_| "-6".into()).collect();
        let w = format!(
            r#"{{"wave": 1, "correction": [{}], "layers": [{{"family": "additive", "t": {{"harm": [{h}]}}}}, {{"family": "additive", "t": {{"harm": [{h}]}}}}]}}"#,
            (0..40).map(|_| "0").collect::<Vec<_>>().join(","),
            h = harm.join(",")
        );
        let e = pack(&fixture("budget", &[("001.json", &w)])).err().unwrap();
        assert!(e.contains("wave 1") && e.contains("numbers (max 512)"), "{e}");
    }

    #[test]
    fn too_many_layers_fails() {
        let l = r#"{"family": "noise"}"#;
        let w = format!(r#"{{"wave": 0, "layers": [{l},{l},{l},{l},{l}]}}"#);
        let e = pack(&fixture("layers", &[("000.json", &w)])).err().unwrap();
        assert!(e.contains("wave 0") && e.contains("5 layers"), "{e}");
    }

    #[test]
    fn unknown_family_fails() {
        let w = r#"{"wave": 0, "layers": [{"family": "theremin"}]}"#;
        let e = pack(&fixture("family", &[("000.json", w)])).err().unwrap();
        assert!(e.contains("wave 0") && e.contains("unknown family \"theremin\""), "{e}");
    }

    #[test]
    fn sustain_needs_a_periodic_family() {
        let w = r#"{"wave": 0, "layers": [{"family": "drum", "region": "sustain"}]}"#;
        let e = pack(&fixture("periodic", &[("000.json", w)])).err().unwrap();
        assert!(e.contains("cannot render the sustain region"), "{e}");
    }

    #[test]
    fn missing_wave_is_reported() {
        let w = r#"{"wave": 0, "layers": [{"family": "noise"}]}"#;
        let p = pack(&fixture("missing", &[("000.json", w)])).unwrap();
        assert_eq!(p.missing, vec![1]);
    }

    #[test]
    fn json_round_trip() {
        let mut l = Layer::new(family("additive").unwrap(), Region::Sustain);
        l.set("d", 0.25);
        l.tables[0] = vec![0.0, -6.5, -12.25];
        let mut n = Layer::new(family("noise").unwrap(), Region::Attack);
        n.tables[0] = vec![1000.0, 2.0, -3.0, 4000.0, 1.0, -9.0];
        let mut p = Params::default();
        p.wave[0] = -3.0;
        p.wave_explicit[0] = true;
        p.layers = vec![l, n];
        p.correction = vec![0.0, 1.5];
        let wf = WaveFile { wave: 7, params: p, placeholder: true, note: "a, b".into() };
        let text = to_json(&wf);
        assert_eq!(from_json(&text).unwrap(), wf, "{text}");
        let mut enc = Vec::new();
        wf.params.encode(&mut enc);
        let (dec, rest) = Params::decode(&enc).unwrap();
        assert!(rest.is_empty());
        assert_eq!(dec, wf.params);
    }
}

/// `modal` (design D4l): mode frequency and decay land, a hit train repeats at its period, and both sustain modes
/// (steady sparse tones, folded hit train) loop exactly.
#[test]
fn modal_modes_hits_and_loops() {
    use crate::params::{Layer, Params};
    use crate::render::WaveMeta;
    use crate::schema::{family, Region};
    let rate = 22050u32;
    let one = WaveMeta { rate, len: 8820, unity: 60, fine: 0, looped: None };
    let mut lay = Layer::new(family("modal").unwrap(), Region::Whole);
    lay.tables[0] = std::vec![1000.0, 0.0, 0.1, 0.0];
    lay.set("level_db", -6.0);
    let mut p = Params::default();
    p.layers.push(lay);
    let x = render(&one, &p, 1);
    // Frequency: zero crossings over 0.1–0.2 s → 1000 Hz within 0.5 %.
    let seg = &x[2205..4410];
    let zc = seg.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 / 0.1;
    assert!((zc - 1000.0).abs() < 5.0, "freq {zc}");
    // Decay: RMS ratio between 0.1 s windows 0.1 s apart → e^{-1} within 5 %.
    let rms = |a: usize| (x[a..a + 441].iter().map(|v| v * v).sum::<f32>() / 441.0).sqrt();
    let r = rms(4410) / rms(2205);
    assert!((r - (-1.0f32).exp()).abs() < 0.02, "decay ratio {r}");
    // Hit train: 4 hits every 50 ms → envelope peaks at 0, 50, 100, 150 ms.
    p.layers[0].tables[0] = std::vec![1000.0, 0.0, 0.01, 0.0];
    p.layers[0].set("hits", 4.0);
    p.layers[0].set("period", 0.05);
    let y = render(&one, &p, 1);
    for k in 0..4 {
        let i = (k as f32 * 0.05 * rate as f32) as usize;
        let e = |a: usize| y[a..a + 110].iter().map(|v| v.abs()).fold(0.0, f32::max);
        assert!(e(i) > 4.0 * e(i + 800), "hit {k} not a fresh peak");
    }
    // Sustain loops exactly, steady and folded.
    let looped = WaveMeta { rate, len: 3000, unity: 60, fine: 0, looped: Some((500, 1500)) };
    for hits in [1.0, 3.0] {
        let mut lay = Layer::new(family("modal").unwrap(), Region::Sustain);
        lay.tables[0] = std::vec![2345.0, 0.0, 0.02, 0.25, 7777.0, -6.0, 0.01, 0.0];
        lay.set("hits", hits);
        lay.set("glide", 600.0);
        let mut p = Params::default();
        p.layers.push(lay);
        let x = render(&looped, &p, 1);
        assert!(x.iter().any(|v| v.abs() > 0.05), "silent (hits {hits})");
        for i in 500..1500 {
            assert!((x[i] - x[i + 1500]).abs() < 1e-5, "not periodic at {i} (hits {hits})");
        }
    }
}

/// `modal` trill (design D4m): the instantaneous frequency swings by the set depth at the set rate.
#[test]
fn modal_trill_modulates_frequency() {
    use crate::params::{Layer, Params};
    use crate::render::WaveMeta;
    use crate::schema::{family, Region};
    let one = WaveMeta { rate: 22050, len: 8820, unity: 60, fine: 0, looped: None };
    let mut lay = Layer::new(family("modal").unwrap(), Region::Whole);
    lay.tables[0] = std::vec![2000.0, -6.0, 10.0, 0.0];
    lay.set("trill_hz", 100.0);
    lay.set("trill_cents", 100.0);
    let mut p = Params::default();
    p.layers.push(lay);
    let x = render(&one, &p, 1);
    // Interpolated upward zero crossings → per-cycle frequency: spans about ±6 % (100 cents).
    let seg = &x[2205..6615];
    let cr: Vec<f32> = (1..seg.len()).filter(|&k| seg[k - 1] <= 0.0 && seg[k] > 0.0).map(|k| (k - 1) as f32 + seg[k - 1] / (seg[k - 1] - seg[k])).collect();
    let zc: Vec<f32> = cr.windows(2).map(|w| 22050.0 / (w[1] - w[0])).collect();
    let (lo, hi) = (zc.iter().copied().fold(f32::MAX, f32::min), zc.iter().copied().fold(0.0, f32::max));
    assert!(hi / lo > 1.08 && hi / lo < 1.25, "swing {lo}..{hi}");
}

/// Spec "shaker" (task 7.5): event rate and resonance peaks within 10 % of the set values; sustain loops exactly.
#[test]
fn shaker_rate_resonances_and_loop() {
    use crate::params::{Layer, Params};
    use crate::render::WaveMeta;
    use crate::schema::{family, Region};
    let rate = 22050u32;
    let one = WaveMeta { rate, len: 4 * 22050, unity: 60, fine: 0, looped: None };
    let mut lay = Layer::new(family("shaker").unwrap(), Region::Whole);
    lay.set("rate", 300.0);
    lay.set("d", 30.0);
    // Event rate: the generator's count over 4 s at a steady envelope.
    let ctx = crate::render::Ctx { rate: rate as f32, len: one.len, lp: None, f0: 261.6, den: rate * 64, seed: 7, region: Region::Whole };
    let ev = crate::fam::shaker::events(&ctx, &lay, one.len, &mut crate::math::Rng::new(7), false);
    let per_s = ev.len() as f32 / 4.0;
    assert!((per_s - 300.0).abs() < 30.0, "event rate {per_s}");
    // Resonances: spectrum peaks at 1500 and 4000 Hz (±10 %).
    lay.tables[0] = std::vec![1500.0, 20.0, 0.0, 4000.0, 20.0, 0.0];
    let mut p = Params::default();
    p.layers.push(lay.clone());
    let x = render(&one, &p, 3);
    assert!(x.iter().any(|v| v.abs() > 0.01), "silent");
    let n = 16384;
    let mut pw = std::vec![0.0f64; n / 2];
    for seg in x.chunks_exact(n) {
        for (k, v) in pw.iter_mut().enumerate().skip(1) {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            let w = std::f64::consts::TAU * k as f64 / n as f64;
            for (i, s) in seg.iter().enumerate().step_by(2) {
                re += *s as f64 * (w * i as f64).cos();
                im += *s as f64 * (w * i as f64).sin();
            }
            *v += re * re + im * im;
        }
    }
    let hz = |k: usize| k as f32 * rate as f32 / n as f32;
    let peak_in = |lo: f32, hi: f32| (1..n / 2).filter(|&k| hz(k) >= lo && hz(k) < hi).max_by(|a, b| pw[*a].total_cmp(&pw[*b])).map(hz).unwrap();
    let (a, b) = (peak_in(800.0, 2500.0), peak_in(2500.0, 6000.0));
    assert!((a - 1500.0).abs() < 150.0 && (b - 4000.0).abs() < 400.0, "peaks {a} {b}");
    // Sustain: exactly periodic over the loop.
    let looped = WaveMeta { rate, len: 6000, unity: 60, fine: 0, looped: Some((1000, 2000)) };
    let mut lay = lay;
    lay.region = Region::Sustain;
    lay.set("fjit", 200.0);
    lay.set("chirp", 1.0);
    let mut p = Params::default();
    p.layers.push(lay);
    let x = render(&looped, &p, 5);
    assert!(x.iter().any(|v| v.abs() > 0.01), "sustain silent");
    for i in 1000..4000 {
        assert!((x[i] - x[i + 2000]).abs() < 1e-5, "not periodic at {i}");
    }
}

/// Power spectrum (f64 DFT over `n` samples from `x[at..]`, Hann), bins 0..n/2.
fn power(x: &[f32], at: usize, n: usize) -> std::vec::Vec<f64> {
    (0..n / 2)
        .map(|k| {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            let w = std::f64::consts::TAU * k as f64 / n as f64;
            for i in 0..n {
                let h = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos();
                re += x[at + i] as f64 * h * (w * i as f64).cos();
                im += x[at + i] as f64 * h * (w * i as f64).sin();
            }
            re * re + im * im
        })
        .collect()
}

/// Bessel J_k(x) by its series.
fn bessel(k: i32, x: f64) -> f64 {
    let sign = if k < 0 && k % 2 != 0 { -1.0 } else { 1.0 };
    let k = k.unsigned_abs() as i32;
    let mut s = 0.0;
    let mut fact_m = 1.0;
    for m in 0..30 {
        if m > 0 {
            fact_m *= m as f64;
        }
        let fact_mk: f64 = (1..=(m + k)).map(|v| v as f64).product();
        s += (-1f64).powi(m) / (fact_m * fact_mk) * (x / 2.0).powi(2 * m + k);
    }
    sign * s
}

/// Spec "fm" (task 7.6): sidebands of a 1:1 and a 1:3.5 pair at fc ± k·fm with Bessel levels; the cycle curve sets
/// the pitch; sustain loops exactly.
#[test]
fn fm_sidebands_cycle_and_loop() {
    use crate::params::{Layer, Params};
    use crate::render::WaveMeta;
    use crate::schema::{family, Region};
    let rate = 22050u32;
    let meta = WaveMeta { rate, len: 22050, unity: 69, fine: 0, looped: None };
    // 2 Hz bins: every line (a multiple of 220 Hz) sits on a bin, so the window's scalloping is the same for all.
    let n = 11025;
    let hz = |k: usize| k as f64 * rate as f64 / n as f64;
    let bin = |f: f64| (f * n as f64 / rate as f64).round() as usize;
    let level = |pw: &[f64], f: f64| (bin(f) - 2..=bin(f) + 2).map(|k| pw[k]).fold(0.0, f64::max);
    for (ratio, index) in [(1.0f32, 1.0f32), (3.5, 1.5)] {
        let mut l = Layer::new(family("fm").unwrap(), Region::Whole);
        l.set("freq", 440.0);
        l.set("d", 30.0);
        // carrier [ratio, dB, index decay, index sustain], modulator [ratio, index, decay, sustain]
        l.tables[0] = std::vec![1.0, 0.0, 1.0, 1.0, ratio, index, 1.0, 1.0];
        let mut p = Params::default();
        p.layers.push(l);
        let x = render(&meta, &p, 1);
        let pw = power(&x, 4000, n);
        let (fc, fm) = (440.0f64, 440.0 * ratio as f64);
        // Lines expected at |fc + k·fm|, k = −3..3; amplitude Σ J_k over lines that coincide (1:1 folds).
        let mut lines: std::vec::Vec<(f64, f64)> = std::vec::Vec::new();
        for k in -5i32..=5 {
            let f = (fc + k as f64 * fm).abs();
            let a = bessel(k, index as f64) * if fc + k as f64 * fm < 0.0 { -1.0 } else { 1.0 };
            match lines.iter_mut().find(|(g, _)| (*g - f).abs() < 1.0) {
                Some(e) => e.1 += a,
                None => lines.push((f, a)),
            }
        }
        let c = level(&pw, fc).sqrt();
        let a0 = lines.iter().find(|(f, _)| (*f - fc).abs() < 1.0).unwrap().1.abs();
        for &(f, a) in lines.iter().filter(|(f, a)| *f > 50.0 && a.abs() > 0.05 && *f < 10000.0) {
            let got = 20.0 * (level(&pw, f).sqrt() / c).log10();
            let want = 20.0 * (a.abs() / a0).log10();
            assert!((got - want).abs() < 1.0, "1:{ratio} I={index}: line {f} Hz {got:.2} dB re carrier, theory {want:.2}");
        }
        // Nothing strong off the lines.
        let top = pw.iter().cloned().fold(0.0, f64::max);
        for (k, v) in pw.iter().enumerate().skip(5) {
            if *v > top * 1e-3 {
                assert!(lines.iter().any(|(f, _)| (hz(k) - f).abs() < 12.0), "1:{ratio}: stray line at {} Hz", hz(k));
            }
        }
    }
    // Cycle: plateaus [0, 0, +1200, +1200] cents at 10 Hz (linear between points) → the first quarter of each cycle at
    // 440 Hz, the third at 880 Hz.
    let mut l = Layer::new(family("fm").unwrap(), Region::Whole);
    l.set("freq", 440.0);
    l.set("d", 30.0);
    l.set("cyc_hz", 10.0);
    l.tables[0] = std::vec![1.0, 0.0, 1.0, 1.0];
    l.tables[1] = std::vec![0.0, 0.0, 0.0, 0.0, 1200.0, 0.0, 1200.0, 0.0];
    let mut p = Params::default();
    p.layers.push(l.clone());
    let x = render(&meta, &p, 1);
    let per = 2205;
    let zc = |off: usize| {
        let c: usize = (1..9).map(|k| x[k * per + off..k * per + off + 500].windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()).sum();
        c as f32 / (8.0 * 500.0 / rate as f32)
    };
    let (lo, hi) = (zc(20), zc(1122));
    assert!((lo - 440.0).abs() < 10.0 && (hi - 880.0).abs() < 20.0, "cycle pitch {lo} / {hi}");
    // Sustain: exactly periodic over the loop (cycle, glide frozen, stacked modulator with an odd ratio).
    let looped = WaveMeta { rate, len: 6000, unity: 69, fine: 0, looped: Some((1000, 1486)) };
    l.region = Region::Sustain;
    l.set("cyc_hz", 29.4);
    l.set("glide", -300.0);
    l.tables[0] = std::vec![1.0, 0.0, 1.0, 1.0, 1.37, 0.8, 0.5, 0.5];
    l.tables[1] = std::vec![0.0, 0.0, -900.0, -6.0, -400.0, -30.0];
    let mut p = Params::default();
    p.layers.push(l);
    let x = render(&looped, &p, 5);
    assert!(x.iter().any(|v| v.abs() > 0.01), "sustain silent");
    for i in 1000..4000 {
        assert!((x[i] - x[i + 1486]).abs() < 1e-4, "not periodic at {i}: {} vs {}", x[i], x[i + 1486]);
    }    // With feedback on the top operator (one warm-up loop settles it).
    p.layers[0].set("feedback", 0.8);
    let x = render(&looped, &p, 5);
    for i in 1000..4000 {
        assert!((x[i] - x[i + 1486]).abs() < 1e-4, "feedback: not periodic at {i}: {} vs {}", x[i], x[i + 1486]);
    }
}

/// Harmonic amplitudes of `x[at..at+n]` at k·f0 (k = 1..), by direct projection.
fn harmonics(x: &[f32], at: usize, n: usize, f0: f64, rate: f64, kmax: usize) -> std::vec::Vec<f64> {
    (1..=kmax)
        .map(|k| {
            let w = std::f64::consts::TAU * k as f64 * f0 / rate;
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for i in 0..n {
                re += x[at + i] as f64 * (w * i as f64).cos();
                im += x[at + i] as f64 * (w * i as f64).sin();
            }
            (re * re + im * im).sqrt() * 2.0 / n as f64
        })
        .collect()
}

/// Spec "formant" (task 7.3): the spectral envelope peaks at the formants (whole and sustain), syllables sound at
/// their table times, sustain loops exactly.
#[test]
fn formant_peaks_syllables_and_loop() {
    use crate::params::{Layer, Params};
    use crate::render::WaveMeta;
    use crate::schema::{family, Region};
    let rate = 22050u32;
    let formants = std::vec![600.0f32, 80.0, 0.0, 1400.0, 100.0, 0.0, 2600.0, 150.0, 0.0];
    let check = |x: &[f32], at: usize, n: usize, f0: f64, what: &str| {
        let h = harmonics(x, at, n, f0, rate as f64, (3500.0 / f0) as usize);
        for (lo, hi, want) in [(400.0, 900.0, 600.0), (1100.0, 1900.0, 1400.0), (2200.0, 3200.0, 2600.0)] {
            let k = (1..=h.len()).filter(|k| (lo..hi).contains(&(*k as f64 * f0))).max_by(|a, b| h[a - 1].total_cmp(&h[b - 1])).unwrap();
            let got = k as f64 * f0;
            assert!((got / want - 1.0).abs() < 0.1, "{what}: formant {want} peaks at {got} Hz ({h:?})");
        }
    };
    // Whole: saw at 100 Hz.
    let meta = WaveMeta { rate, len: 22050, unity: 60, fine: 0, looped: None };
    let mut l = Layer::new(family("formant").unwrap(), Region::Whole);
    l.set("f0", 100.0);
    l.set("d", 30.0);
    l.tables[0] = formants.clone();
    let mut p = Params::default();
    p.layers.push(l.clone());
    let x = render(&meta, &p, 1);
    assert!(x.iter().any(|v| v.abs() > 0.01), "silent");
    check(&x, 4410, 8820, 100.0, "whole");
    // Sustain: harmonics on the loop grid (loop 2205 = 10 periods of 100 Hz), exactly periodic, same peaks.
    let looped = WaveMeta { rate, len: 8000, unity: 60, fine: 0, looped: Some((1000, 2205)) };
    let mut s = l.clone();
    s.region = Region::Sustain;
    s.set("vib_rate", 5.0);
    s.set("vib_cents", 20.0);
    let mut p = Params::default();
    p.layers.push(s.clone());
    let x = render(&looped, &p, 1);
    for i in 1000..5000 {
        assert!((x[i] - x[i + 2205]).abs() < 1e-4, "not periodic at {i}");
    }
    s.set("vib_cents", 0.0);
    let mut p = Params::default();
    p.layers.push(s);
    let x = render(&looped, &p, 1);
    check(&x, 1000, 2205, 100.0, "sustain");
    // Syllables at 0.1, 0.3, 0.5 s: 10 ms envelope peaks there, quiet halfway between.
    let mut l = l;
    l.tables[1] = std::vec![0.1, 0.0, 0.0, 0.3, 0.0, 0.0, 0.5, -6.0, 0.0];
    l.set("syl_a", 0.005);
    l.set("syl_d", 0.03);
    let mut p = Params::default();
    p.layers.push(l);
    let x = render(&meta, &p, 1);
    let env: std::vec::Vec<f32> = x.chunks(220).map(|c| (c.iter().map(|v| v * v).sum::<f32>() / 220.0).sqrt()).collect();
    let top = env.iter().copied().fold(0.0f32, f32::max);
    for t in [0.1f32, 0.3, 0.5] {
        let k = (t * 100.0) as usize;
        let near = env[k..k + 4].iter().copied().fold(0.0f32, f32::max);
        assert!(near > 0.4 * top, "no syllable at {t} s");
        let mid = env[k + 10];
        assert!(mid < 0.1 * near, "not quiet after the syllable at {t} s");
    }
}
