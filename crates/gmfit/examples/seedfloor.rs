//! Hearing-model floor of event textures (design D4p): committed params rendered with other seeds must hear as
//! the same sound (added + missing ≤ 0.05) — `cargo run --release -p gmfit --example seedfloor`.
fn main() {
    for i in [62usize, 404, 476, 230, 26, 435, 106, 272, 58, 50, 226, 405, 376] {
        let wf = gmfit::load_wave(i).unwrap();
        let m = gmgen::bank::meta(i).unwrap();
        let sp = gmfit::play_speed(i).1;
        let a = gmgen::render::render(&m, &wf.params, i as u32);
        let mut out = String::new();
        for s in [1000u32, 2000, 3000] {
            let b = gmgen::render::render(&m, &wf.params, s + i as u32);
            let (pa, pb) = (gmfit::audio::played(&a, &m, 1.0), gmfit::audio::played(&b, &m, 1.0));
            let h = gmfit::hear::hear(&gmfit::hear::at_speed(&pa, sp), &gmfit::hear::at_speed(&pb, sp), m.rate as f32);
            out += &format!(" add {:.2} miss {:.2} mod {:.3} |", h.added, h.missing, h.moddiff);
        }
        let fams: Vec<&str> = wf.params.layers.iter().map(|l| gmgen::schema::FAMILIES[l.family].name).collect();
        println!("{i:3} {fams:?}:{out}");
    }
}
