//! `gmfit <command>`: fit the procedural GM bank against the original `gm.dls` (`GM_DLS`).
//!
//! gmfit extract [-o assets/gm/structure.json]   write the bank structure (no audio)
//! gmfit placeholder                             write a placeholder parameter file for every wave without one
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
        Some("placeholder") => {
            let dir = gmfit::assets_dir().join("waves");
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| die(e));
            let mut made = 0;
            for i in 0..gmgen::wave_count() {
                let path = dir.join(format!("{i:03}.json"));
                if path.exists() {
                    continue;
                }
                let m = gmgen::bank::meta(i).unwrap_or_else(|| die(format!("wave {i}: no meta")));
                let wf = gmfit::placeholder(i as u32, &m);
                std::fs::write(&path, gmgen::params_json::to_json(&wf)).unwrap_or_else(|e| die(e));
                made += 1;
            }
            println!("{made} placeholder files written to {}", dir.display());
        }
        _ => die("usage: gmfit extract [-o <structure.json>] | placeholder"),
    }
}
