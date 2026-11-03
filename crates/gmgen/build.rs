//! Packs `assets/gm/` (structure + per-wave parameters) into `OUT_DIR` blobs embedded by `src/bank.rs`.
//! Invalid parameter files fail the build with the file and wave named.
#![allow(dead_code)]
extern crate alloc;

#[path = "src/params.rs"]
mod params;
#[path = "src/params_json.rs"]
mod params_json;
#[path = "src/schema.rs"]
mod schema;
#[path = "src/structure.rs"]
mod structure;
#[path = "src/pack.rs"]
mod pack;

fn main() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/gm");
    println!("cargo:rerun-if-changed={}", dir.display());
    for f in ["params.rs", "params_json.rs", "schema.rs", "structure.rs", "pack.rs"] {
        println!("cargo:rerun-if-changed=src/{f}");
    }
    let p = pack::pack(&dir).unwrap_or_else(|e| panic!("assets/gm: {e}"));
    if !p.missing.is_empty() {
        println!("cargo:warning=gmgen: {} waves have no parameter file (rendered silent)", p.missing.len());
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join("skeleton.dls"), &p.skeleton).unwrap();
    std::fs::write(out.join("lens.bin"), &p.lens).unwrap();
    std::fs::write(out.join("params.bin"), &p.params).unwrap();
}
