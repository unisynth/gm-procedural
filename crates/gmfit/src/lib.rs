//! `gmfit`: fits the procedural GM bank (`crates/gmgen`) against the original Windows `gm.dls` (from `GM_DLS`, never
//! committed). Subcommands live in `main.rs`; shared helpers here.
use std::path::PathBuf;

/// Repository root (two levels above this crate).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `assets/gm/` in the repository.
pub fn assets_dir() -> PathBuf {
    repo_root().join("assets/gm")
}

/// The original bank bytes from `GM_DLS` (set and non-empty), or a message saying it is required.
pub fn original_bank() -> Result<Vec<u8>, String> {
    let p = std::env::var("GM_DLS").ok().filter(|p| !p.is_empty()).ok_or(
        "the original bank is required: set GM_DLS to a local copy of Windows gm.dls (never commit it)",
    )?;
    std::fs::read(&p).map_err(|e| format!("GM_DLS={p}: {e}"))
}

/// Placeholder parameters (task 3.5): one `additive` layer at the unity-note pitch with a gently falling
/// 8-harmonic table — `sustain` for looped waves, a decaying `whole` layer otherwise. Flagged `placeholder`.
pub fn placeholder(wave: u32, m: &gmgen::render::WaveMeta) -> gmgen::params_json::WaveFile {
    use gmgen::params::{Layer, Params};
    use gmgen::schema::{family, Region};
    let looped = m.looped.is_some_and(|(s, l)| l > 0 && (s + l) as usize <= m.len);
    let mut l = Layer::new(family("additive").expect("additive family"), if looped { Region::Sustain } else { Region::Whole });
    l.set("level_db", -12.0);
    // 5 ms fade-in, shortened when the loop starts earlier (the envelope freezes at the loop start).
    let start = m.looped.map_or(m.len as u32, |(s, _)| s) as f32 / m.rate as f32;
    l.set("a", (0.005f32).min(start * 0.5));
    if !looped {
        l.set("d", 0.25);
        l.set("s", 0.0);
    }
    l.tables[0] = vec![0.0, -6.0, -10.0, -13.0, -16.0, -18.0, -20.0, -22.0];
    let mut p = Params::default();
    p.layers.push(l);
    gmgen::params_json::WaveFile { wave, params: p, placeholder: true, note: String::new() }
}
