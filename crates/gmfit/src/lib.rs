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
