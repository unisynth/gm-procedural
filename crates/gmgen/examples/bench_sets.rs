//! Generation time of the full bank (change add-gmplus-bank 2.3): the gm.dls set as committed, and the plus
//! skeleton (32 kHz, plus lengths) rendered with the gm set's parameters as a stand-in until the plus fit exists.
//! `cargo run --release -p gmgen --features plus --example bench_sets`
fn main() {
    let t = std::time::Instant::now();
    let gm = gmgen::bank::build(None);
    let gm_ms = t.elapsed().as_secs_f64() * 1e3;
    let params = gmgen::bank::committed();
    let (mut gm_n, mut plus_n) = (0usize, 0usize);
    let t = std::time::Instant::now();
    for (i, p) in params.iter().enumerate() {
        let (Some(p), Some(m)) = (p, gmgen::plus::meta(i)) else { continue };
        plus_n += gmgen::render::render(&m, p, i as u32).len();
        gm_n += gmgen::bank::meta(i).unwrap().len;
    }
    let plus_ms = t.elapsed().as_secs_f64() * 1e3;
    println!("gm set: {gm_ms:.0} ms, {} bytes, {gm_n} samples", gm.len());
    println!("plus skeleton with gm parameters: {plus_ms:.0} ms, {plus_n} samples ({:.1}× the samples)", plus_n as f64 / gm_n as f64);
}
