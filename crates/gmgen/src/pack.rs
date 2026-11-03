//! `assets/gm/` → the three blobs the generator embeds (used by `build.rs` through `#[path]`, and by tests):
//! - `skeleton`: the bank structure as a DLS buffer whose waves have no samples (parsed at runtime by `dls`),
//! - `lens`: each wave's sample count (u32 LE),
//! - `params`: per wave a presence byte (0 = no parameter file) followed by the encoded [`Params`].
//!
//! Invalid parameter files fail with a message naming the file and wave. Missing files are recorded as absent;
//! the test suite (not the build) fails on them, so tooling can run while the bank is being filled.
use crate::params_json;
use crate::structure;
use std::path::Path;

pub struct Packed {
    pub skeleton: Vec<u8>,
    pub lens: Vec<u8>,
    pub params: Vec<u8>,
    /// Waves without a parameter file.
    pub missing: Vec<u32>,
}

pub fn pack(dir: &Path) -> Result<Packed, String> {
    let sp = dir.join("structure.json");
    let text = std::fs::read_to_string(&sp).map_err(|e| format!("{}: {e}", sp.display()))?;
    let mut bank = structure::from_json(&text)?;
    let mut lens = Vec::new();
    for w in &mut bank.waves {
        lens.extend_from_slice(&(w.samples.len() as u32).to_le_bytes());
        w.samples = Vec::new();
    }
    let count = bank.waves.len();
    let mut files: Vec<Option<params_json::WaveFile>> = vec![None; count];
    let wd = dir.join("waves");
    if let Ok(rd) = std::fs::read_dir(&wd) {
        let mut paths: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            let idx: u32 = name.trim_end_matches(".json").parse().map_err(|_| format!("waves/{name}: name must be NNN.json"))?;
            let text = std::fs::read_to_string(&p).map_err(|e| format!("waves/{name}: {e}"))?;
            let wf = params_json::from_json(&text).map_err(|e| format!("waves/{name} (wave {idx}): {e}"))?;
            if wf.wave != idx {
                return Err(format!("waves/{name}: \"wave\" is {} but the file name says {idx}", wf.wave));
            }
            let slot = files.get_mut(idx as usize).ok_or(format!("waves/{name}: wave {idx} is not in the bank ({count} waves)"))?;
            *slot = Some(wf);
        }
    }
    let mut params = Vec::new();
    let mut missing = Vec::new();
    for (i, f) in files.iter().enumerate() {
        match f {
            Some(wf) => {
                params.push(1);
                wf.params.encode(&mut params);
            }
            None => {
                params.push(0);
                missing.push(i as u32);
            }
        }
    }
    Ok(Packed { skeleton: dls::write::write(&bank), lens, params, missing })
}
