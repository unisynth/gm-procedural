//! `assets/gm/structure.json` (format `gm-structure/1`): the Windows `gm.dls` bank structure without audio —
//! instruments (names, regions, articulation, `wsmp`) and per-wave rate, length and `wsmp`. One instrument / wave
//! per line so diffs stay readable. Shared by `gmfit extract` (writer) and the generator (`build.rs`, reader).
//! Plain `std` + `serde_json`; included by `build.rs` through `#[path]`.
use dls::write::{Bank, Ins, Rgn, WaveData};
use dls::{Connection, Wsmp};
use serde_json::{json, Value};

pub const FORMAT: &str = "gm-structure/1";

fn arts(c: &[Connection]) -> Value {
    Value::Array(c.iter().map(|k| json!([k.source, k.control, k.destination, k.transform, k.scale])).collect())
}

fn wsmp(s: &Wsmp) -> Value {
    match s.looped {
        Some((a, l)) => json!([s.unity_note, s.fine_tune, s.gain, a, l]),
        None => json!([s.unity_note, s.fine_tune, s.gain]),
    }
}

/// Bank structure as JSON text; wave samples are dropped (only their count is kept).
pub fn to_json(b: &Bank) -> String {
    let ins: Vec<String> = b
        .instruments
        .iter()
        .map(|i| {
            let regions: Vec<Value> = i
                .regions
                .iter()
                .map(|r| {
                    json!({"key": [r.key_lo, r.key_hi], "vel": [r.vel_lo, r.vel_hi], "group": r.key_group,
                        "wave": r.wave, "wsmp": r.wsmp.as_ref().map(wsmp), "art": arts(&r.articulation)})
                })
                .collect();
            json!({"drums": i.drums, "bank": [i.bank_msb, i.bank_lsb], "program": i.program,
                "name": String::from_utf8_lossy(&i.name), "art": arts(&i.articulation), "regions": regions})
            .to_string()
        })
        .collect();
    let waves: Vec<String> = b
        .waves
        .iter()
        .map(|w| json!({"rate": w.rate, "len": w.samples.len(), "wsmp": wsmp(&w.wsmp)}).to_string())
        .collect();
    // Stereo pairs (change add-gmplus-bank D4/D5) only when present: the gm.dls file stays byte-identical.
    let pairs = if b.pairs.is_empty() { String::new() } else { format!(",\n\"pairs\": {}", json!(b.pairs)) };
    format!(
        "{{\"format\": \"{FORMAT}\",\n\"instruments\": [\n{}\n],\n\"waves\": [\n{}\n]{pairs}}}\n",
        ins.join(",\n"),
        waves.join(",\n")
    )
}

fn num(v: &Value, what: &str) -> Result<i64, String> {
    v.as_i64().ok_or_else(|| format!("{what}: not an integer: {v}"))
}

fn arr<'a>(v: &'a Value, what: &str) -> Result<&'a Vec<Value>, String> {
    v.as_array().ok_or_else(|| format!("{what}: not an array"))
}

fn conns(v: &Value, what: &str) -> Result<Vec<Connection>, String> {
    arr(v, what)?
        .iter()
        .map(|c| {
            let c = arr(c, what)?;
            if c.len() != 5 {
                return Err(format!("{what}: connection needs 5 numbers"));
            }
            let n = |k: usize| num(&c[k], what);
            Ok(Connection {
                source: n(0)? as u16,
                control: n(1)? as u16,
                destination: n(2)? as u16,
                transform: n(3)? as u16,
                scale: n(4)? as i32,
            })
        })
        .collect()
}

fn parse_wsmp(v: &Value, what: &str) -> Result<Wsmp, String> {
    let a = arr(v, what)?;
    if a.len() != 3 && a.len() != 5 {
        return Err(format!("{what}: wsmp needs 3 or 5 numbers"));
    }
    let n = |k: usize| num(&a[k], what);
    Ok(Wsmp {
        unity_note: n(0)? as u16,
        fine_tune: n(1)? as i16,
        gain: n(2)? as i32,
        looped: if a.len() == 5 { Some((n(3)? as u32, n(4)? as u32)) } else { None },
    })
}

/// Structure JSON → bank description; every wave gets `len` zero samples.
pub fn from_json(text: &str) -> Result<Bank, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("structure.json: {e}"))?;
    if v["format"] != FORMAT {
        return Err(format!("structure.json: format is not {FORMAT}"));
    }
    let mut instruments = Vec::new();
    for (k, i) in arr(&v["instruments"], "instruments")?.iter().enumerate() {
        let what = format!("instrument {k}");
        let bank = arr(&i["bank"], &what)?;
        let mut regions = Vec::new();
        for r in arr(&i["regions"], &what)? {
            let key = arr(&r["key"], &what)?;
            let vel = arr(&r["vel"], &what)?;
            regions.push(Rgn {
                key_lo: num(&key[0], &what)? as u16,
                key_hi: num(&key[1], &what)? as u16,
                vel_lo: num(&vel[0], &what)? as u16,
                vel_hi: num(&vel[1], &what)? as u16,
                key_group: num(&r["group"], &what)? as u16,
                wave: num(&r["wave"], &what)? as u32,
                wsmp: if r["wsmp"].is_null() { None } else { Some(parse_wsmp(&r["wsmp"], &what)?) },
                articulation: conns(&r["art"], &what)?,
            });
        }
        instruments.push(Ins {
            drums: i["drums"].as_bool().ok_or_else(|| format!("{what}: drums"))?,
            bank_msb: num(&bank[0], &what)? as u8,
            bank_lsb: num(&bank[1], &what)? as u8,
            program: num(&i["program"], &what)? as u8,
            name: i["name"].as_str().unwrap_or_default().as_bytes().to_vec(),
            articulation: conns(&i["art"], &what)?,
            regions,
        });
    }
    let mut waves = Vec::new();
    for (k, w) in arr(&v["waves"], "waves")?.iter().enumerate() {
        let what = format!("wave {k}");
        waves.push(WaveData {
            rate: num(&w["rate"], &what)? as u32,
            wsmp: parse_wsmp(&w["wsmp"], &what)?,
            samples: vec![0; num(&w["len"], &what)? as usize],
        });
    }
    let mut pairs = Vec::new();
    if !v["pairs"].is_null() {
        for (k, p) in arr(&v["pairs"], "pairs")?.iter().enumerate() {
            let what = format!("pair {k}");
            let p = arr(p, &what)?;
            if p.len() != 2 {
                return Err(format!("{what}: needs [left, right]"));
            }
            let (l, r) = (num(&p[0], &what)?, num(&p[1], &what)?);
            if l as usize >= waves.len() || r as usize >= waves.len() {
                return Err(format!("{what}: wave out of range"));
            }
            pairs.push((l as u32, r as u32));
        }
    }
    Ok(Bank { instruments, waves, pairs })
}
