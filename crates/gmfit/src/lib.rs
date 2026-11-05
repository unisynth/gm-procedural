//! `gmfit`: fits the procedural GM bank (`crates/gmgen`) against the original Windows `gm.dls` (from `GM_DLS`, never
//! committed). Subcommands live in `main.rs`; shared helpers here.
use std::path::PathBuf;

/// Repository root (two levels above this crate).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `assets/gm/` in the repository.
pub fn assets_dir() -> PathBuf {
    repo_root().join("assets/gm")
}

/// The original bank bytes from `GM_DLS` (set and non-empty), or a message saying it is required.
pub fn original_bank() -> Result<Vec<u8>, String> {
    let p = std::env::var("GM_DLS").ok().filter(|p| !p.is_empty()).ok_or(
        "the original bank is required: set GM_DLS to a local copy of Windows gm.dls (never commit it)",
    )?;
    std::fs::read(&p).map_err(|e| format!("GM_DLS={p}: {e}"))
}

/// Placeholder parameters (task 3.5): one `additive` layer at the unity-note pitch with a gently falling
/// 8-harmonic table — `sustain` for looped waves, a decaying `whole` layer otherwise. Flagged `placeholder`.
pub fn placeholder(wave: u32, m: &gmgen::render::WaveMeta) -> gmgen::params_json::WaveFile {
    use gmgen::params::{Layer, Params};
    use gmgen::schema::{family, Region};
    let looped = m.looped.is_some_and(|(s, l)| l > 0 && (s + l) as usize <= m.len);
    let mut l = Layer::new(family("additive").expect("additive family"), if looped { Region::Sustain } else { Region::Whole });
    l.set("level_db", -12.0);
    // 5 ms fade-in, shortened when the loop starts earlier (the envelope freezes at the loop start).
    let start = m.looped.map_or(m.len as u32, |(s, _)| s) as f32 / m.rate as f32;
    l.set("a", (0.005f32).min(start * 0.5));
    if !looped {
        l.set("d", 0.25);
        l.set("s", 0.0);
    }
    l.tables[0] = vec![0.0, -6.0, -10.0, -13.0, -16.0, -18.0, -20.0, -22.0];
    let mut p = Params::default();
    p.layers.push(l);
    gmgen::params_json::WaveFile { wave, params: p, placeholder: true, note: String::new() }
}

pub mod analyze;
pub mod audio;
pub mod cmaes;
pub mod desc;
pub mod fit;
pub mod fm;
pub mod formant;
pub mod hear;
pub mod modal;
pub mod plan;
pub mod wavelet;
pub mod priors;
pub mod score;
pub mod shaker;
pub mod sheet;
pub mod sf2;

use gmgen::params::Params;
use gmgen::params_json::WaveFile;
use serde_json::{json, Value};

/// GM category names by program / 8.
pub const CATEGORIES: [&str; 16] = [
    "piano", "chromatic", "organ", "guitar", "bass", "strings", "ensemble", "brass", "reed", "pipe", "lead", "pad",
    "synthfx", "ethnic", "percussive", "sfx",
];

/// The original bank, loaded once from `GM_DLS`.
pub struct Orig {
    bytes: Vec<u8>,
}

impl Orig {
    pub fn load() -> Result<Orig, String> {
        let bytes = original_bank()?;
        dls::Dls::parse(&bytes).map_err(|e| format!("GM_DLS is not a DLS bank: {e:?}"))?;
        Ok(Orig { bytes })
    }
    pub fn dls(&self) -> dls::Dls<'_> {
        dls::Dls::parse(&self.bytes).expect("checked at load")
    }
    pub fn wave(&self, i: usize) -> Vec<f32> {
        audio::original_wave(&self.dls(), i)
    }
}

/// Instruments using a wave: names ("drums:<kit> key N" for kits) and the GM category of the first user.
pub fn users(i: usize) -> (Vec<String>, &'static str) {
    let bank = gmgen::bank::skeleton();
    let mut names = Vec::new();
    let mut cat = "unused";
    for ins in &bank.instruments {
        for r in &ins.regions {
            if r.wave as usize != i {
                continue;
            }
            let name = String::from_utf8_lossy(&ins.name).trim().to_string();
            let label = if ins.drums {
                format!("{name} (kit) keys {}-{}", r.key_lo, r.key_hi)
            } else {
                format!("{name} (bank {} prog {})", ins.bank_msb, ins.program)
            };
            if !names.contains(&label) {
                names.push(label);
            }
            if cat == "unused" || (cat != "drums" && ins.drums && ins.bank_msb == 0) {
                cat = if ins.drums { "drums" } else { CATEGORIES[(ins.program / 8) as usize] };
            }
        }
    }
    (names, cat)
}

/// How the bank plays wave `i`: `(key, speed, instrument)`. Speed = 2^((key − unity)/12 + fine/1200) with the
/// region's own unity/fine (falling back to the wave's). The key: a drum kit's key for the wave (Standard kit
/// first); else the main melodic user (bank 0 first), at the unity note when its key range holds it, otherwise
/// the nearest key of the range. Listening pages play waves at this speed (kick 1.0, belltree 0.71).
pub fn play_speed(i: usize) -> (u8, f32, String) {
    let bank = gmgen::bank::skeleton();
    let wave_ws = gmgen::bank::meta(i).map(|m| (m.unity, m.fine));
    let mut best: Option<(u8, (u8, f32, String))> = None;
    for ins in &bank.instruments {
        for r in ins.regions.iter().filter(|r| r.wave as usize == i) {
            let (unity, fine) = r.wsmp.as_ref().map(|w| (w.unity_note, w.fine_tune)).or(wave_ws).unwrap_or((60, 0));
            let (lo, hi) = (r.key_lo.min(127), r.key_hi.min(127));
            let (rank, key) = if ins.drums {
                (if ins.program == 0 { 0 } else { 1 }, lo)
            } else {
                (if ins.bank_msb == 0 { 2 } else { 3 }, unity.clamp(lo, hi.max(lo)))
            };
            if best.as_ref().is_none_or(|b| rank < b.0) {
                let speed = 2f32.powf((key as f32 - unity as f32) / 12.0 + fine as f32 / 1200.0);
                let name = String::from_utf8_lossy(&ins.name).trim().to_string();
                best = Some((rank, (key as u8, speed, name)));
            }
        }
    }
    best.map_or((60, 1.0, String::new()), |b| b.1)
}

pub fn wave_path(i: usize) -> PathBuf {
    assets_dir().join("waves").join(format!("{i:03}.json"))
}

/// Parameter file of wave `i` (read at run time, so edits need no rebuild).
pub fn load_wave(i: usize) -> Result<WaveFile, String> {
    let p = wave_path(i);
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    gmgen::params_json::from_json(&text).map_err(|e| format!("{}: {e}", p.display()))
}

pub fn out_dir(i: usize) -> PathBuf {
    let d = repo_root().join("build/gm").join(format!("{i:03}"));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Renders `p` for wave `i` and scores it with and without the corrections (harmonic, `tf`, `env`).
pub fn evaluate(orig: &[f32], i: usize, p: &Params) -> (score::Score, Option<score::Score>, Vec<f32>) {
    let m = gmgen::bank::meta(i).expect("wave meta");
    let synth = gmgen::render_wave(i, p);
    let mut s = score::score(orig, &synth, &m);
    s.describe(orig, &synth, &m);
    if !s.flags.contains(&"nan") {
        let speed = play_speed(i).1;
        let (a, b) = (audio::played(orig, &m, 1.0), audio::played(&synth, &m, 1.0));
        let h = hear::hear(&hear::at_speed(&a, speed), &hear::at_speed(&b, speed), m.rate as f32);
        s.heard = Some([h.added, h.missing, h.moddiff, h.rough]);
    }
    let plain = (!p.correction.is_empty() || !p.tf.is_empty() || !p.env.is_empty()).then(|| {
        let mut q = p.clone();
        q.correction.clear();
        q.tf.clear();
        q.env.clear();
        score::score(orig, &gmgen::render_wave(i, &q), &m)
    });
    (s, plain, synth)
}

/// Committed scores (`assets/gm/scores.json`): wave → score object.
pub fn load_scores() -> serde_json::Map<String, Value> {
    std::fs::read_to_string(assets_dir().join("scores.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Writes `assets/gm/scores.json`, one wave per line.
pub fn save_scores(m: &serde_json::Map<String, Value>) -> Result<(), String> {
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort_by_key(|k| k.parse::<usize>().unwrap_or(usize::MAX));
    let lines: Vec<String> = keys.iter().map(|k| format!("{}: {}", json!(k), m[*k])).collect();
    std::fs::write(assets_dir().join("scores.json"), format!("{{\n{}\n}}\n", lines.join(",\n"))).map_err(|e| e.to_string())
}

/// Tier of a score (design D6, calibrated on the pilot listening, task 6.2): the approved pilot waves scored
/// 0.71–0.89. A texture flag heard as a defect (buzz, clipping, loop click, silence, NaN) keeps a wave in C.
pub fn tier(s: &score::Score) -> &'static str {
    let defect = s.flags.iter().any(|f| matches!(*f, "buzz" | "clipped" | "loop_click" | "silent" | "nan"));
    if defect || s.score < 0.70 {
        "C"
    } else if s.score >= 0.85 {
        "A"
    } else {
        "B"
    }
}

/// Sets one parameter by path: `gain_db`, `pitch`, `<layer>.<scalar>`, `<layer>.<table>.<index>`,
/// `correction.<index>` (tables and the correction grow with zeros as needed).
pub fn set_param(p: &mut Params, path: &str, v: f32) -> Result<(), String> {
    let parts: Vec<&str> = path.split('.').collect();
    let fams = gmgen::schema::FAMILIES;
    match parts.as_slice() {
        [w] => {
            let k = gmgen::schema::WAVE.iter().position(|s| s.name == *w).ok_or(format!("unknown wave scalar {w}"))?;
            p.wave[k] = v;
            p.wave_explicit[k] = true;
        }
        ["correction", k] => {
            let k: usize = k.parse().map_err(|_| "bad index")?;
            if p.correction.len() <= k {
                p.correction.resize(k + 1, 0.0);
            }
            p.correction[k] = v;
        }
        [l, name] => {
            let l = p.layers.get_mut(l.parse::<usize>().map_err(|_| "bad layer")?).ok_or("no such layer")?;
            let f = &fams[l.family];
            let k = gmgen::schema::scalar(f, name).ok_or(format!("{} has no scalar {name}", f.name))?;
            l.scalars[k] = v;
            l.explicit[k] = true;
        }
        [l, t, k] => {
            let l = p.layers.get_mut(l.parse::<usize>().map_err(|_| "bad layer")?).ok_or("no such layer")?;
            let f = &fams[l.family];
            let ti = f.tables.iter().position(|x| x.name == *t).ok_or(format!("{} has no table {t}", f.name))?;
            let k: usize = k.parse().map_err(|_| "bad index")?;
            if l.tables[ti].len() <= k {
                l.tables[ti].resize(k + 1, 0.0);
            }
            l.tables[ti][k] = v;
        }
        _ => return Err(format!("bad parameter path {path}")),
    }
    p.validate()
}

/// Character flags (design D4n) and the score cost of each when choosing between versions of a wave.
pub const CHAR_FLAGS: [&str; 3] = ["lines", "transient_off", "peaks_off"];
pub const CHAR_PENALTY: f32 = 0.1;
/// Weight of the audible difference (design D4p) in the rank.
pub const HEAR_PENALTY: f32 = 0.5;

/// Audible excess of a hearing-model difference [added, missing, moddiff] over its noise floor (an independent take
/// of the same noise): 0 for every approved pilot wave.
/// Roughness difference beyond `ROUGH_AUDIBLE` counts 0.1 per dB.
pub fn hear_excess(h: [f32; 4]) -> f32 {
    (h[0] + h[1] - hear::AUDIBLE).max(0.0) + (h[2] - hear::MOD_AUDIBLE).max(0.0) + 0.1 * (h[3].abs() - hear::ROUGH_AUDIBLE).max(0.0)
}

/// Rank of a scored version: score − `CHAR_PENALTY` per character flag − `HEAR_PENALTY` × audible excess. A version
/// that clears a character defect, or that a listener hears closer, wins over one the frame score prefers (it
/// rewards a dense wash matching the average spectrum and envelope over the loud single events it loses). Needs
/// `evaluate` (descriptors and hearing model).
pub fn rank(s: &score::Score) -> f32 {
    s.score
        - CHAR_PENALTY * s.flags.iter().filter(|f| CHAR_FLAGS.contains(f)).count() as f32
        - HEAR_PENALTY * s.heard.map_or(0.0, hear_excess)
}

/// Same from a score JSON object (`scores.json` row).
pub fn rank_json(v: &Value) -> Option<f32> {
    let n = v["flags"].as_array().map_or(0, |a| a.iter().filter(|f| f.as_str().is_some_and(|f| CHAR_FLAGS.contains(&f))).count());
    let h = v["hear"].as_array().filter(|a| a.len() >= 3).map(|a| {
        let g = |k: usize| a.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        hear_excess([g(0), g(1), g(2), g(3)])
    });
    Some(v["score"].as_f64()? as f32 - CHAR_PENALTY * n as f32 - HEAR_PENALTY * h.unwrap_or(0.0))
}

/// `rank` of parameters `p` for wave `i` (renders, scores, describes).
pub fn rank_of(orig: &[f32], i: usize, p: &Params) -> f32 {
    rank(&evaluate(orig, i, p).0)
}

/// Waves whose new rank (`rank_json`: score − flags − audible excess) is more than 0.005 below the committed one
/// (`report --check`).
pub fn regressions(committed: &serde_json::Map<String, Value>, new: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut out: Vec<(usize, String)> = new
        .iter()
        .filter_map(|(k, v)| {
            let (old, now) = (rank_json(committed.get(k)?)?, rank_json(v)?);
            (now < old - 0.005).then(|| (k.parse().unwrap_or(0), format!("wave {k}: {old:.4} → {now:.4}")))
        })
        .collect();
    out.sort();
    out.into_iter().map(|x| x.1).collect()
}

/// Writes `wf` for wave `i` when its rank (score − 0.1 per character flag) beats the committed one (scores.json
/// when it has the descriptors, else the current file's), and records the new score. `_score` (the caller's plain
/// score) is not trusted: the version is re-evaluated with descriptors. Returns whether it was written.
pub fn keep_if_better(orig: &Orig, i: usize, mut wf: WaveFile, _score: f32) -> Result<bool, String> {
    let mut scores = load_scores();
    let o = orig.wave(i);
    let old = match scores.get(&i.to_string()).filter(|v| ["desc", "hear"].iter().all(|k| v.get(*k).is_some_and(|d| !d.is_null()))).and_then(rank_json) {
        Some(r) => r,
        None => load_wave(i).map(|w| rank_of(&o, i, &w.params)).unwrap_or(f32::MIN),
    };
    // Rank what will be on disk: the file rounds numbers to 5 digits (it can cost a modal whistle 0.1).
    wf.params = gmgen::params_json::from_json(&gmgen::params_json::to_json(&wf))?.params;
    if rank_of(&o, i, &wf.params) <= old + 1e-4 {
        return Ok(false);
    }
    wf.placeholder = false;
    wf.params.validate()?;
    std::fs::write(wave_path(i), gmgen::params_json::to_json(&wf)).map_err(|e| e.to_string())?;
    // Re-read: the file rounds numbers to 5 digits; record the score of what is on disk.
    let on_disk = load_wave(i)?;
    let s = evaluate(&orig.wave(i), i, &on_disk.params).0;
    scores.insert(i.to_string(), s.json());
    save_scores(&scores)?;
    Ok(true)
}
