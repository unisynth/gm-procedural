//! Per-wave model parameters: in-memory form, budget/range validation and the compact binary encoding embedded by
//! `build.rs` (little-endian f32). JSON form: `params_json.rs`.
use crate::schema::{self, Region, FAMILIES, MAX_CORRECTION, MAX_LAYERS, MAX_NUMBERS, WAVE};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// Index into `schema::FAMILIES`.
    pub family: usize,
    pub region: Region,
    /// Every scalar of the family, defaults filled in.
    pub scalars: Vec<f32>,
    /// Which scalars were given explicitly (they count toward the budget).
    pub explicit: Vec<bool>,
    /// One flattened table per family table (rows × width).
    pub tables: Vec<Vec<f32>>,
}

impl Layer {
    pub fn new(family: usize, region: Region) -> Layer {
        let f = &FAMILIES[family];
        Layer {
            family,
            region,
            scalars: f.scalars.iter().map(|s| s.default).collect(),
            explicit: alloc::vec![false; f.scalars.len()],
            tables: f.tables.iter().map(|_| Vec::new()).collect(),
        }
    }
    /// Scalar by name (panics on a name not in the family: a code bug, not data).
    /// Value of scalar `name`, or `None` when the family has no such scalar.
    pub fn try_get(&self, name: &str) -> Option<f32> {
        FAMILIES[self.family].scalars.iter().position(|s| s.name == name).map(|i| self.scalars[i])
    }
    pub fn get(&self, name: &str) -> f32 {
        self.scalars[schema::scalar(&FAMILIES[self.family], name).unwrap_or_else(|| panic!("no scalar {name}"))]
    }
    pub fn set(&mut self, name: &str, v: f32) {
        let i = schema::scalar(&FAMILIES[self.family], name).unwrap_or_else(|| panic!("no scalar {name}"));
        self.scalars[i] = v;
        self.explicit[i] = true;
    }
    pub fn table(&self, name: &str) -> &[f32] {
        let f = &FAMILIES[self.family];
        &self.tables[f.tables.iter().position(|t| t.name == name).unwrap_or_else(|| panic!("no table {name}"))]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// Wave-level scalars (`schema::WAVE`), defaults filled in.
    pub wave: Vec<f32>,
    pub wave_explicit: Vec<bool>,
    pub layers: Vec<Layer>,
    /// Harmonic correction, dB per harmonic 1..C (empty = none).
    pub correction: Vec<f32>,
    /// Time × band correction (`tf`), rows of `tf::BANDS` dB gains (empty = none).
    pub tf: Vec<f32>,
    /// Fine envelope correction (`env`): dB gains at the first `env.len()` of `schema::ENV_TIMES` (empty = none).
    pub env: Vec<f32>,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            wave: WAVE.iter().map(|s| s.default).collect(),
            wave_explicit: alloc::vec![false; WAVE.len()],
            layers: Vec::new(),
            correction: Vec::new(),
            tf: Vec::new(),
            env: Vec::new(),
        }
    }
}

impl Params {
    pub fn gain_db(&self) -> f32 {
        self.wave[0]
    }
    /// Fundamental MIDI pitch, or `None` for "use the wave's unity note".
    pub fn pitch(&self) -> Option<f32> {
        (self.wave[1] >= 0.0).then_some(self.wave[1])
    }

    /// Numbers counted toward the budget: explicit scalars, table entries, correction entries.
    pub fn numbers(&self) -> usize {
        let w = self.wave_explicit.iter().filter(|e| **e).count();
        let l: usize = self
            .layers
            .iter()
            .map(|l| l.explicit.iter().filter(|e| **e).count() + l.tables.iter().map(Vec::len).sum::<usize>())
            .sum();
        w + l + self.correction.len() + self.tf.len() + self.env.len()
    }

    /// Budget, table shape and range checks. The message names the offending part; callers prefix the wave.
    pub fn validate(&self) -> Result<(), String> {
        if self.layers.len() > MAX_LAYERS {
            return Err(format!("{} layers (max {MAX_LAYERS})", self.layers.len()));
        }
        let n = self.numbers();
        if n > MAX_NUMBERS {
            return Err(format!("{n} numbers (max {MAX_NUMBERS})"));
        }
        if self.correction.len() > MAX_CORRECTION {
            return Err(format!("{} correction entries (max {MAX_CORRECTION})", self.correction.len()));
        }
        for (v, s) in self.wave.iter().zip(WAVE) {
            if !(s.min..=s.max).contains(v) {
                return Err(format!("{} = {v} outside {}..{}", s.name, s.min, s.max));
            }
        }
        if !self.tf.len().is_multiple_of(crate::schema::TF_BANDS) || self.tf.len() / crate::schema::TF_BANDS > crate::schema::TF_TIMES.len() {
            return Err(format!("tf needs ≤ {} rows of {}", crate::schema::TF_TIMES.len(), crate::schema::TF_BANDS));
        }
        if self.env.len() > crate::schema::ENV_TIMES.len() {
            return Err(format!("env needs ≤ {} points", crate::schema::ENV_TIMES.len()));
        }
        if let Some(c) = self.env.iter().find(|c| !(-48.0..=24.0).contains(*c)) {
            return Err(format!("env {c} dB outside -48..24"));
        }
        if let Some(c) = self.tf.iter().find(|c| !(-24.0..=24.0).contains(*c)) {
            return Err(format!("tf {c} dB outside -24..24"));
        }
        if let Some(c) = self.correction.iter().find(|c| !(-24.0..=24.0).contains(*c)) {
            return Err(format!("correction {c} dB outside -24..24"));
        }
        for (k, l) in self.layers.iter().enumerate() {
            let f = &FAMILIES[l.family];
            if l.region == Region::Sustain && !f.periodic {
                return Err(format!("layer {k} ({}) cannot render the sustain region", f.name));
            }
            for (v, s) in l.scalars.iter().zip(f.scalars) {
                if !(s.min..=s.max).contains(v) || !v.is_finite() {
                    return Err(format!("layer {k} ({}) {} = {v} outside {}..{}", f.name, s.name, s.min, s.max));
                }
            }
            for (t, d) in l.tables.iter().zip(f.tables) {
                if t.len() % d.width != 0 || t.len() / d.width > d.max_rows {
                    return Err(format!("layer {k} ({}) table {} needs ≤ {} rows of {}", f.name, d.name, d.max_rows, d.width));
                }
                if t.iter().any(|v| !v.is_finite()) {
                    return Err(format!("layer {k} ({}) table {} has a non-finite value", f.name, d.name));
                }
            }
        }
        Ok(())
    }

    /// Binary form (decoded by [`Params::decode`]).
    pub fn encode(&self, out: &mut Vec<u8>) {
        let f32s = |out: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
        let bits = |out: &mut Vec<u8>, v: &[bool]| {
            let mut m = 0u64;
            v.iter().enumerate().for_each(|(i, b)| m |= (*b as u64) << i);
            out.extend_from_slice(&m.to_le_bytes());
        };
        f32s(out, &self.wave);
        bits(out, &self.wave_explicit);
        out.push(self.layers.len() as u8);
        for l in &self.layers {
            out.push(l.family as u8);
            out.push(l.region as u8);
            f32s(out, &l.scalars);
            bits(out, &l.explicit);
            for t in &l.tables {
                out.extend_from_slice(&(t.len() as u16).to_le_bytes());
                f32s(out, t);
            }
        }
        out.push(self.correction.len() as u8);
        f32s(out, &self.correction);
        out.push((self.tf.len() / crate::schema::TF_BANDS) as u8);
        f32s(out, &self.tf);
        out.push(self.env.len() as u8);
        f32s(out, &self.env);
    }

    /// Reads one encoded parameter set from the front of `b`, returning it and the remaining bytes.
    pub fn decode(b: &[u8]) -> Option<(Params, &[u8])> {
        struct R<'a>(&'a [u8]);
        impl R<'_> {
            fn take(&mut self, n: usize) -> Option<&[u8]> {
                let (a, b) = (self.0.get(..n)?, self.0.get(n..)?);
                self.0 = b;
                Some(a)
            }
            fn u8(&mut self) -> Option<u8> {
                Some(self.take(1)?[0])
            }
            fn f32s(&mut self, n: usize) -> Option<Vec<f32>> {
                Some(self.take(n * 4)?.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
            }
            fn bits(&mut self, n: usize) -> Option<Vec<bool>> {
                let m = u64::from_le_bytes(self.take(8)?.try_into().ok()?);
                Some((0..n).map(|i| m >> i & 1 != 0).collect())
            }
        }
        let mut r = R(b);
        let wave = r.f32s(WAVE.len())?;
        let wave_explicit = r.bits(WAVE.len())?;
        let mut layers = Vec::new();
        for _ in 0..r.u8()? {
            let family = r.u8()? as usize;
            let f = FAMILIES.get(family)?;
            let region = Region::from_u8(r.u8()?)?;
            let scalars = r.f32s(f.scalars.len())?;
            let explicit = r.bits(f.scalars.len())?;
            let mut tables = Vec::new();
            for _ in f.tables {
                let n = u16::from_le_bytes(r.take(2)?.try_into().ok()?) as usize;
                tables.push(r.f32s(n)?);
            }
            layers.push(Layer { family, region, scalars, explicit, tables });
        }
        let n = r.u8()? as usize;
        let correction = r.f32s(n)?;
        let rows = r.u8()? as usize;
        let tf = r.f32s(rows * crate::schema::TF_BANDS)?;
        let n = r.u8()? as usize;
        let env = r.f32s(n)?;
        Some((Params { wave, wave_explicit, layers, correction, tf, env }, r.0))
    }
}
