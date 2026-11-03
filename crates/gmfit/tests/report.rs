//! `report --check` regression rule.
use serde_json::{json, Map, Value};

fn m(v: &[(&str, f64)]) -> Map<String, Value> {
    v.iter().map(|(k, s)| (k.to_string(), json!({"score": s}))).collect()
}

#[test]
fn lowered_wave_is_named() {
    let committed = m(&[("36", 0.9), ("37", 0.8)]);
    let now = m(&[("36", 0.88), ("37", 0.797)]);
    let r = gmfit::regressions(&committed, &now);
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(r[0].starts_with("wave 36:"), "{r:?}");
}

#[test]
fn new_waves_and_improvements_pass() {
    assert!(gmfit::regressions(&m(&[("1", 0.5)]), &m(&[("1", 0.6), ("2", 0.1)])).is_empty());
}
