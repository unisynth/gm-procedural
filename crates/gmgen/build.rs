//! Packs `assets/gm/` (and `assets/gm-plus/` with feature `plus`; structure + per-wave parameters) into `OUT_DIR` blobs embedded by `src/bank.rs`.
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

fn pack_set(dir: &str, prefix: &str) {
    let path = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..").join(dir);
    println!("cargo:rerun-if-changed={}", path.display());
    let p = pack::pack(&path).unwrap_or_else(|e| panic!("{dir}: {e}"));
    if !p.missing.is_empty() {
        println!("cargo:warning=gmgen: {dir}: {} waves have no parameter file (rendered silent)", p.missing.len());
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out.join(format!("{prefix}skeleton.dls")), &p.skeleton).unwrap();
    std::fs::write(out.join(format!("{prefix}lens.bin")), &p.lens).unwrap();
    std::fs::write(out.join(format!("{prefix}params.bin")), &p.params).unwrap();
}

fn main() {
    for f in ["params.rs", "params_json.rs", "schema.rs", "structure.rs", "pack.rs"] {
        println!("cargo:rerun-if-changed=src/{f}");
    }
    pack_set("assets/gm", "");
    // Feature `plus` (change add-gmplus-bank): the gmplus set as well.
    if std::env::var_os("CARGO_FEATURE_PLUS").is_some() {
        pack_set("assets/gm-plus", "plus_");
    }
}
