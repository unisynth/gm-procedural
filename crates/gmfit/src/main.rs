//! `gmfit <command>`: fit the procedural GM bank against the original `gm.dls` (`GM_DLS`).
//!
//! gmfit extract [-o assets/gm/structure.json]   write the bank structure (no audio)
use std::process::exit;

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("gmfit: {msg}");
    exit(1)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(String::as_str) {
        Some("extract") => {
            let bytes = gmfit::original_bank().unwrap_or_else(|e| die(e));
            let d = dls::Dls::parse(&bytes).unwrap_or_else(|e| die(format!("not a DLS bank: {e:?}")));
            let bank = dls::write::Bank::read(&d);
            let out = opt("-o").map(Into::into).unwrap_or_else(|| gmfit::assets_dir().join("structure.json"));
            std::fs::write(&out, gmgen::structure::to_json(&bank)).unwrap_or_else(|e| die(e));
            println!("{}: {} instruments, {} waves", out.display(), bank.instruments.len(), bank.waves.len());
        }
        _ => die("usage: gmfit extract [-o <structure.json>]"),
    }
}
