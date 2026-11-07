//! `gmfit <command>`: fit the procedural GM bank against the original `gm.dls` (`GM_DLS`).
//!
//! gmfit extract [-o assets/gm/structure.json]   write the bank structure (no audio)
//! gmfit bank [--orig] [-o build/gm/gen.dls]   write the generated bank (committed parameters) as a DLS file;
//!                                               --orig re-writes GM_DLS through the same writer
//! gmfit placeholder                             write a placeholder parameter file for every wave without one
//! gmfit --set plus partners                  stereo partners = left parameters + correction vs the right target
//! gmfit --set plus stereo                    stereo check of every pair (left vs mid score, loop correlation)
//! gmfit dump <dir> [--secs 2]                 every wave's orig/gen wav (played) + index.json, for the A/B pages
//! gmfit render <wave> [-p file.json]            A/B wav + sheet.png + report.json in build/gm/NNN, JSON on stdout
//! gmfit sweep <wave> <param> <v1,v2,..>         one scored, rendered variant per value (files unchanged)
//! gmfit analyze <wave> [--force] [-o f]         initial parameters from the original + priors (placeholders only,
//!                                               unless --force; -o writes elsewhere)
//! gmfit fit <wave> [--iters N] [--seed S] [--vars 0.d,gain_db]   CMA-ES + harmonic refinement, kept if better
//! gmfit correct <wave>                          analytic harmonic + time × band (tf) + fine envelope (env) correction, kept if better
//! gmfit refine <wave> [--rounds 3] [--iters 80] [--seed 1]   alternate fit ↔ correct, kept if better
//! gmfit report [--check] [--baseline]           score every wave → build/gm/report.md; --check vs scores.json
//!
//! gmfit plus-map                               gmplus target per wave → assets/gm-plus/map.json (needs GMPLUS_SF2)
//!
//! Every command takes `--set gm|plus` (default gm): `plus` reads/writes assets/gm-plus and build/gm-plus and fits
//! the gmplus soundfont targets (`GMPLUS_SF2`) instead of the gm.dls waves (change add-gmplus-bank).
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
    if i >= gmfit::wave_count() {
        die(format!("wave {i} out of range (0..{})", gmfit::wave_count()));
    }
    i
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(k) = args.iter().position(|a| a == "--set") {
        let name = args.get(k + 1).cloned().unwrap_or_else(|| die("--set: gm | plus"));
        args.drain(k..k + 2);
        gmfit::select_set(gmfit::Set::from_name(&name).unwrap_or_else(|e| die(e))).unwrap_or_else(|e| die(e));
    }
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let flag = |name: &str| args.iter().any(|a| a == name);
    match args.first().map(String::as_str) {
        Some("extract") => {
            let bytes = gmfit::original_bank().unwrap_or_else(|e| die(e));
            let d = dls::Dls::parse(&bytes).unwrap_or_else(|e| die(format!("not a DLS bank: {e:?}")));
            let bank = match gmfit::set() {
                // plus: lengths, loops, fine tune and gain from the gmplus targets (add-gmplus-bank D3)
                gmfit::Set::Plus => gmfit::plus::Targets::load().and_then(|t| t.structure(&d)).unwrap_or_else(|e| die(e)),
                gmfit::Set::Gm => dls::write::Bank::read(&d),
            };
            let out = opt("-o").map(Into::into).unwrap_or_else(|| gmfit::assets_dir().join("structure.json"));
            std::fs::write(&out, gmgen::structure::to_json(&bank)).unwrap_or_else(|e| die(e));
            println!("{}: {} instruments, {} waves", out.display(), bank.instruments.len(), bank.waves.len());
        }
        Some("bank") => {
            // --orig: the original bank re-written by the same DLS writer (same layout as the generated one)
            let out = opt("-o").unwrap_or_else(|| gmfit::build_dir().join("gen.dls").display().to_string());
            let bytes = if gmfit::set() == gmfit::Set::Plus {
                gmgen::plus::build(None)
            } else if flag("--orig") {
                let b = gmfit::original_bank().unwrap_or_else(|e| die(e));
                let d = dls::Dls::parse(&b).unwrap_or_else(|e| die(format!("not a DLS bank: {e:?}")));
                dls::write::write(&dls::write::Bank::read(&d))
            } else {
                gmgen::bank::build(None)
            };
            std::fs::write(&out, &bytes).unwrap_or_else(|e| die(e));
            println!("{out}: {} bytes", bytes.len());
        }
        Some("placeholder") => {
            let dir = gmfit::assets_dir().join("waves");
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| die(e));
            let mut made = 0;
            for i in 0..gmfit::wave_count() {
                let path = dir.join(format!("{i:03}.json"));
                if path.exists() {
                    continue;
                }
                let m = gmfit::meta(i).unwrap_or_else(|| die(format!("wave {i}: no meta")));
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
            let m = gmfit::meta(i).unwrap();
            let o = orig.wave(i);
            let (s, plain, synth) = gmfit::evaluate(&o, i, &wf.params);
            let dir = gmfit::out_dir(i);
            let (pa, pb) = (gmfit::audio::played(&o, &m, 1.0), gmfit::audio::played(&synth, &m, 1.0));
            gmfit::audio::write_wav(&dir.join("orig.wav"), &pa, m.rate).unwrap_or_else(|e| die(e));
            gmfit::audio::write_wav(&dir.join("gen.wav"), &pb, m.rate).unwrap_or_else(|e| die(e));
            let (sa, sb) = (gmfit::audio::played(&o, &m, score::PLAYED_SECS), gmfit::audio::played(&synth, &m, score::PLAYED_SECS));
            match orig.targets() {
                // plus: target / generated / the gm.dls wave (resampled to the plus rate) as a third row
                Some(_) => {
                    // a stereo partner shows its left wave's gm.dls wave
                    let v = gmfit::voice(i);
                    let gm_meta = gmgen::bank::meta(v).unwrap();
                    let g = gmfit::audio::played(&orig.gm_wave(v), &gm_meta, score::PLAYED_SECS);
                    let step = gm_meta.rate as f64 / m.rate as f64;
                    let g = gmfit::dsp::resample(&g, 0.0, step, ((g.len() as f64 - 1.0) / step) as usize);
                    sheet::rows(&dir.join("sheet.png"), &sa, &[sb.clone(), g], m.rate as f32).unwrap_or_else(|e| die(e));
                }
                None => sheet::pair(&dir.join("sheet.png"), &sa, &sb, m.rate as f32).unwrap_or_else(|e| die(e)),
            }
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
                "layers": layers, "tier": gmfit::tier(&s), "play": {"key": gmfit::play_speed(i).0, "speed": score::r4(gmfit::play_speed(i).1), "by": gmfit::play_speed(i).2}, "dir": dir.display().to_string(),
            });
            rep.as_object_mut().unwrap().extend(s.json().as_object().unwrap().clone());
            if let Some(p) = plain {
                rep["score_without_correction"] = json!(score::r4(p.score));
            }
            if let Some(t) = orig.targets() {
                rep["gmplus"] = t.wave(&orig.dls(), i).map(|t| t.row).unwrap_or_else(|e| json!({"error": e}));
            }
            let text = serde_json::to_string_pretty(&rep).unwrap();
            std::fs::write(dir.join("report.json"), &text).unwrap_or_else(|e| die(e));
            println!("{text}");
            exit(if s.flags.is_empty() { 0 } else { 2 });
        }
        Some("hear") => {
            // hear <w> [-p params.json]: hearing-model difference (design D4p) at bank playback speed, 1 s played.
            let i = wave_arg(&args);
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let wf = match opt("-p") {
                Some(f) => std::fs::read_to_string(&f)
                    .map_err(|e| e.to_string())
                    .and_then(|t| gmgen::params_json::from_json(&t))
                    .unwrap_or_else(|e| die(format!("{f}: {e}"))),
                None => gmfit::load_wave(i).unwrap_or_else(|e| die(e)),
            };
            let m = gmfit::meta(i).unwrap();
            let o = orig.wave(i);
            let synth = gmfit::render_wave(i, &wf.params);
            let (key, speed, by) = gmfit::play_speed(i);
            let pitched = score::score(&o, &synth, &m).f0_orig.is_some();
            let h = gmfit::hear::listen(&o, &synth, &m, speed, pitched);
            let mut rep = json!({"wave": i, "play": {"key": key, "speed": score::r4(speed), "by": by}});
            rep.as_object_mut().unwrap().extend(h.json().as_object().unwrap().clone());
            println!("{}", serde_json::to_string_pretty(&rep).unwrap());
        }
        Some("sweep") => {
            let i = wave_arg(&args);
            let (path, values) = (args.get(2).unwrap_or_else(|| die("missing parameter")), args.get(3).unwrap_or_else(|| die("missing values")));
            let values: Vec<f32> = values.split(',').map(|v| v.parse().unwrap_or_else(|_| die(format!("bad value {v}")))).collect();
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let wf = gmfit::load_wave(i).unwrap_or_else(|e| die(e));
            let m = gmfit::meta(i).unwrap();
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
        Some("analyze") => {
            let i = wave_arg(&args);
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let m = gmfit::meta(i).unwrap();
            let o = orig.wave(i);
            let wf = gmfit::analyze::analyze(i, &o, &m);
            let s = gmfit::evaluate(&o, i, &wf.params).0;
            let text = gmgen::params_json::to_json(&wf);
            let target = opt("-o").map(std::path::PathBuf::from);
            let current = gmfit::load_wave(i).ok();
            let write_to = match target {
                Some(t) => Some(t),
                None if flag("--force") || current.as_ref().is_none_or(|w| w.placeholder) => Some(gmfit::wave_path(i)),
                None => None,
            };
            if let Some(t) = &write_to {
                std::fs::write(t, &text).unwrap_or_else(|e| die(e));
                if *t == gmfit::wave_path(i) {
                    let mut sc = gmfit::load_scores();
                    sc.insert(i.to_string(), s.json());
                    gmfit::save_scores(&sc).unwrap_or_else(|e| die(e));
                }
            }
            println!("{}", json!({"wave": i, "written": write_to.map(|p| p.display().to_string()), "score": score::r4(s.score), "flags": s.flags}));
        }
        Some("fit") => {
            let i = wave_arg(&args);
            let iters: usize = opt("--iters").and_then(|v| v.parse().ok()).unwrap_or(40);
            let seed: u64 = opt("--seed").and_then(|v| v.parse().ok()).unwrap_or(1);
            let only: Option<Vec<String>> = opt("--vars").map(|v| v.split(',').map(String::from).collect());
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let wf = gmfit::load_wave(i).unwrap_or_else(|e| die(e));
            let o = orig.wave(i);
            let before = gmfit::fit::score_of(&o, i, &wf.params);
            let (p, after) = gmfit::fit::fit(&o, i, &wf.params, iters, seed, only.as_deref());
            let kept = gmfit::keep_if_better(&orig, i, gmgen::params_json::WaveFile { params: p, ..wf }, after).unwrap_or_else(|e| die(e));
            println!("{}", json!({"wave": i, "before": score::r4(before), "after": score::r4(after), "written": kept}));
        }
        Some("correct") => {
            let i = wave_arg(&args);
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let wf = gmfit::load_wave(i).unwrap_or_else(|e| die(e));
            let o = orig.wave(i);
            let before = gmfit::fit::score_of(&o, i, &wf.params);
            let (p, after) = gmfit::fit::correct_all(&o, i, &wf.params);
            let (n, rows, env) = (p.correction.len(), p.tf.len() / gmgen::tf::BANDS, p.env.len());
            let kept = gmfit::keep_if_better(&orig, i, gmgen::params_json::WaveFile { params: p, ..wf }, after).unwrap_or_else(|e| die(e));
            println!("{}", json!({"wave": i, "before": score::r4(before), "after": score::r4(after), "entries": n, "tf_rows": rows, "env_points": env, "written": kept}));
        }
        Some("refine") => {
            let i = wave_arg(&args);
            let rounds = opt("--rounds").map_or(3, |v| v.parse().unwrap_or_else(|_| die("--rounds: number")));
            let iters = opt("--iters").map_or(80, |v| v.parse().unwrap_or_else(|_| die("--iters: number")));
            let seed = opt("--seed").map_or(1, |v| v.parse().unwrap_or_else(|_| die("--seed: number")));
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let wf = gmfit::load_wave(i).unwrap_or_else(|e| die(e));
            let o = orig.wave(i);
            let before = gmfit::fit::score_of(&o, i, &wf.params);
            let (p, after) = gmfit::fit::refine(&o, i, &wf.params, rounds, iters, seed);
            let kept = gmfit::keep_if_better(&orig, i, gmgen::params_json::WaveFile { params: p, ..wf }, after).unwrap_or_else(|e| die(e));
            println!("{}", json!({"wave": i, "before": score::r4(before), "after": score::r4(after), "written": kept}));
        }
        Some("bulk") => {
            // bulk (--cat <category> | <w>...) [--iters N] [--rounds N] [--jobs N]: full pipeline per wave from the
            // original, waves in parallel; each result is written only if it beats the committed score.
            let iters = opt("--iters").map_or(160, |v| v.parse().unwrap_or_else(|_| die("--iters: number")));
            let rounds = opt("--rounds").map_or(2, |v| v.parse().unwrap_or_else(|_| die("--rounds: number")));
            let jobs = opt("--jobs").map_or(4, |v| v.parse().unwrap_or_else(|_| die("--jobs: number")));
            let waves: Vec<usize> = match opt("--cat") {
                Some(c) => (0..gmfit::wave_count()).filter(|&i| gmfit::users(i).1 == c).collect(),
                None => args[1..]
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| !args[*k].starts_with("--"))
                    .filter_map(|(_, a)| a.parse().ok())
                    .collect(),
            };
            // stereo partners follow their left wave (`partners`) unless `--partners` (different samples per side)
            let partners = flag("--partners");
            let waves: Vec<usize> = waves.into_iter().filter(|&i| partners || gmfit::voice(i) == i || { eprintln!("bulk: {i} is a stereo partner, use `partners` (or --partners)"); false }).collect();
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let next = std::sync::atomic::AtomicUsize::new(0);
            let write = std::sync::Mutex::new(());
            eprintln!("bulk: {} waves, {jobs} jobs", waves.len());
            std::thread::scope(|sc| {
                for _ in 0..jobs {
                    sc.spawn(|| loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let Some(&i) = waves.get(k) else { break };
                        let o = orig.wave(i);
                        let t = std::time::Instant::now();
                        // One wave's panic must not end the batch: log it and go on.
                        let Ok((p, after)) = std::panic::catch_unwind(|| gmfit::fit::pipeline(&o, i, iters, rounds, 1)) else {
                            println!("{}", json!({"wave": i, "error": "panic"}));
                            continue;
                        };
                        let _g = write.lock().unwrap();
                        let wf = gmfit::load_wave(i).unwrap_or_else(|e| die(e));
                        // GMFIT_BULK_DUMP=<dir>: keep every pipeline result, written or not (listening, diagnosis).
                        if let Ok(dir) = std::env::var("GMFIT_BULK_DUMP") {
                            let d = gmgen::params_json::WaveFile { params: p.clone(), ..wf.clone() };
                            let _ = std::fs::write(format!("{dir}/{i:03}.json"), gmgen::params_json::to_json(&d));
                        }
                        let before = gmfit::fit::score_of(&o, i, &wf.params);
                        let kept = gmfit::keep_if_better(&orig, i, gmgen::params_json::WaveFile { params: p, ..wf }, after)
                            .unwrap_or_else(|e| die(e));
                        println!("{}", json!({"wave": i, "before": score::r4(before), "after": score::r4(after), "written": kept, "secs": t.elapsed().as_secs()}));
                    });
                }
            });
        }
        Some("partners") => {
            // partners: every stereo partner = its left wave's parameters (same phases and seeds: gmplus's channels
            // correlate 0.97–1.00) plus the analytic correction against the right-channel target. Written as a pair
            // decision, not by the partner's own rank. Then the stereo check row.
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let mut scores = gmfit::load_scores();
            for &(l, r) in &gmfit::skeleton().pairs {
                let (l, r) = (l as usize, r as usize);
                let left = gmfit::load_wave(l).unwrap_or_else(|e| die(e));
                let o = orig.wave(r);
                // different samples per side (target channels uncorrelated): the partner is fitted on its own
                // (`bulk --partners`); a copy of the left could not become another sample
                let m = gmfit::meta(l).unwrap();
                let tc = gmfit::stereo_check(&orig.wave(l), &o, &orig.wave(l), &o, &m)["target_corr"].as_f64().unwrap_or(1.0);
                if tc < 0.5 {
                    println!("{}", json!({"wave": l, "partner": r, "target_corr": tc, "independent": true}));
                    continue;
                }
                let copy = gmfit::fit::score_of(&o, r, &left.params);
                let (q, s) = gmfit::fit::correct_all(&o, r, &left.params);
                let params = if s > copy { q } else { left.params.clone() };
                let wf = gmgen::params_json::WaveFile { wave: r as u32, params, placeholder: false, note: format!("stereo partner of {l}: its parameters + correction") };
                wf.params.validate().unwrap_or_else(|e| die(format!("partner {r}: {e}")));
                std::fs::write(gmfit::wave_path(r), gmgen::params_json::to_json(&wf)).unwrap_or_else(|e| die(e));
                let on_disk = gmfit::load_wave(r).unwrap_or_else(|e| die(e));
                let (sc, _, gr) = gmfit::evaluate(&o, r, &on_disk.params);
                scores.insert(r.to_string(), sc.json());
                let mut row = gmfit::stereo_check(&orig.wave(l), &o, &gmfit::render_wave(l, &left.params), &gr, &m);
                row["wave"] = json!(l);
                row["partner"] = json!(r);
                row["right_score"] = json!(score::r4(sc.score));
                row["copy_score"] = json!(score::r4(copy));
                println!("{row}");
            }
            gmfit::save_scores(&scores).unwrap_or_else(|e| die(e));
        }
        Some("stereo") => {
            // stereo: every pair of the plus set — left alone and mid vs the mono target, loop correlation, flags.
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let pairs = gmfit::skeleton().pairs;
            let mut flagged = 0;
            for &(l, r) in &pairs {
                let (l, r) = (l as usize, r as usize);
                let (wl, wr) = (gmfit::load_wave(l).unwrap_or_else(|e| die(e)), gmfit::load_wave(r).unwrap_or_else(|e| die(e)));
                let m = gmfit::meta(l).unwrap();
                let (gl, gr) = (gmfit::render_wave(l, &wl.params), gmfit::render_wave(r, &wr.params));
                let mut row = gmfit::stereo_check(&orig.wave(l), &orig.wave(r), &gl, &gr, &m);
                row["wave"] = json!(l);
                row["partner"] = json!(r);
                row["placeholder"] = json!(wl.placeholder || wr.placeholder);
                flagged += !row["flags"].as_array().unwrap().is_empty() as usize;
                println!("{row}");
            }
            eprintln!("stereo: {} pairs, {flagged} flagged", pairs.len());
        }
        Some("dump") => {
            // dump <dir> [--secs s]: every wave of the set as <dir>/NNN-orig.wav + NNN-gen.wav (played for s seconds,
            // default 2) and <dir>/index.json (score, flags, placeholder, users) — the per-sound comparison page.
            let dir = std::path::PathBuf::from(args.get(1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| die("dump <dir>")));
            let secs: f32 = opt("--secs").map_or(2.0, |v| v.parse().unwrap_or_else(|_| die("--secs: number")));
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| die(e));
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let n = gmfit::wave_count();
            let next = std::sync::atomic::AtomicUsize::new(0);
            let rows = std::sync::Mutex::new(vec![Value::Null; n]);
            std::thread::scope(|sc| {
                for _ in 0..std::thread::available_parallelism().map_or(4, |n| n.get()) {
                    sc.spawn(|| loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        if i >= n {
                            break;
                        }
                        let wf = gmfit::load_wave(i).unwrap_or_else(|e| die(e));
                        let (o, m) = (orig.wave(i), gmfit::meta(i).unwrap());
                        let (sc, _, synth) = gmfit::evaluate(&o, i, &wf.params);
                        gmfit::audio::write_wav(&dir.join(format!("{i:03}-orig.wav")), &gmfit::audio::played(&o, &m, secs), m.rate).unwrap_or_else(|e| die(e));
                        gmfit::audio::write_wav(&dir.join(format!("{i:03}-gen.wav")), &gmfit::audio::played(&synth, &m, secs), m.rate).unwrap_or_else(|e| die(e));
                        let (names, cat) = gmfit::users(i);
                        let mut row = json!({"wave": i, "category": cat, "instruments": names, "rate": m.rate, "placeholder": wf.placeholder,
                            "score": score::r4(sc.score), "tier": gmfit::tier(&sc), "flags": sc.json()["flags"].clone()});
                        if let Some(t) = orig.targets() {
                            row["source"] = t.wave(&orig.dls(), i).map(|t| t.row["sample"].clone()).unwrap_or(Value::Null);
                        }
                        rows.lock().unwrap()[i] = row;
                    });
                }
            });
            let rows = rows.into_inner().unwrap();
            std::fs::write(dir.join("index.json"), serde_json::to_string(&rows).unwrap()).unwrap_or_else(|e| die(e));
            println!("{n} waves → {}", dir.display());
        }
        Some("plus-map") => {
            gmfit::select_set(gmfit::Set::Plus).unwrap_or_else(|e| die(e));
            let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
            let t = orig.targets().expect("plus set");
            let (path, summary) = t.write_map(&orig.dls()).unwrap_or_else(|e| die(e));
            println!("{}", json!({"map": path.display().to_string(), "summary": summary}));
            if summary["errors"].as_u64() != Some(0) {
                die("plus-map: some waves have no target (rows with \"error\" in the map)");
            }
        }
        Some("report") => report(flag("--check"), flag("--baseline")),
        _ => die("usage: gmfit extract | placeholder | analyze <w> | fit <w> | correct <w> | refine <w> | render <w> [-p f] | dump <dir> [--secs s] | hear <w> [-p f] | sweep <w> <param> <values> | bulk (--cat c | <w>...) [--iters n] [--rounds n] [--jobs n] | report [--check] [--baseline]"),
    }
}

fn report(check: bool, baseline: bool) {
    let orig = gmfit::Orig::load().unwrap_or_else(|e| die(e));
    let n = gmfit::wave_count();
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
                                let m = gmfit::meta(i).unwrap();
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
                *tiers.entry(gmfit::tier(&s)).or_default() += 1;
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
                        gmfit::tier(&s),
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
    let path = gmfit::build_dir().join("report.md");
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    std::fs::write(&path, md).unwrap_or_else(|e| die(e));
    let mut cells: Vec<(usize, f32, Vec<f32>, Vec<f32>)> =
        results.iter().filter_map(|(i, r)| r.as_ref().ok().map(|x| (*i, x.0.score, x.3.clone(), x.4.clone()))).collect();
    cells.sort_by(|a, b| a.1.total_cmp(&b.1));
    let rate = gmfit::meta(0).map_or(22050.0, |m| m.rate as f32);
    sheet::bank(&gmfit::build_dir().join("bank.png"), &cells, rate).unwrap_or_else(|e| die(e));
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
