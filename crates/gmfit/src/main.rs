//! `gmfit <command>`: fit the procedural GM bank against the original `gm.dls` (`GM_DLS`).
//!
//! gmfit extract [-o assets/gm/structure.json]   write the bank structure (no audio)
//! gmfit placeholder                             write a placeholder parameter file for every wave without one
//! gmfit render <wave> [-p file.json]            A/B wav + sheet.png + report.json in build/gm/NNN, JSON on stdout
//! gmfit sweep <wave> <param> <v1,v2,..>         one scored, rendered variant per value (files unchanged)
//! gmfit report [--check] [--baseline]           score every wave → build/gm/report.md; --check vs scores.json
//!
//! Exit: 0 ok, 2 flags raised (render) / regression (report --check), 1 error.
use gmfit::{score, sheet};
use serde_json::{json, Value};
use std::process::exit;

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("gmfit: {msg}");
    exit(1)
}

fn wave_arg(args: &[String]) -> usize {
    let i: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or_else(|| die("missing wave index"));
    if i >= gmgen::wave_count() {
        die(format!("wave {i} out of range (0..{})", gmgen::wave_count()));
    }
    i
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let flag = |name: &str| args.iter().any(|a| a == name);
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
                std::fs::write(&path, gmgen::params_json::to_json(&gmfit::placeholder(i as u32, &m))).unwrap_or_else(|e| die(e));
                made += 1;
            }
            println!("{made} placeholder files written to {}", dir.display());
        }
        Some("render") => {
            let i = wave_arg(&args);
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let wf = match opt("-p") {
                Some(f) => std::fs::read_to_string(&f)
                    .map_err(|e| e.to_string())
                    .and_then(|t| gmgen::params_json::from_json(&t))
                    .unwrap_or_else(|e| die(format!("{f}: {e}"))),
                None => gmfit::load_wave(i).unwrap_or_else(|e| die(e)),
            };
            let m = gmgen::bank::meta(i).unwrap();
            let o = orig.wave(i);
            let (s, plain, synth) = gmfit::evaluate(&o, i, &wf.params);
            let dir = gmfit::out_dir(i);
            let (pa, pb) = (gmfit::audio::played(&o, &m, 1.0), gmfit::audio::played(&synth, &m, 1.0));
            gmfit::audio::write_wav(&dir.join("orig.wav"), &pa, m.rate).unwrap_or_else(|e| die(e));
            gmfit::audio::write_wav(&dir.join("gen.wav"), &pb, m.rate).unwrap_or_else(|e| die(e));
            let (sa, sb) = (gmfit::audio::played(&o, &m, score::PLAYED_SECS), gmfit::audio::played(&synth, &m, score::PLAYED_SECS));
            sheet::pair(&dir.join("sheet.png"), &sa, &sb, m.rate as f32).unwrap_or_else(|e| die(e));
            let (names, cat) = gmfit::users(i);
            let layers: Vec<Value> = wf
                .params
                .layers
                .iter()
                .map(|l| json!({"family": gmgen::schema::FAMILIES[l.family].name, "region": l.region.name()}))
                .collect();
            let mut rep = json!({
                "wave": i, "category": cat, "instruments": names, "rate": m.rate, "len": m.len,
                "loop": m.looped, "unity": m.unity, "placeholder": wf.placeholder, "numbers": wf.params.numbers(),
                "layers": layers, "tier": gmfit::tier(s.score), "dir": dir.display().to_string(),
            });
            rep.as_object_mut().unwrap().extend(s.json().as_object().unwrap().clone());
            if let Some(p) = plain {
                rep["score_without_correction"] = json!(score::r4(p.score));
            }
            let text = serde_json::to_string_pretty(&rep).unwrap();
            std::fs::write(dir.join("report.json"), &text).unwrap_or_else(|e| die(e));
            println!("{text}");
            exit(if s.flags.is_empty() { 0 } else { 2 });
        }
        Some("sweep") => {
            let i = wave_arg(&args);
            let (path, values) = (args.get(2).unwrap_or_else(|| die("missing parameter")), args.get(3).unwrap_or_else(|| die("missing values")));
            let values: Vec<f32> = values.split(',').map(|v| v.parse().unwrap_or_else(|_| die(format!("bad value {v}")))).collect();
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let wf = gmfit::load_wave(i).unwrap_or_else(|e| die(e));
            let m = gmgen::bank::meta(i).unwrap();
            let o = orig.wave(i);
            let mut rows = Vec::new();
            let mut sigs = Vec::new();
            for v in &values {
                let mut p = wf.params.clone();
                gmfit::set_param(&mut p, path, *v).unwrap_or_else(|e| die(format!("{path} = {v}: {e}")));
                let (s, _, synth) = gmfit::evaluate(&o, i, &p);
                let mut row = json!({"value": v});
                row.as_object_mut().unwrap().extend(s.json().as_object().unwrap().clone());
                rows.push(row);
                sigs.push(gmfit::audio::played(&synth, &m, score::PLAYED_SECS));
            }
            let dir = gmfit::out_dir(i);
            sheet::rows(&dir.join("sweep.png"), &gmfit::audio::played(&o, &m, score::PLAYED_SECS), &sigs, m.rate as f32)
                .unwrap_or_else(|e| die(e));
            println!("{}", serde_json::to_string_pretty(&json!({"wave": i, "param": path, "rows": rows, "sheet": dir.join("sweep.png").display().to_string()})).unwrap());
        }
        Some("report") => report(flag("--check"), flag("--baseline")),
        _ => die("usage: gmfit extract | placeholder | render <wave> [-p f] | sweep <wave> <param> <values> | report [--check] [--baseline]"),
    }
}

fn report(check: bool, baseline: bool) {
    let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
    let n = gmgen::wave_count();
    let waves: Vec<usize> = (0..n).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    type Row = (score::Score, bool, usize, Vec<f32>, Vec<f32>);
    let results: Vec<(usize, Result<Row, String>)> = std::thread::scope(|sc| {
        let handles: Vec<_> = waves
            .chunks(n.div_ceil(threads))
            .map(|chunk| {
                let orig = &orig;
                sc.spawn(move || {
                    chunk
                        .iter()
                        .map(|&i| {
                            let r = gmfit::load_wave(i).map(|wf| {
                                let o = orig.wave(i);
                                let (s, _, synth) = gmfit::evaluate(&o, i, &wf.params);
                                let m = gmgen::bank::meta(i).unwrap();
                                let (pa, pb) = (gmfit::audio::played(&o, &m, 0.25), gmfit::audio::played(&synth, &m, 0.25));
                                (s, wf.placeholder, wf.params.numbers(), pa, pb)
                            });
                            (i, r)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let committed = gmfit::load_scores();
    let mut rows: Vec<(String, f32, String)> = Vec::new();
    let mut new_scores = serde_json::Map::new();
    let mut tiers = std::collections::BTreeMap::<&str, usize>::new();
    let mut placeholders = 0;
    for (i, r) in &results {
        let (cat_names, cat) = gmfit::users(*i);
        match r {
            Ok((s, ph, nums, _, _)) => {
                *tiers.entry(gmfit::tier(s.score)).or_default() += 1;
                placeholders += *ph as usize;
                new_scores.insert(i.to_string(), s.json());
                let first = cat_names.first().cloned().unwrap_or_default();
                rows.push((
                    cat.to_string(),
                    s.score,
                    format!(
                        "| {i} | {} | {:.4} | {} | {:.2} | {:.2} | {:.2} | {} | {nums} | {}{} |",
                        first.replace('|', "/"),
                        s.score,
                        gmfit::tier(s.score),
                        s.d_spec,
                        s.d_env,
                        s.d_loop,
                        s.flags.join(" "),
                        if *ph { "placeholder " } else { "" },
                        if cat_names.len() > 1 { format!("+{} more", cat_names.len() - 1) } else { String::new() }
                    ),
                ));
            }
            Err(e) => rows.push((cat.to_string(), -1.0, format!("| {i} | | error | | | | | {e} | | |"))),
        }
    }
    let regressions = gmfit::regressions(&committed, &new_scores);
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut md = format!(
        "# GM bank fit report\n\nTiers: {tiers:?}; placeholders: {placeholders}; regressions vs scores.json: {}.\n\n",
        regressions.len()
    );
    let mut last = String::new();
    for (cat, _, line) in &rows {
        if *cat != last {
            md.push_str(&format!("\n## {cat}\n\n| wave | first user | score | tier | d_spec | d_env | d_loop | flags | numbers | note |\n|---|---|---|---|---|---|---|---|---|---|\n"));
            last = cat.clone();
        }
        md.push_str(line);
        md.push('\n');
    }
    let path = gmfit::repo_root().join("build/gm/report.md");
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    std::fs::write(&path, md).unwrap_or_else(|e| die(e));
    let mut cells: Vec<(usize, f32, Vec<f32>, Vec<f32>)> =
        results.iter().filter_map(|(i, r)| r.as_ref().ok().map(|x| (*i, x.0.score, x.3.clone(), x.4.clone()))).collect();
    cells.sort_by(|a, b| a.1.total_cmp(&b.1));
    sheet::bank(&gmfit::repo_root().join("build/gm/bank.png"), &cells, 22050.0).unwrap_or_else(|e| die(e));
    let mean = results.iter().filter_map(|(_, r)| r.as_ref().ok().map(|x| x.0.score)).sum::<f32>() / n as f32;
    println!("{}", json!({"report": path.display().to_string(), "tiers": tiers, "placeholders": placeholders, "mean": score::r4(mean), "regressions": regressions}));
    if baseline {
        gmfit::save_scores(&new_scores).unwrap_or_else(|e| die(e));
    }
    if check && !regressions.is_empty() {
        eprintln!("gmfit: {} waves regressed:\n{}", regressions.len(), regressions.join("\n"));
        exit(2);
    }
}
