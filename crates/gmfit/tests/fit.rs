//! Analysis, fit and correct against the original bank (skip without `GM_DLS`).
fn orig() -> Option<gmfit::Orig> {
    match gmfit::Orig::load() {
        Ok(o) => Some(o),
        Err(_) => {
            eprintln!("skip: GM_DLS not set");
            None
        }
    }
}

/// Task 5.2: the analysed `additive` init of a pure organ wave (Organ 1, wave 150) scores ≥ 0.85. (It reached
/// 0.99 before `d_onset`; the onset term now sees the original's 2–3 kHz key click in the first 10 ms, which the
/// analysis does not model yet.)
#[test]
fn organ_analysis_scores_high() {
    let Some(o) = orig() else { return };
    let m = gmgen::bank::meta(150).unwrap();
    let w = o.wave(150);
    let wf = gmfit::analyze::analyze(150, &w, &m);
    let s = gmfit::evaluate(&w, 150, &wf.params).0;
    assert!(s.score >= 0.85, "{:?}", s);
}

/// Spec "Seeded reproducibility".
#[test]
fn fit_is_reproducible() {
    let Some(o) = orig() else { return };
    let w = o.wave(150);
    let m = gmgen::bank::meta(150).unwrap();
    let start = gmfit::analyze::analyze(150, &w, &m).params;
    let a = gmfit::fit::fit(&w, 150, &start, 5, 7, None);
    let b = gmfit::fit::fit(&w, 150, &start, 5, 7, None);
    assert_eq!(a.0, b.0);
    assert_eq!(a.1, b.1);
}

/// Spec "No regression written": a lower score leaves the committed file alone.
#[test]
fn worse_result_is_not_written() {
    let Some(o) = orig() else { return };
    let before = std::fs::read_to_string(gmfit::wave_path(150)).unwrap();
    let mut wf = gmfit::load_wave(150).unwrap();
    wf.params.layers.clear();
    assert!(!gmfit::keep_if_better(&o, 150, wf, 0.0).unwrap());
    assert_eq!(std::fs::read_to_string(gmfit::wave_path(150)).unwrap(), before);
}
