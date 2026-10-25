//! `cargo run -p dls --example dump -- <bank.dls>`: TSV listing of instruments and waves.
//! Structural fingerprint for diffing banks (e.g. macOS gs_instruments.dls vs Windows gm.dls):
//! wave lines carry rate/bits/length/unity/loop, which survive 8-bit vs 16-bit requantization.
fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <bank.dls>");
    let file = std::fs::read(&path).expect("read bank");
    let d = dls::Dls::parse(&file).expect("parse DLS");
    println!("# {path}: {} instruments, {} waves", d.instrument_count(), d.wave_count());
    for i in d.instruments() {
        let name = i.name.map(String::from_utf8_lossy).unwrap_or_default();
        let waves: Vec<String> = i.regions().map(|r| format!("{}-{}:{}", r.key_lo, r.key_hi, r.wave)).collect();
        println!(
            "ins\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            i.drums as u8,
            i.bank_msb,
            i.program,
            name.trim(),
            i.regions().count(),
            i.articulation().count(),
            waves.join(" ")
        );
    }
    for w in 0..d.wave_count() {
        match d.wave(w) {
            Some(v) => println!(
                "wave\t{w}\t{}\t{}\t{}\t{}\t{}\t{:?}",
                v.rate,
                v.bits,
                v.len(),
                v.wsmp.unity_note,
                v.wsmp.fine_tune,
                v.wsmp.looped
            ),
            None => println!("wave\t{w}\tUNREADABLE"),
        }
    }
}
