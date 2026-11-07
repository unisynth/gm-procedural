//! Zero-copy, zero-alloc DLS level 1/2 reader (`RIFF 'DLS '`), used to play the OS General MIDI
//! bank at runtime (Windows `drivers\gm.dls`, macOS CoreAudio `gs_instruments.dls`) without
//! shipping any sample data. Layout read:
//!
//! ```text
//! RIFF 'DLS ' { colh, LIST lins { LIST ins  { insh, LIST lrgn { LIST rgn |rgn2 { rgnh, wsmp?, wlnk, LIST lart? } },
//!                                             LIST lart|lar2?, LIST INFO? } },
//!               ptbl (cue offsets into wvpl), LIST wvpl { LIST wave { fmt , wsmp?, data } } }
//! ```
//!
//! Every read is bounds-checked; malformed data yields `Err`/`None`, never a panic.
#![cfg_attr(not(test), no_std)]

#[cfg(feature = "write")]
extern crate alloc;

#[cfg(test)]
mod tests;
#[cfg(feature = "write")]
pub mod write;
#[cfg(all(test, feature = "write"))]
mod write_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a `RIFF 'DLS '` file.
    NotDls,
    /// A chunk size runs past the end of the buffer.
    Truncated,
    /// Required top-level chunk absent (`colh`, `lins`, `ptbl`, `wvpl`).
    Missing([u8; 4]),
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// One RIFF chunk. For `LIST`, `kind` is the list type and `data` excludes it.
#[derive(Clone, Copy)]
struct Chunk<'a> {
    id: [u8; 4],
    kind: [u8; 4],
    data: &'a [u8],
}

/// Iterates sibling chunks; stops at the first malformed one (`bad` set).
#[derive(Clone)]
struct Chunks<'a> {
    rest: &'a [u8],
    bad: bool,
}

fn chunks(data: &[u8]) -> Chunks<'_> {
    Chunks { rest: data, bad: false }
}

impl<'a> Iterator for Chunks<'a> {
    type Item = Chunk<'a>;
    fn next(&mut self) -> Option<Chunk<'a>> {
        if self.rest.len() < 8 {
            return None;
        }
        let id: [u8; 4] = self.rest[..4].try_into().ok()?;
        let size = u32_at(self.rest, 4)? as usize;
        let Some(data) = self.rest.get(8..8 + size) else {
            self.bad = true;
            self.rest = &[];
            return None;
        };
        self.rest = self.rest.get(8 + size + (size & 1)..).unwrap_or(&[]);
        let (kind, data) = if &id == b"LIST" && data.len() >= 4 {
            (data[..4].try_into().ok()?, &data[4..])
        } else {
            ([0; 4], data)
        };
        Some(Chunk { id, kind, data })
    }
}

fn find<'a>(data: &'a [u8], id: &[u8; 4]) -> Option<&'a [u8]> {
    chunks(data).find(|c| &c.id == id).map(|c| c.data)
}
fn find_list<'a>(data: &'a [u8], kinds: &[&[u8; 4]]) -> Option<&'a [u8]> {
    chunks(data).find(|c| &c.id == b"LIST" && kinds.contains(&&c.kind)).map(|c| c.data)
}

/// Parsed bank: slices into the caller's buffer.
#[derive(Clone, Copy)]
pub struct Dls<'a> {
    count: u32,
    lins: &'a [u8],
    ptbl: &'a [u8],
    wvpl: &'a [u8],
    /// `LIST 'stpr' { pair }` data: `u32` (left, right) wave pairs (change add-gmplus-bank D5); empty when absent.
    pairs: &'a [u8],
}

impl<'a> Dls<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self, Error> {
        if file.len() < 12 || &file[..4] != b"RIFF" || &file[8..12] != b"DLS " {
            return Err(Error::NotDls);
        }
        let size = u32_at(file, 4).ok_or(Error::NotDls)? as usize;
        let body = file.get(12..8 + size).ok_or(Error::Truncated)?;
        let colh = find(body, b"colh").ok_or(Error::Missing(*b"colh"))?;
        let lins = find_list(body, &[b"lins"]).ok_or(Error::Missing(*b"lins"))?;
        let ptbl = find(body, b"ptbl").ok_or(Error::Missing(*b"ptbl"))?;
        let wvpl = find_list(body, &[b"wvpl"]).ok_or(Error::Missing(*b"wvpl"))?;
        let pairs = find_list(body, &[b"stpr"]).and_then(|l| find(l, b"pair")).unwrap_or(&[]);
        Ok(Dls { count: u32_at(colh, 0).unwrap_or(0), lins, ptbl, wvpl, pairs })
    }

    /// Instrument count declared in `colh`.
    pub fn instrument_count(&self) -> u32 {
        self.count
    }

    pub fn instruments(&self) -> impl Iterator<Item = Instrument<'a>> + use<'a> {
        chunks(self.lins).filter(|c| &c.id == b"LIST" && &c.kind == b"ins ").filter_map(|c| Instrument::parse(c.data))
    }

    /// GM lookup: `drums` = percussion bank flag, `bank` = CC0 MSB, `program` 0..=127.
    pub fn find(&self, drums: bool, bank: u8, program: u8) -> Option<Instrument<'a>> {
        self.instruments().find(|i| i.drums == drums && i.bank_msb == bank && i.program == program)
    }

    /// Number of cues in the pool table.
    pub fn wave_count(&self) -> u32 {
        u32_at(self.ptbl, 4).unwrap_or(0)
    }

    pub fn wave(&self, index: u32) -> Option<Wave<'a>> {
        if index >= self.wave_count() {
            return None;
        }
        let cb = u32_at(self.ptbl, 0)? as usize;
        let off = u32_at(self.ptbl, cb + index as usize * 4)? as usize;
        let c = chunks(self.wvpl.get(off..)?).next()?;
        if &c.id != b"LIST" || &c.kind != b"wave" {
            return None;
        }
        Wave::parse(c.data)
    }

    /// Raw `(left, right)` pairs of the stereo-pair chunk, unvalidated.
    pub fn pairs(&self) -> impl Iterator<Item = (u32, u32)> + use<'a> {
        let p = self.pairs;
        (0..p.len() / 8).filter_map(move |k| Some((u32_at(p, k * 8)?, u32_at(p, k * 8 + 4)?)))
    }

    /// Right-channel partner of wave `left`: the first pair naming it whose waves both parse and share rate, length
    /// and loop. Anything else is ignored (mono).
    pub fn partner(&self, left: u32) -> Option<u32> {
        let l = self.wave(left)?;
        self.pairs().filter(|&(a, _)| a == left).find_map(|(_, r)| {
            let w = self.wave(r)?;
            (r != left && w.rate == l.rate && w.len() == l.len() && w.wsmp.looped == l.wsmp.looped).then_some(r)
        })
    }

    /// Wave plus effective sample params (region `wsmp` overrides the wave's).
    pub fn sampler(&self, r: &Region<'a>) -> Option<(Wave<'a>, Wsmp)> {
        let w = self.wave(r.wave)?;
        Some((w, r.wsmp.unwrap_or(w.wsmp)))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Instrument<'a> {
    pub program: u8,
    pub bank_msb: u8,
    pub bank_lsb: u8,
    pub drums: bool,
    /// `INFO/INAM`, NUL trimmed.
    pub name: Option<&'a [u8]>,
    lrgn: &'a [u8],
    lart: &'a [u8],
}

impl<'a> Instrument<'a> {
    fn parse(d: &'a [u8]) -> Option<Self> {
        let insh = find(d, b"insh")?;
        let bank = u32_at(insh, 4)?;
        let name = find_list(d, &[b"INFO"]).and_then(|i| find(i, b"INAM")).map(|n| {
            let end = n.iter().position(|&b| b == 0).unwrap_or(n.len());
            &n[..end]
        });
        Some(Instrument {
            program: (u32_at(insh, 8)? & 0x7f) as u8,
            bank_msb: ((bank >> 8) & 0x7f) as u8,
            bank_lsb: (bank & 0x7f) as u8,
            drums: bank & 0x8000_0000 != 0,
            name,
            lrgn: find_list(d, &[b"lrgn"]).unwrap_or(&[]),
            lart: find_list(d, &[b"lart", b"lar2"]).unwrap_or(&[]),
        })
    }

    pub fn regions(&self) -> impl Iterator<Item = Region<'a>> + use<'a> {
        chunks(self.lrgn)
            .filter(|c| &c.id == b"LIST" && (&c.kind == b"rgn " || &c.kind == b"rgn2"))
            .filter_map(|c| Region::parse(c.data))
    }

    /// First region covering `key` and `vel`.
    pub fn region_for(&self, key: u8, vel: u8) -> Option<Region<'a>> {
        let (k, v) = (key as u16, vel as u16);
        self.regions().find(|r| (r.key_lo..=r.key_hi).contains(&k) && (r.vel_lo..=r.vel_hi).contains(&v))
    }

    /// Instrument-global articulation (`art1`/`art2` connection blocks).
    pub fn articulation(&self) -> Connections<'a> {
        Connections::new(self.lart)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Region<'a> {
    pub key_lo: u16,
    pub key_hi: u16,
    pub vel_lo: u16,
    pub vel_hi: u16,
    pub key_group: u16,
    /// Pool table index.
    pub wave: u32,
    /// Region-level override of the wave's `wsmp`.
    pub wsmp: Option<Wsmp>,
    lart: &'a [u8],
}

impl<'a> Region<'a> {
    fn parse(d: &'a [u8]) -> Option<Self> {
        let h = find(d, b"rgnh")?;
        let wlnk = find(d, b"wlnk")?;
        Some(Region {
            key_lo: u16_at(h, 0)?,
            key_hi: u16_at(h, 2)?,
            vel_lo: u16_at(h, 4)?,
            vel_hi: u16_at(h, 6)?,
            key_group: u16_at(h, 10)?,
            wave: u32_at(wlnk, 8)?,
            wsmp: find(d, b"wsmp").and_then(Wsmp::parse),
            lart: find_list(d, &[b"lart", b"lar2"]).unwrap_or(&[]),
        })
    }

    /// Region-level articulation (overrides the instrument's when non-empty).
    pub fn articulation(&self) -> Connections<'a> {
        Connections::new(self.lart)
    }
}

/// Sample playback parameters (`wsmp`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wsmp {
    pub unity_note: u16,
    /// Cents.
    pub fine_tune: i16,
    /// 1/65536 dB units (DLS `lGain`).
    pub gain: i32,
    /// `(start, length)` in samples of the first loop.
    pub looped: Option<(u32, u32)>,
}

impl Wsmp {
    fn parse(d: &[u8]) -> Option<Self> {
        let cb = u32_at(d, 0)? as usize;
        let loops = u32_at(d, 16)?;
        let looped = if loops > 0 { Some((u32_at(d, cb + 8)?, u32_at(d, cb + 12)?)) } else { None };
        Some(Wsmp { unity_note: u16_at(d, 4)?, fine_tune: u16_at(d, 6)? as i16, gain: u32_at(d, 8)? as i32, looped })
    }
}

/// One DLS connection block: `source`/`control` → `destination` with `scale` (DLS units).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Connection {
    pub source: u16,
    pub control: u16,
    pub destination: u16,
    pub transform: u16,
    pub scale: i32,
}

/// Iterates connection blocks of every `art1`/`art2` chunk in a `lart`/`lar2` list.
pub struct Connections<'a> {
    arts: Chunks<'a>,
    blocks: &'a [u8],
}

impl<'a> Connections<'a> {
    fn new(lart: &'a [u8]) -> Self {
        Connections { arts: chunks(lart), blocks: &[] }
    }
}

impl Iterator for Connections<'_> {
    type Item = Connection;
    fn next(&mut self) -> Option<Connection> {
        while self.blocks.len() < 12 {
            let c = self.arts.next()?;
            if &c.id == b"art1" || &c.id == b"art2" {
                let (cb, n) = (u32_at(c.data, 0)? as usize, u32_at(c.data, 4)? as usize);
                self.blocks = c.data.get(cb..cb + n * 12).unwrap_or(&[]);
            }
        }
        let b = self.blocks;
        self.blocks = &b[12..];
        Some(Connection {
            source: u16_at(b, 0)?,
            control: u16_at(b, 2)?,
            destination: u16_at(b, 4)?,
            transform: u16_at(b, 6)?,
            scale: u32_at(b, 8)? as i32,
        })
    }
}

/// Mono PCM wave (8-bit unsigned or 16-bit signed).
#[derive(Clone, Copy, Debug)]
pub struct Wave<'a> {
    pub rate: u32,
    pub bits: u16,
    pub wsmp: Wsmp,
    pub data: &'a [u8],
}

impl<'a> Wave<'a> {
    fn parse(d: &'a [u8]) -> Option<Self> {
        let f = find(d, b"fmt ")?;
        let (format, channels, bits) = (u16_at(f, 0)?, u16_at(f, 2)?, u16_at(f, 14)?);
        if format != 1 || channels != 1 || (bits != 8 && bits != 16) {
            return None;
        }
        Some(Wave {
            rate: u32_at(f, 4)?,
            bits,
            wsmp: find(d, b"wsmp").and_then(Wsmp::parse).unwrap_or_default(),
            data: find(d, b"data")?,
        })
    }

    /// Sample count.
    pub fn len(&self) -> usize {
        self.data.len() / (self.bits as usize / 8)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Sample `i` in `[-1, 1)`; out of range = 0.
    pub fn sample(&self, i: usize) -> f32 {
        if self.bits == 8 {
            self.data.get(i).map_or(0.0, |&s| (s as f32 - 128.0) / 128.0)
        } else {
            u16_at(self.data, i * 2).map_or(0.0, |s| s as i16 as f32 / 32768.0)
        }
    }
}
