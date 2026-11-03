//! `assets/gm/waves/NNN.json` ⇄ [`Params`]:
//!
//! ```json
//! {"wave": 289, "gain_db": -3, "pitch": 36.1, "placeholder": false, "note": "free text",
//!  "layers": [{"family": "additive", "region": "sustain", "p": {"level_db": 0, "d": 0.4}, "t": {"harm": [0, -6]}}],
//!  "correction": [0, 1.5, -2]}
//! ```
//! Only explicit scalars are written; omitted ones take the schema default. Tables of width > 1 are arrays of rows.
use crate::params::{Layer, Params};
use crate::schema::{self, Region, FAMILIES, WAVE};
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct WaveFile {
    pub wave: u32,
    pub params: Params,
    /// Set by `gmfit placeholder`; cleared when a real fit is written.
    pub placeholder: bool,
    pub note: String,
}

fn f(v: &Value, what: &str) -> Result<f32, String> {
    v.as_f64().map(|x| x as f32).ok_or_else(|| format!("{what}: not a number: {v}"))
}

pub fn from_json(text: &str) -> Result<WaveFile, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let o = v.as_object().ok_or("not an object")?;
    let wave = o.get("wave").and_then(Value::as_u64).ok_or("missing \"wave\" index")? as u32;
    let mut p = Params::default();
    for (i, s) in WAVE.iter().enumerate() {
        if let Some(x) = o.get(s.name) {
            p.wave[i] = f(x, s.name)?;
            p.wave_explicit[i] = true;
        }
    }
    for key in o.keys() {
        if !matches!(key.as_str(), "wave" | "placeholder" | "note" | "layers" | "correction" | "tf")
            && WAVE.iter().all(|s| s.name != key)
        {
            return Err(format!("unknown key \"{key}\""));
        }
    }
    for (k, l) in o.get("layers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().enumerate() {
        let fam_name = l["family"].as_str().ok_or_else(|| format!("layer {k}: missing family"))?;
        let fam = schema::family(fam_name).ok_or_else(|| format!("layer {k}: unknown family \"{fam_name}\""))?;
        let region_name = l["region"].as_str().unwrap_or("attack");
        let region = Region::from_name(region_name).ok_or_else(|| format!("layer {k}: unknown region \"{region_name}\""))?;
        let mut layer = Layer::new(fam, region);
        let fd = &FAMILIES[fam];
        if let Some(ps) = l.get("p").and_then(Value::as_object) {
            for (name, x) in ps {
                let i = schema::scalar(fd, name).ok_or_else(|| format!("layer {k} ({fam_name}): unknown parameter \"{name}\""))?;
                layer.scalars[i] = f(x, name)?;
                layer.explicit[i] = true;
            }
        }
        if let Some(ts) = l.get("t").and_then(Value::as_object) {
            for (name, rows) in ts {
                let ti = fd
                    .tables
                    .iter()
                    .position(|t| t.name == name)
                    .ok_or_else(|| format!("layer {k} ({fam_name}): unknown table \"{name}\""))?;
                let width = fd.tables[ti].width;
                let rows = rows.as_array().ok_or_else(|| format!("table {name}: not an array"))?;
                let mut flat = Vec::new();
                for r in rows {
                    if width == 1 {
                        flat.push(f(r, name)?);
                    } else {
                        let r = r.as_array().filter(|r| r.len() == width).ok_or_else(|| format!("table {name}: rows of {width}"))?;
                        for x in r {
                            flat.push(f(x, name)?);
                        }
                    }
                }
                layer.tables[ti] = flat;
            }
        }
        p.layers.push(layer);
    }
    if let Some(c) = o.get("correction").and_then(Value::as_array) {
        p.correction = c.iter().map(|x| f(x, "correction")).collect::<Result<_, _>>()?;
    }
    if let Some(rows) = o.get("tf").and_then(Value::as_array) {
        for r in rows {
            let r = r.as_array().filter(|r| r.len() == crate::schema::TF_BANDS).ok_or(format!("tf: rows of {}", crate::schema::TF_BANDS))?;
            for x in r {
                p.tf.push(f(x, "tf")?);
            }
        }
    }
    p.validate()?;
    Ok(WaveFile {
        wave,
        params: p,
        placeholder: o.get("placeholder").and_then(Value::as_bool).unwrap_or(false),
        note: o.get("note").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

/// Rounds to 5 significant digits so files stay short and diffs readable (re-validated after rounding).
fn num(x: f32) -> Value {
    let r: f64 = format!("{:.5e}", x).parse().unwrap_or(x as f64);
    json!(r)
}

pub fn to_json(w: &WaveFile) -> String {
    let mut o = Map::new();
    o.insert("wave".into(), json!(w.wave));
    for (i, s) in WAVE.iter().enumerate() {
        if w.params.wave_explicit[i] {
            o.insert(s.name.into(), num(w.params.wave[i]));
        }
    }
    if w.placeholder {
        o.insert("placeholder".into(), json!(true));
    }
    if !w.note.is_empty() {
        o.insert("note".into(), json!(w.note));
    }
    let layers: Vec<Value> = w
        .params
        .layers
        .iter()
        .map(|l| {
            let fd = &FAMILIES[l.family];
            let mut p = Map::new();
            for (i, s) in fd.scalars.iter().enumerate() {
                if l.explicit[i] {
                    p.insert(s.name.into(), num(l.scalars[i]));
                }
            }
            let mut t = Map::new();
            for (td, flat) in fd.tables.iter().zip(&l.tables) {
                if flat.is_empty() {
                    continue;
                }
                let rows: Vec<Value> = if td.width == 1 {
                    flat.iter().map(|x| num(*x)).collect()
                } else {
                    flat.chunks(td.width).map(|r| Value::Array(r.iter().map(|x| num(*x)).collect())).collect()
                };
                t.insert(td.name.into(), Value::Array(rows));
            }
            json!({"family": fd.name, "region": l.region.name(), "p": p, "t": t})
        })
        .collect();
    o.insert("layers".into(), Value::Array(layers));
    if !w.params.correction.is_empty() {
        o.insert("correction".into(), Value::Array(w.params.correction.iter().map(|x| num(*x)).collect()));
    }
    if !w.params.tf.is_empty() {
        let rows = w.params.tf.chunks(crate::schema::TF_BANDS).map(|r| Value::Array(r.iter().map(|x| num(*x)).collect()));
        o.insert("tf".into(), Value::Array(rows.collect()));
    }
    let mut s = String::new();
    pretty(&Value::Object(o), 0, &mut s);
    s.push('\n');
    s
}

/// Objects indented one key per line; arrays of numbers (or of number rows) and scalars stay on one line.
fn pretty(v: &Value, ind: usize, out: &mut String) {
    let pad = |n: usize| "  ".repeat(n);
    match v {
        Value::Object(o) if !o.is_empty() => {
            out.push_str("{\n");
            for (i, (k, x)) in o.iter().enumerate() {
                out.push_str(&format!("{}{}: ", pad(ind + 1), json!(k)));
                pretty(x, ind + 1, out);
                out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{}}}", pad(ind)));
        }
        Value::Array(a) if a.iter().any(Value::is_object) => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                out.push_str(&pad(ind + 1));
                pretty(x, ind + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{}]", pad(ind)));
        }
        Value::Array(_) => out.push_str(&v.to_string().replace(',', ", ")),
        _ => out.push_str(&v.to_string()),
    }
}
