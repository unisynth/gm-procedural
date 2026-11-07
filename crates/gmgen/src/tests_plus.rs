//! The gmplus set (feature `plus`, change add-gmplus-bank 2.2): every wave present, deterministic, sane, exact
//! loops at 32 kHz; the gm.dls set (`bank`) is unaffected by the feature.
use crate::plus::{build, committed, meta, wave_count};
use crate::render::render;
use dls::Dls;
use std::vec::Vec;

#[test]
fn every_plus_wave_has_parameters() {
    let missing: Vec<usize> = committed().iter().enumerate().filter(|(_, p)| p.is_none()).map(|(i, _)| i).collect();
    assert!(missing.is_empty(), "waves without assets/gm-plus/waves/NNN.json: {missing:?}");
    // 495 skeleton waves + one partner per stereo wave (change add-gmplus-bank D4).
    assert_eq!(wave_count(), 495 + crate::plus::skeleton().pairs.len());
}

#[test]
fn plus_build_is_deterministic_at_32k() {
    let a = build(None);
    assert_eq!(a, build(None));
    let d = Dls::parse(&a).unwrap();
    assert_eq!((d.instrument_count(), d.wave_count() as usize), (235, wave_count()));
    for i in 0..d.wave_count() {
        assert_eq!(d.wave(i).unwrap().rate, 32000, "wave {i}");
    }
    // Every stereo pair survives the build and validates (equal rate, length and loop).
    let pairs = crate::plus::skeleton().pairs;
    assert!(!pairs.is_empty());
    for (l, r) in pairs {
        assert_eq!(d.partner(l), Some(r), "pair {l}/{r}");
    }
    // The crate-level API builds this set; `bank` stays the gm.dls one.
    assert_eq!(crate::build(None), a);
    assert_eq!(crate::bank::meta(293).unwrap().rate, 22050);
}

#[test]
fn every_plus_wave_is_sane_and_loops_seamlessly() {
    let mut problems = Vec::new();
    for (i, p) in committed().iter().enumerate() {
        let (Some(p), Some(m)) = (p, meta(i)) else { continue };
        let x = render(&m, p, i as u32);
        if x.iter().any(|v| !v.is_finite()) {
            problems.push(std::format!("{i}: non-finite"));
            continue;
        }
        if let Some((s, l)) = m.looped.map(|(s, l)| (s as usize, l as usize)).filter(|&(s, l)| l > 0 && s + l <= x.len()) {
            let mut m2 = m;
            m2.len = x.len().max(s + 2 * l);
            let y = render(&m2, p, i as u32);
            let worst = (0..l).map(|j| (y[s + l + j] - y[s + j]).abs()).fold(0.0f32, f32::max);
            if worst > 1e-5 {
                problems.push(std::format!("{i}: not periodic over the loop (max diff {worst:.2e})"));
            }
        }
    }
    assert!(problems.is_empty(), "{} waves fail:\n{}", problems.len(), problems.join("\n"));
}

/// Generation cost guard (task 2.3): render time scales with samples. 1 s of baked mod-envelope motion, all layered
/// zones summed, crossfaded loops for zones that cannot lock (3.3 review) and 60 stereo partners (4.2) give 14.46 M
/// samples, 9.4× the gm.dls set (gm 6.8 s native, single thread, before any optimisation). Growth past 15 M must be a
/// decision, not an accident.
#[test]
fn plus_sample_budget() {
    let n: usize = (0..wave_count()).map(|i| meta(i).unwrap().len).sum();
    assert!(n <= 15_000_000, "plus set has {n} samples");
}

/// A partner with its left wave's parameters renders the same samples (same seed): coherent stereo (4.3).
#[test]
fn partner_with_left_params_renders_identically() {
    let pairs = crate::plus::skeleton().pairs;
    let (l, r) = pairs[0];
    let p = committed()[l as usize].clone().unwrap();
    assert_eq!(crate::plus::PLUS.render_wave(l as usize, &p), crate::plus::PLUS.render_wave(r as usize, &p));
    assert_eq!(crate::bank::seed_of(&pairs, r as usize), l);
    assert_eq!(crate::bank::seed_of(&pairs, l as usize), l);
}
