//! Host-only SoundFont 2.04 reader (change add-gmplus-bank, D2): sample headers, 16-bit sample data, preset and
//! instrument zones with generators and modulators. `Sf2::zones(bank, program, key, vel)` resolves every sounding
//! zone of a note: instrument generators (global zone, then local) are absolute, preset generators (global, then
//! local) are added on top, key and velocity ranges must match on both levels.

use std::fmt;

/// Generator ids used here (SF2 2.04 §8.1.2).
pub mod gn {
    pub const START_OFS: usize = 0;
    pub const END_OFS: usize = 1;
    pub const STARTLOOP_OFS: usize = 2;
    pub const ENDLOOP_OFS: usize = 3;
    pub const START_COARSE_OFS: usize = 4;
    pub const FILTER_FC: usize = 8;
    pub const FILTER_Q: usize = 9;
    pub const MOD_ENV_TO_FC: usize = 11;
    pub const MOD_ENV_TO_PITCH: usize = 7;
    pub const END_COARSE_OFS: usize = 12;
    pub const PAN: usize = 17;
    pub const INSTRUMENT: usize = 41;
    pub const KEY_RANGE: usize = 43;
    pub const VEL_RANGE: usize = 44;
    pub const STARTLOOP_COARSE_OFS: usize = 45;
    pub const ATTENUATION: usize = 48;
    pub const ENDLOOP_COARSE_OFS: usize = 50;
    pub const COARSE_TUNE: usize = 51;
    pub const FINE_TUNE: usize = 52;
    pub const SAMPLE_ID: usize = 53;
    pub const SAMPLE_MODES: usize = 54;
    pub const SCALE_TUNING: usize = 56;
    pub const ROOT_KEY: usize = 58;
    pub const COUNT: usize = 61;

    /// Generators that are absolute at instrument level and never summed at preset level.
    pub fn instrument_only(g: usize) -> bool {
        matches!(g, 0..=4 | 12 | 45 | 46 | 47 | 50 | 53 | 54 | 57 | 58)
    }
}

/// Default generator values (SF2 2.04 §8.1.3). Ranges are packed `lo | hi << 8`.
fn defaults() -> [i32; gn::COUNT] {
    let mut d = [0i32; gn::COUNT];
    d[gn::FILTER_FC] = 13500;
    for g in [21, 23, 25, 26, 27, 28, 30, 33, 34, 35, 36, 38] {
        d[g] = -12000;
    }
    d[gn::KEY_RANGE] = 127 << 8;
    d[gn::VEL_RANGE] = 127 << 8;
    d[46] = -1;
    d[47] = -1;
    d[gn::SCALE_TUNING] = 100;
    d[gn::ROOT_KEY] = -1;
    d
}

#[derive(Debug)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "sf2: {}", self.0)
    }
}
impl std::error::Error for Error {}
fn err<T>(m: impl Into<String>) -> Result<T, Error> {
    Err(Error(m.into()))
}

#[derive(Clone, Debug)]
pub struct SampleHdr {
    pub name: String,
    pub start: u32,
    pub end: u32,
    pub loop_start: u32,
    pub loop_end: u32,
    pub rate: u32,
    pub pitch: u8,
    pub correction: i8,
    pub link: u16,
    /// 1 mono, 2 right, 4 left, 8 linked; bit 15 ROM.
    pub kind: u16,
}

/// A modulator record (`pmod`/`imod`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modulator {
    pub src: u16,
    pub dest: u16,
    pub amount: i16,
    pub amt_src: u16,
    pub trans: u16,
}

impl Modulator {
    /// Same modulator identity (source, destination, amount source, transform): a later one replaces an earlier one.
    fn same(&self, o: &Modulator) -> bool {
        self.src == o.src && self.dest == o.dest && self.amt_src == o.amt_src && self.trans == o.trans
    }
}

#[derive(Clone, Debug, Default)]
struct Zone {
    gens: Vec<(u16, i32)>,
    mods: Vec<Modulator>,
}

impl Zone {
    fn get(&self, g: usize) -> Option<i32> {
        self.gens.iter().rev().find(|&&(id, _)| id as usize == g).map(|&(_, v)| v)
    }
}

#[derive(Clone, Debug)]
pub struct Preset {
    pub name: String,
    pub program: u16,
    pub bank: u16,
    zones: Vec<Zone>,
    global: Option<Zone>,
}

#[derive(Clone, Debug)]
struct Instrument {
    name: String,
    zones: Vec<Zone>,
    global: Option<Zone>,
}

/// A parsed soundfont.
pub struct Sf2 {
    pub samples: Vec<SampleHdr>,
    pub presets: Vec<Preset>,
    instruments: Vec<Instrument>,
    data: Vec<i16>,
}

/// A resolved zone of one note: final generator values, modulators and the sample.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub preset: String,
    pub instrument: String,
    /// Index of the instrument zone inside its instrument (local zones only).
    pub zone: usize,
    pub sample: usize,
    pub gens: [i32; gn::COUNT],
    pub mods: Vec<Modulator>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn name20(b: &[u8]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..n]).trim_end().to_string()
}

/// Sub-chunks `(id, body)` of a chunk body.
fn chunks(b: &[u8]) -> Result<Vec<([u8; 4], &[u8])>, Error> {
    let mut out = Vec::new();
    let mut o = 0;
    while o + 8 <= b.len() {
        let id = [b[o], b[o + 1], b[o + 2], b[o + 3]];
        let n = u32_at(b, o + 4) as usize;
        let s = o + 8;
        if s + n > b.len() {
            return err(format!("chunk {} overruns its parent", String::from_utf8_lossy(&id)));
        }
        out.push((id, &b[s..s + n]));
        o = s + n + (n & 1);
    }
    Ok(out)
}

fn records<'a>(b: &'a [u8], size: usize, what: &str) -> Result<Vec<&'a [u8]>, Error> {
    if b.len() % size != 0 || b.len() < size {
        return err(format!("{what}: size {} is not a positive multiple of {size}", b.len()));
    }
    Ok(b.chunks(size).collect())
}

/// Zones from bag/gen/mod tables; `last` = the terminal generator (instrument 41 / sampleID 53).
fn zones(bags: &[(usize, usize)], gens: &[(u16, i32)], mods: &[Modulator], last: usize) -> (Option<Zone>, Vec<Zone>) {
    let mut zs: Vec<Zone> = Vec::new();
    for w in bags.windows(2) {
        let (g0, m0) = w[0];
        let (g1, m1) = w[1];
        let gs = gens.get(g0..g1.min(gens.len())).unwrap_or(&[]).to_vec();
        let ms = mods.get(m0..m1.min(mods.len())).unwrap_or(&[]).to_vec();
        zs.push(Zone { gens: gs, mods: ms });
    }
    let mut global = None;
    if let Some(z) = zs.first() {
        if z.gens.last().map(|&(id, _)| id as usize) != Some(last) {
            global = Some(zs.remove(0));
        }
    }
    zs.retain(|z| z.gens.last().map(|&(id, _)| id as usize) == Some(last));
    (global, zs)
}

fn in_range(packed: i32, v: u8) -> bool {
    let lo = (packed & 0xff) as u8;
    let hi = ((packed >> 8) & 0xff) as u8;
    v >= lo && v <= hi
}

/// Generator amount: ranges stay packed bytes, everything else is a signed 16-bit value.
fn amount(id: u16, raw: u16) -> i32 {
    if id as usize == gn::KEY_RANGE || id as usize == gn::VEL_RANGE {
        raw as i32
    } else {
        raw as i16 as i32
    }
}

impl Sf2 {
    pub fn parse(b: &[u8]) -> Result<Sf2, Error> {
        if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"sfbk" {
            return err("not a RIFF sfbk file");
        }
        let n = (u32_at(b, 4) as usize).min(b.len() - 8);
        let mut smpl: &[u8] = &[];
        let mut pdta: Vec<([u8; 4], &[u8])> = Vec::new();
        for (id, body) in chunks(&b[12..8 + n])? {
            if &id != b"LIST" || body.len() < 4 {
                continue;
            }
            let sub = chunks(&body[4..])?;
            match &body[0..4] {
                b"sdta" => {
                    if let Some((_, s)) = sub.iter().find(|(i, _)| i == b"smpl") {
                        smpl = s;
                    }
                }
                b"pdta" => pdta = sub,
                _ => {}
            }
        }
        let get = |id: &[u8; 4]| -> Result<&[u8], Error> {
            pdta.iter().find(|(i, _)| i == id).map(|(_, s)| *s).ok_or_else(|| Error(format!("missing {}", String::from_utf8_lossy(id))))
        };
        let data: Vec<i16> = smpl.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();

        let gens_of = |id: &[u8; 4]| -> Result<Vec<(u16, i32)>, Error> {
            Ok(records(get(id)?, 4, "gen")?.iter().map(|r| {
                let g = u16_at(r, 0);
                (g, amount(g, u16_at(r, 2)))
            }).collect())
        };
        let mods_of = |id: &[u8; 4]| -> Result<Vec<Modulator>, Error> {
            Ok(records(get(id)?, 10, "mod")?
                .iter()
                .map(|r| Modulator { src: u16_at(r, 0), dest: u16_at(r, 2), amount: u16_at(r, 4) as i16, amt_src: u16_at(r, 6), trans: u16_at(r, 8) })
                .collect())
        };
        let bags_of = |id: &[u8; 4]| -> Result<Vec<(usize, usize)>, Error> {
            Ok(records(get(id)?, 4, "bag")?.iter().map(|r| (u16_at(r, 0) as usize, u16_at(r, 2) as usize)).collect())
        };
        let (pgen, pmod, pbag) = (gens_of(b"pgen")?, mods_of(b"pmod")?, bags_of(b"pbag")?);
        let (igen, imod, ibag) = (gens_of(b"igen")?, mods_of(b"imod")?, bags_of(b"ibag")?);

        let inst_recs = records(get(b"inst")?, 22, "inst")?;
        let mut instruments = Vec::new();
        for w in inst_recs.windows(2) {
            let (b0, b1) = (u16_at(w[0], 20) as usize, u16_at(w[1], 20) as usize);
            let bags = ibag.get(b0..=b1.min(ibag.len().saturating_sub(1))).unwrap_or(&[]);
            let (global, zones) = zones(bags, &igen, &imod, gn::SAMPLE_ID);
            instruments.push(Instrument { name: name20(&w[0][..20]), zones, global });
        }
        let ph = records(get(b"phdr")?, 38, "phdr")?;
        let mut presets = Vec::new();
        for w in ph.windows(2) {
            let (b0, b1) = (u16_at(w[0], 24) as usize, u16_at(w[1], 24) as usize);
            let bags = pbag.get(b0..=b1.min(pbag.len().saturating_sub(1))).unwrap_or(&[]);
            let (global, zones) = zones(bags, &pgen, &pmod, gn::INSTRUMENT);
            presets.push(Preset { name: name20(&w[0][..20]), program: u16_at(w[0], 20), bank: u16_at(w[0], 22), zones, global });
        }
        let sh = records(get(b"shdr")?, 46, "shdr")?;
        let samples = sh[..sh.len() - 1]
            .iter()
            .map(|r| SampleHdr {
                name: name20(&r[..20]),
                start: u32_at(r, 20),
                end: u32_at(r, 24),
                loop_start: u32_at(r, 28),
                loop_end: u32_at(r, 32),
                rate: u32_at(r, 36),
                pitch: r[40],
                correction: r[41] as i8,
                link: u16_at(r, 42),
                kind: u16_at(r, 44),
            })
            .collect();
        Ok(Sf2 { samples, presets, instruments, data })
    }

    pub fn open(path: &std::path::Path) -> Result<Sf2, Error> {
        let b = std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
        Sf2::parse(&b)
    }

    pub fn preset(&self, bank: u16, program: u16) -> Option<&Preset> {
        self.presets.iter().find(|p| p.bank == bank && p.program == program)
    }

    /// Every zone that sounds for (bank, program, key, velocity), in file order.
    pub fn zones(&self, bank: u16, program: u16, key: u8, vel: u8) -> Vec<Resolved> {
        let Some(p) = self.preset(bank, program) else { return Vec::new() };
        let mut out = Vec::new();
        let pg = p.global.as_ref();
        for pz in &p.zones {
            let prange = |g| pz.get(g).or_else(|| pg.and_then(|z| z.get(g))).unwrap_or(defaults()[g]);
            if !in_range(prange(gn::KEY_RANGE), key) || !in_range(prange(gn::VEL_RANGE), vel) {
                continue;
            }
            let Some(ii) = pz.get(gn::INSTRUMENT) else { continue };
            let Some(inst) = self.instruments.get(ii as usize) else { continue };
            let ig = inst.global.as_ref();
            for (zi, iz) in inst.zones.iter().enumerate() {
                let iget = |g| iz.get(g).or_else(|| ig.and_then(|z| z.get(g)));
                if !in_range(iget(gn::KEY_RANGE).unwrap_or(defaults()[gn::KEY_RANGE]), key)
                    || !in_range(iget(gn::VEL_RANGE).unwrap_or(defaults()[gn::VEL_RANGE]), vel)
                {
                    continue;
                }
                let Some(si) = iz.get(gn::SAMPLE_ID) else { continue };
                if si as usize >= self.samples.len() {
                    continue;
                }
                let mut g = defaults();
                for (k, v) in g.iter_mut().enumerate() {
                    if let Some(x) = iget(k) {
                        *v = x;
                    }
                }
                for (k, v) in g.iter_mut().enumerate() {
                    if gn::instrument_only(k) || k == gn::KEY_RANGE || k == gn::VEL_RANGE || k == gn::INSTRUMENT {
                        continue;
                    }
                    if let Some(x) = pz.get(k).or_else(|| pg.and_then(|z| z.get(k))) {
                        *v += x;
                    }
                }
                // Modulators: instrument global then local (local replaces identical), then preset ones add.
                let mut mods: Vec<Modulator> = Vec::new();
                for m in ig.map(|z| z.mods.as_slice()).unwrap_or(&[]).iter().chain(&iz.mods) {
                    if let Some(e) = mods.iter_mut().find(|e| e.same(m)) {
                        *e = *m;
                    } else {
                        mods.push(*m);
                    }
                }
                let mut pm: Vec<Modulator> = Vec::new();
                for m in pg.map(|z| z.mods.as_slice()).unwrap_or(&[]).iter().chain(&pz.mods) {
                    if let Some(e) = pm.iter_mut().find(|e| e.same(m)) {
                        *e = *m;
                    } else {
                        pm.push(*m);
                    }
                }
                mods.extend(pm);
                out.push(Resolved { preset: p.name.clone(), instrument: inst.name.clone(), zone: zi, sample: si as usize, gens: g, mods });
            }
        }
        out
    }

    /// Sample-data words `a..b` (absolute), as f32 in −1..1; clipped to the data.
    pub fn slice(&self, a: usize, b: usize) -> Vec<f32> {
        let b = b.min(self.data.len());
        self.data.get(a.min(b)..b).unwrap_or(&[]).iter().map(|&v| v as f32 / 32768.0).collect()
    }

    /// Raw 16-bit data of a sample header, as f32 in −1..1.
    pub fn sample_data(&self, s: &SampleHdr) -> Vec<f32> {
        let (a, b) = (s.start as usize, (s.end as usize).min(self.data.len()));
        self.data.get(a..b.max(a)).unwrap_or(&[]).iter().map(|&v| v as f32 / 32768.0).collect()
    }
}

/// Normalised value of a modulator source at note-on (`vel`, `key`); other controllers read 0 (CC rest values).
fn source(op: u16, vel: u8, key: u8) -> f32 {
    let index = op & 0x7f;
    let cc = op & 0x80 != 0;
    let raw = if cc {
        0.0
    } else {
        match index {
            0 => return 1.0, // no controller: constant 1
            2 => vel as f32,
            3 => key as f32,
            _ => 0.0,
        }
    };
    let mut x = raw / 128.0;
    if op & 0x100 != 0 {
        x = 1.0 - x;
    }
    let curve = |x: f32| -> f32 {
        match op >> 10 {
            1 => {
                if x >= 1.0 {
                    1.0
                } else {
                    (-(40.0 / 96.0) * (1.0 - x).log10()).clamp(0.0, 1.0)
                }
            }
            2 => {
                if x <= 0.0 {
                    0.0
                } else {
                    1.0 - (-(40.0 / 96.0) * x.log10()).clamp(0.0, 1.0)
                }
            }
            3 => {
                if x >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            _ => x,
        }
    };
    let y = curve(x);
    if op & 0x200 != 0 {
        2.0 * y - 1.0
    } else {
        y
    }
}

impl Resolved {
    /// Generator value plus every modulator on it at note-on (velocity, key).
    pub fn with_mods(&self, g: usize, vel: u8, key: u8) -> f32 {
        let mut v = self.gens[g] as f32;
        for m in self.mods.iter().filter(|m| m.dest as usize == g) {
            v += m.amount as f32 * source(m.src, vel, key) * source(m.amt_src, vel, key);
        }
        v
    }

    /// Effective root key.
    pub fn root(&self, s: &SampleHdr) -> i32 {
        if self.gens[gn::ROOT_KEY] >= 0 { self.gens[gn::ROOT_KEY] } else { s.pitch as i32 }
    }

    /// Pitch offset in cents from playing the sample at its own rate, for `key`.
    pub fn cents(&self, s: &SampleHdr, key: u8) -> f32 {
        (key as i32 - self.root(s)) as f32 * self.gens[gn::SCALE_TUNING] as f32
            + self.gens[gn::COARSE_TUNE] as f32 * 100.0
            + self.gens[gn::FINE_TUNE] as f32
            + s.correction as f32
    }

    /// Absolute (start, end, loop start, loop end) in sample-data words after the address offsets.
    pub fn bounds(&self, s: &SampleHdr) -> (i64, i64, i64, i64) {
        let g = &self.gens;
        let ofs = |fine: usize, coarse: usize| g[fine] as i64 + g[coarse] as i64 * 32768;
        (
            s.start as i64 + ofs(gn::START_OFS, gn::START_COARSE_OFS),
            s.end as i64 + ofs(gn::END_OFS, gn::END_COARSE_OFS),
            s.loop_start as i64 + ofs(gn::STARTLOOP_OFS, gn::STARTLOOP_COARSE_OFS),
            s.loop_end as i64 + ofs(gn::ENDLOOP_OFS, gn::ENDLOOP_COARSE_OFS),
        )
    }

    /// Looped (sample modes 1 or 3).
    pub fn looped(&self) -> bool {
        matches!(self.gens[gn::SAMPLE_MODES] & 3, 1 | 3)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Minimal SF2 writer for tests: presets (bank, program, name, zones), instruments, samples.
    pub struct Builder {
        pub samples: Vec<(String, Vec<i16>, u32, u8, i8, (u32, u32))>,
        /// Instrument: name, global gens, zones (gens incl. trailing sampleID), global mods, zone mods.
        pub insts: Vec<(String, Option<Vec<(u16, u16)>>, Vec<Vec<(u16, u16)>>, Vec<Modulator>)>,
        /// Preset: name, bank, program, global gens, zones (gens incl. trailing instrument).
        pub presets: Vec<(String, u16, u16, Option<Vec<(u16, u16)>>, Vec<Vec<(u16, u16)>>)>,
    }

    fn chunk(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut v = id.to_vec();
        v.extend((body.len() as u32).to_le_bytes());
        v.extend(body);
        if body.len() & 1 == 1 {
            v.push(0);
        }
        v
    }
    fn list(form: &[u8], subs: &[Vec<u8>]) -> Vec<u8> {
        let mut b = form.to_vec();
        for s in subs {
            b.extend(s);
        }
        chunk(b"LIST", &b)
    }
    fn name(n: &str) -> Vec<u8> {
        let mut v = n.as_bytes().to_vec();
        v.resize(20, 0);
        v
    }

    impl Builder {
        pub fn build(&self) -> Vec<u8> {
            let mut smpl = Vec::new();
            let mut shdr = Vec::new();
            let mut pos = 0u32;
            for (n, d, rate, pitch, corr, (ls, le)) in &self.samples {
                for v in d {
                    smpl.extend(v.to_le_bytes());
                }
                shdr.extend(name(n));
                for x in [pos, pos + d.len() as u32, pos + ls, pos + le, *rate] {
                    shdr.extend(x.to_le_bytes());
                }
                shdr.extend([*pitch, *corr as u8]);
                shdr.extend(0u16.to_le_bytes());
                shdr.extend(1u16.to_le_bytes());
                pos += d.len() as u32 + 46;
                smpl.extend(vec![0u8; 92]);
            }
            shdr.extend(name("EOS"));
            shdr.extend(vec![0u8; 26]);

            let mut inst = Vec::new();
            let mut ibag = Vec::new();
            let mut igen = Vec::new();
            let mut imod = Vec::new();
            let (mut ng, mut nm) = (0u16, 0u16);
            for (n, global, zones, gmods) in &self.insts {
                inst.extend(name(n));
                inst.extend(((ibag.len() / 4) as u16).to_le_bytes());
                let mut zs: Vec<(Vec<(u16, u16)>, Vec<Modulator>)> = Vec::new();
                if global.is_some() || !gmods.is_empty() {
                    zs.push((global.clone().unwrap_or_default(), gmods.clone()));
                }
                zs.extend(zones.iter().map(|z| (z.clone(), Vec::new())));
                for (z, ms) in zs {
                    ibag.extend(ng.to_le_bytes());
                    ibag.extend(nm.to_le_bytes());
                    for (g, a) in z {
                        igen.extend(g.to_le_bytes());
                        igen.extend(a.to_le_bytes());
                        ng += 1;
                    }
                    for m in ms {
                        for x in [m.src, m.dest, m.amount as u16, m.amt_src, m.trans] {
                            imod.extend(x.to_le_bytes());
                        }
                        nm += 1;
                    }
                }
            }
            inst.extend(name("EOI"));
            inst.extend(((ibag.len() / 4) as u16).to_le_bytes());
            ibag.extend(ng.to_le_bytes());
            ibag.extend(nm.to_le_bytes());
            igen.extend([0u8; 4]);
            imod.extend([0u8; 10]);

            let mut phdr = Vec::new();
            let mut pbag = Vec::new();
            let mut pgen = Vec::new();
            let mut ng = 0u16;
            for (n, bank, prog, global, zones) in &self.presets {
                phdr.extend(name(n));
                phdr.extend(prog.to_le_bytes());
                phdr.extend(bank.to_le_bytes());
                phdr.extend(((pbag.len() / 4) as u16).to_le_bytes());
                phdr.extend([0u8; 12]);
                let mut zs = Vec::new();
                if let Some(g) = global {
                    zs.push(g.clone());
                }
                zs.extend(zones.iter().cloned());
                for z in zs {
                    pbag.extend(ng.to_le_bytes());
                    pbag.extend(0u16.to_le_bytes());
                    for (g, a) in z {
                        pgen.extend(g.to_le_bytes());
                        pgen.extend(a.to_le_bytes());
                        ng += 1;
                    }
                }
            }
            phdr.extend(name("EOP"));
            phdr.extend([0u8; 4]);
            phdr.extend(((pbag.len() / 4) as u16).to_le_bytes());
            phdr.extend([0u8; 12]);
            pbag.extend(ng.to_le_bytes());
            pbag.extend(0u16.to_le_bytes());
            pgen.extend([0u8; 4]);
            let pmod = vec![0u8; 10];

            let info = list(b"INFO", &[chunk(b"ifil", &[2, 0, 4, 0])]);
            let sdta = list(b"sdta", &[chunk(b"smpl", &smpl)]);
            let pdta = list(
                b"pdta",
                &[
                    chunk(b"phdr", &phdr),
                    chunk(b"pbag", &pbag),
                    chunk(b"pmod", &pmod),
                    chunk(b"pgen", &pgen),
                    chunk(b"inst", &inst),
                    chunk(b"ibag", &ibag),
                    chunk(b"imod", &imod),
                    chunk(b"igen", &igen),
                    chunk(b"shdr", &shdr),
                ],
            );
            let mut body = b"sfbk".to_vec();
            body.extend(info);
            body.extend(sdta);
            body.extend(pdta);
            chunk(b"RIFF", &body)
        }
    }

    fn range(lo: u8, hi: u8) -> u16 {
        lo as u16 | (hi as u16) << 8
    }

    pub fn fixture() -> Vec<u8> {
        let ramp = |n: i16| (0..100).map(|k| k * n).collect::<Vec<i16>>();
        Builder {
            samples: vec![
                ("low".into(), ramp(1), 32000, 48, 0, (10, 90)),
                ("high".into(), ramp(2), 44100, 72, -7, (20, 80)),
                ("soft".into(), ramp(3), 22050, 60, 0, (0, 0)),
            ],
            insts: vec![(
                "Inst".into(),
                Some(vec![(gn::FILTER_FC as u16, 9000), (gn::SAMPLE_MODES as u16, 1)]),
                vec![
                    vec![(gn::KEY_RANGE as u16, range(0, 59)), (gn::VEL_RANGE as u16, range(64, 127)), (gn::ROOT_KEY as u16, 50), (gn::SAMPLE_ID as u16, 0)],
                    vec![(gn::KEY_RANGE as u16, range(60, 127)), (gn::STARTLOOP_OFS as u16, 5), (gn::FINE_TUNE as u16, (-10i16) as u16), (gn::SAMPLE_ID as u16, 1)],
                    vec![(gn::KEY_RANGE as u16, range(0, 59)), (gn::VEL_RANGE as u16, range(0, 63)), (gn::SAMPLE_MODES as u16, 0), (gn::SAMPLE_ID as u16, 2)],
                ],
                vec![Modulator { src: 0x0102, dest: gn::FILTER_FC as u16, amount: -2400, amt_src: 0, trans: 0 }],
            )],
            presets: vec![
                ("Piano".into(), 0, 0, Some(vec![(gn::COARSE_TUNE as u16, 1)]), vec![vec![(gn::FINE_TUNE as u16, 5), (gn::INSTRUMENT as u16, 0)]]),
                ("Kit".into(), 128, 0, None, vec![vec![(gn::KEY_RANGE as u16, range(35, 36)), (gn::INSTRUMENT as u16, 0)]]),
            ],
        }
        .build()
    }

    #[test]
    fn parses_headers() {
        let f = Sf2::parse(&fixture()).unwrap();
        assert_eq!(f.samples.len(), 3);
        assert_eq!(f.presets.len(), 2);
        assert_eq!(f.samples[1].name, "high");
        assert_eq!(f.samples[1].rate, 44100);
        assert_eq!(f.samples[1].correction, -7);
        let d = f.sample_data(&f.samples[1]);
        assert_eq!(d.len(), 100);
        assert_eq!(d[3], 6.0 / 32768.0);
    }

    #[test]
    fn key_and_velocity_split() {
        let f = Sf2::parse(&fixture()).unwrap();
        let s = |k, v| f.zones(0, 0, k, v).iter().map(|r| r.sample).collect::<Vec<_>>();
        assert_eq!(s(40, 100), vec![0]);
        assert_eq!(s(40, 30), vec![2]);
        assert_eq!(s(70, 100), vec![1]);
        assert!(f.zones(0, 5, 60, 100).is_empty(), "absent preset");
        assert!(f.zones(128, 0, 60, 100).is_empty(), "kit key range excludes 60");
        assert_eq!(f.zones(128, 0, 36, 100).len(), 1);
    }

    #[test]
    fn generators_resolve() {
        let f = Sf2::parse(&fixture()).unwrap();
        let r = &f.zones(0, 0, 40, 100)[0];
        let s = &f.samples[r.sample];
        assert_eq!(r.gens[gn::FILTER_FC], 9000, "global instrument zone");
        assert!(r.looped());
        assert_eq!(r.root(s), 50, "overriding root key");
        // (40 − 50)·100 + coarse 1·100 (preset global) + fine 5 (preset local) + correction 0
        assert_eq!(r.cents(s, 40), -1000.0 + 100.0 + 5.0);
        let r = &f.zones(0, 0, 70, 100)[0];
        let s = &f.samples[r.sample];
        assert_eq!(r.root(s), 72);
        assert_eq!(r.cents(s, 70), -200.0 + 100.0 + (-10.0 + 5.0) - 7.0);
        let (a, _, ls, le) = r.bounds(s);
        assert_eq!((ls - a, le - a), (25, 80), "loop offset applied");
        let r = &f.zones(0, 0, 40, 30)[0];
        assert!(!r.looped(), "local sample modes override the global zone");
    }

    #[test]
    fn velocity_modulator() {
        let f = Sf2::parse(&fixture()).unwrap();
        let r = &f.zones(0, 0, 40, 100)[0];
        // negative unipolar linear: (1 − 100/128) · −2400
        let want = 9000.0 - 2400.0 * (1.0 - 100.0 / 128.0);
        assert!((r.with_mods(gn::FILTER_FC, 100, 40) - want).abs() < 1e-3);
        assert_eq!(r.with_mods(gn::FILTER_Q, 100, 40), 0.0);
    }

    #[test]
    fn rejects_garbage() {
        assert!(Sf2::parse(b"RIFF\x04\0\0\0sfbx").is_err());
        let mut b = fixture();
        b.truncate(b.len() / 2);
        assert!(Sf2::parse(&b).is_err());
    }

    /// With `GMPLUS_SF2`: 235 presets, 398 mono samples.
    #[test]
    fn gmplus_when_present() {
        let Ok(p) = std::env::var("GMPLUS_SF2") else { return };
        let f = Sf2::open(std::path::Path::new(&p)).unwrap();
        assert_eq!(f.presets.len(), 235);
        assert_eq!(f.samples.len(), 398);
        assert!(f.samples.iter().all(|s| s.kind & 0x7fff == 1), "all mono");
        assert!(!f.zones(0, 0, 60, 100).is_empty(), "Piano 1 plays middle C");
        assert!(!f.zones(128, 0, 36, 100).is_empty(), "Standard kit plays the kick");
    }
}

