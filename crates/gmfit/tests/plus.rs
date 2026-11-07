//! gmplus targets against gm.dls (needs `GM_DLS` and `GMPLUS_SF2`; skipped otherwise). Change add-gmplus-bank 1.4.

/// Fundamental from rising zero crossings of the h1 band (narrow band-pass around the estimate) over a long tiled loop.
fn f0(x: &[f32], m: &gmgen::render::WaveMeta, guess: f32) -> f32 {
    let p = gmfit::audio::played(x, m, 4.0);
    let rate = m.rate as f64;
    let bp = |y: &[f64]| {
        let w = 2.0 * std::f64::consts::PI * guess as f64 / rate;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * 8.0);
        let a0 = 1.0 + al;
        let (b0, b2, a1, a2) = (al / a0, -al / a0, -2.0 * c / a0, (1.0 - al) / a0);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        y.iter().map(|&v| { let o = b0 * v + b2 * x2 - a1 * y1 - a2 * y2; x2 = x1; x1 = v; y2 = y1; y1 = o; o }).collect::<Vec<f64>>()
    };
    let p: Vec<f64> = p.iter().map(|&v| v as f64).collect();
    let y = bp(&bp(&p));
    let from = m.looped.map_or(0, |(s, _)| s as usize).max(y.len() / 3);
    let mut cross = Vec::new();
    for k in from..y.len() - 1 {
        if y[k] < 0.0 && y[k + 1] >= 0.0 {
            cross.push(k as f64 + y[k] / (y[k] - y[k + 1]));
        }
    }
    ((cross.len() - 1) as f64 / (cross[cross.len() - 1] - cross[0]) * rate) as f32
}

/// gmplus at the wave's key, rendered naively (linear interpolation, loop unrolled, 32 kHz): the note the target
/// must reproduce. Pitch only (no filter, no envelope).
fn gmplus_note(sf: &gmfit::sf2::Sf2, i: usize) -> Option<(Vec<f32>, f32)> {
    let gm_meta = gmgen::bank::meta(i)?;
    let bank = gmgen::bank::skeleton();
    let u = gmfit::main_user(&bank, Some((gm_meta.unity, gm_meta.fine)), i)?;
    let ins = &bank.instruments[u.ins];
    let (pb, pp) = if ins.drums { (128, ins.program as u16) } else { (ins.bank_msb as u16, ins.program as u16) };
    let z = sf.zones(pb, pp, u.key, 100).into_iter().next()?;
    let smp = &sf.samples[z.sample];
    let (a, e, ls, le) = z.bounds(smp);
    let x = sf.slice(a as usize, e as usize);
    let (s0, p) = ((ls - a) as f64, (le - ls) as f64);
    let step = (z.cents(smp, u.key) as f64 / 1200.0).exp2() * smp.rate as f64 / 32000.0;
    let y = (0..32000 * 4)
        .map(|n| {
            let mut q = n as f64 * step;
            if q >= s0 + p {
                q = s0 + (q - s0) % p;
            }
            let k = q as usize;
            let f = (q - k as f64) as f32;
            let nx = if (k + 1) as f64 >= s0 + p { x[s0 as usize] } else { x[k + 1] };
            x[k] * (1.0 - f) + nx * f
        })
        .collect();
    Some((y, 440.0 * ((u.key as f32 - 69.0) / 12.0).exp2()))
}

#[test]
fn piano_targets_keep_the_gmplus_pitch() {
    if std::env::var("GM_DLS").is_err() || std::env::var("GMPLUS_SF2").is_err() {
        return;
    }
    let gm = std::fs::read(std::env::var("GM_DLS").unwrap()).unwrap();
    let d = dls::Dls::parse(&gm).unwrap();
    let t = gmfit::plus::Targets::load().unwrap();
    let sf = gmfit::sf2::Sf2::open(std::path::Path::new(&std::env::var("GMPLUS_SF2").unwrap())).unwrap();
    let bank = gmgen::bank::skeleton();
    let mut worst = 0.0f32;
    for i in 289..=298 {
        let gm_meta = gmgen::bank::meta(i).unwrap();
        let u = gmfit::main_user(&bank, Some((gm_meta.unity, gm_meta.fine)), i).unwrap();
        let tg = t.wave(&d, i).unwrap();
        let pm = tg.meta(&gm_meta);
        let (note, guess) = gmplus_note(&sf, i).unwrap();
        let flat = gmgen::render::WaveMeta { rate: 32000, len: note.len(), unity: 60, fine: 0, looped: None };
        let want = f0(&note, &flat, guess);
        // The target as the bank plays it at that key: speed from the gm.dls tuning, plus the fine-tune delta.
        let got = f0(&tg.samples, &pm, guess / u.speed) * u.speed * (tg.fine_delta as f32 / 1200.0).exp2();
        let cents = 1200.0 * (got / want).log2();
        println!("wave {i} key {}: gmplus {want:.3} Hz, target {got:.3} Hz → {cents:+.3} cents", u.key);
        worst = worst.max(cents.abs());
    }
    assert!(worst < 1.0, "worst {worst} cents");
}

#[test]
fn map_holds_no_audio() {
    let p = gmfit::repo_root().join("assets/gm-plus/map.json");
    let Ok(text) = std::fs::read_to_string(&p) else { return };
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let waves = v["waves"].as_array().unwrap();
    // 495 skeleton waves + stereo partners (rows with "left").
    let partners = waves.iter().filter(|w| w.get("left").is_some()).count();
    assert_eq!(waves.len(), 495 + partners);
    assert_eq!(partners, waves.iter().filter(|w| w.get("stereo").is_some()).count());
    for w in waves {
        for (k, x) in w.as_object().unwrap() {
            assert!(x.as_array().is_none_or(|a| a.len() <= 3), "wave {}: {k} looks like sample data", w["wave"]);
        }
    }
    assert!(text.len() < 400_000, "map is {} bytes", text.len());
}

/// `assets/gm-plus/structure.json` against the gm set and the map (task 2.1): same layout, 32 kHz, target loops,
/// fine tune and gain shifted by the map's deltas (wave and region `wsmp` alike).
#[test]
fn plus_structure_follows_the_map() {
    let root = gmfit::repo_root();
    let read = |p: &str| std::fs::read_to_string(root.join(p));
    let (Ok(plus), Ok(gm), Ok(map)) = (read("assets/gm-plus/structure.json"), read("assets/gm/structure.json"), read("assets/gm-plus/map.json")) else {
        return;
    };
    let (p, g) = (gmgen::structure::from_json(&plus).unwrap(), gmgen::structure::from_json(&gm).unwrap());
    let rows: serde_json::Value = serde_json::from_str(&map).unwrap();
    let rows = rows["waves"].as_array().unwrap();
    // Skeleton waves, then one partner per pair (change add-gmplus-bank D4): same wave as its left, never in a region.
    assert_eq!((p.instruments.len(), p.waves.len()), (g.instruments.len(), g.waves.len() + p.pairs.len()));
    for (k, &(l, r)) in p.pairs.iter().enumerate() {
        assert_eq!(r as usize, g.waves.len() + k);
        assert_eq!(p.waves[r as usize], p.waves[l as usize], "partner {r} of {l}");
        assert_eq!(rows[r as usize]["left"].as_u64(), Some(l as u64));
        assert!(p.instruments.iter().all(|i| i.regions.iter().all(|x| x.wave != r)), "partner {r} in a region");
    }
    let delta = |i: usize| (rows[i]["fine_delta"].as_i64().unwrap() as i16, rows[i]["wsmp_gain"].as_i64().unwrap() as i32);
    for (i, (pw, gw)) in p.waves.iter().zip(&g.waves).enumerate() {
        let (df, dg) = delta(i);
        assert_eq!(pw.rate, 32000, "wave {i}");
        assert_eq!(pw.samples.len(), rows[i]["len"].as_u64().unwrap() as usize, "wave {i} length");
        assert_eq!(pw.wsmp.looped, rows[i]["loop"].as_array().map(|a| (a[0].as_u64().unwrap() as u32, a[1].as_u64().unwrap() as u32)), "wave {i} loop");
        if let Some((s, l)) = pw.wsmp.looped {
            assert!(l > 0 && (s + l) as usize == pw.samples.len(), "wave {i}: attack + one loop");
        }
        assert_eq!((pw.wsmp.unity_note, pw.wsmp.fine_tune, pw.wsmp.gain), (gw.wsmp.unity_note, gw.wsmp.fine_tune + df, gw.wsmp.gain + dg), "wave {i} wsmp");
    }
    for (pi, gi) in p.instruments.iter().zip(&g.instruments) {
        assert_eq!((pi.drums, pi.bank_msb, pi.program, &pi.name, &pi.articulation), (gi.drums, gi.bank_msb, gi.program, &gi.name, &gi.articulation));
        assert_eq!(pi.regions.len(), gi.regions.len());
        for (pr, gr) in pi.regions.iter().zip(&gi.regions) {
            assert_eq!((pr.key_lo, pr.key_hi, pr.vel_lo, pr.vel_hi, pr.key_group, pr.wave, &pr.articulation), (gr.key_lo, gr.key_hi, gr.vel_lo, gr.vel_hi, gr.key_group, gr.wave, &gr.articulation));
            match (&pr.wsmp, &gr.wsmp) {
                (Some(a), Some(b)) => {
                    let (df, dg) = delta(pr.wave as usize);
                    assert_eq!((a.unity_note, a.fine_tune, a.gain), (b.unity_note, b.fine_tune + df, b.gain + dg));
                    assert_eq!(a.looped, p.waves[pr.wave as usize].wsmp.looped);
                }
                (None, None) => {}
                _ => panic!("region wsmp presence differs"),
            }
        }
    }
}
