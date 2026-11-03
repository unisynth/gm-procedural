//! `assets/gm/structure.json`: committed structure only, never audio; lossless against the real bank.
use gmgen::structure;

fn committed() -> String {
    std::fs::read_to_string(gmfit::assets_dir().join("structure.json")).expect("assets/gm/structure.json committed")
}

/// Spec "No PCM committed": no DLS magic, no sample arrays, small.
#[test]
fn no_pcm_committed() {
    let text = committed();
    assert!(text.len() < 512 * 1024, "structure.json is {} bytes", text.len());
    assert!(!text.contains("RIFF") && !text.contains("DLS "));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    fn longest(v: &serde_json::Value) -> usize {
        match v {
            serde_json::Value::Array(a) => {
                let numeric = a.iter().filter(|x| x.is_number()).count();
                a.iter().map(longest).max().unwrap_or(0).max(numeric)
            }
            serde_json::Value::Object(o) => o.values().map(longest).max().unwrap_or(0),
            _ => 0,
        }
    }
    assert!(longest(&v) <= 5, "a numeric array longer than 5 entries looks like sample data");
    for w in v["waves"].as_array().unwrap() {
        assert!(w.get("samples").is_none() && w.get("data").is_none());
    }
}

#[test]
fn committed_parses() {
    let b = structure::from_json(&committed()).unwrap();
    assert_eq!((b.instruments.len(), b.waves.len()), (235, 495));
}

/// With `GM_DLS`: JSON round trip equals the real bank apart from the samples.
#[test]
fn round_trip_against_gm_dls() {
    let Ok(bytes) = gmfit::original_bank() else {
        eprintln!("skip: GM_DLS not set");
        return;
    };
    let mut orig = dls::write::Bank::read(&dls::Dls::parse(&bytes).unwrap());
    for w in &mut orig.waves {
        w.samples.iter_mut().for_each(|s| *s = 0);
    }
    let text = structure::to_json(&orig);
    assert_eq!(structure::from_json(&text).unwrap(), orig);
    assert_eq!(text, committed(), "assets/gm/structure.json is stale: rerun gmfit extract");
}
