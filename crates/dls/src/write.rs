//! DLS level 1 writer (feature `write`): serialises a [`Bank`] description into a `RIFF 'DLS '` buffer that
//! [`Dls::parse`] reads back field for field. Waves are written as mono 16-bit PCM. [`Bank::read`] builds the
//! description from a parsed bank (8-bit waves widened to 16-bit), so read → write → read is lossless.
use crate::{Connection, Dls, Wsmp};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bank {
    pub instruments: Vec<Ins>,
    /// Pool order: `Rgn::wave` indexes this.
    pub waves: Vec<WaveData>,
    /// Stereo pairs `(left, right)` (`LIST 'stpr' { pair }`, change add-gmplus-bank D5); empty = no chunk.
    pub pairs: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ins {
    pub drums: bool,
    pub bank_msb: u8,
    pub bank_lsb: u8,
    pub program: u8,
    /// `INFO/INAM` without NUL; empty = no `INFO` list.
    pub name: Vec<u8>,
    /// Instrument-global `lart/art1`; empty = no `lart`.
    pub articulation: Vec<Connection>,
    pub regions: Vec<Rgn>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgn {
    pub key_lo: u16,
    pub key_hi: u16,
    pub vel_lo: u16,
    pub vel_hi: u16,
    pub key_group: u16,
    pub wave: u32,
    pub wsmp: Option<Wsmp>,
    pub articulation: Vec<Connection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaveData {
    pub rate: u32,
    pub wsmp: Wsmp,
    pub samples: Vec<i16>,
}

impl Bank {
    /// Description of a parsed bank. Unreadable waves become empty 16-bit waves at 22050 Hz.
    pub fn read(d: &Dls) -> Bank {
        let instruments = d
            .instruments()
            .map(|i| Ins {
                drums: i.drums,
                bank_msb: i.bank_msb,
                bank_lsb: i.bank_lsb,
                program: i.program,
                name: i.name.map(|n| n.to_vec()).unwrap_or_default(),
                articulation: i.articulation().collect(),
                regions: i
                    .regions()
                    .map(|r| Rgn {
                        key_lo: r.key_lo,
                        key_hi: r.key_hi,
                        vel_lo: r.vel_lo,
                        vel_hi: r.vel_hi,
                        key_group: r.key_group,
                        wave: r.wave,
                        wsmp: r.wsmp,
                        articulation: r.articulation().collect(),
                    })
                    .collect(),
            })
            .collect();
        let waves = (0..d.wave_count())
            .map(|i| match d.wave(i) {
                Some(w) => WaveData {
                    rate: w.rate,
                    wsmp: w.wsmp,
                    samples: (0..w.len())
                        .map(|k| if w.bits == 8 { (w.data[k] as i16 - 128) << 8 } else { (w.sample(k) * 32768.0) as i16 })
                        .collect(),
                },
                None => WaveData { rate: 22050, wsmp: Wsmp::default(), samples: Vec::new() },
            })
            .collect();
        Bank { instruments, waves, pairs: d.pairs().collect() }
    }
}

struct W(Vec<u8>);

impl W {
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    /// `id` chunk (or `LIST`/`RIFF` with `kind`), size back-patched, odd sizes padded.
    fn chunk(&mut self, id: &[u8; 4], kind: Option<&[u8; 4]>, body: impl FnOnce(&mut W)) {
        self.0.extend_from_slice(id);
        let at = self.0.len();
        self.u32(0);
        if let Some(k) = kind {
            self.0.extend_from_slice(k);
        }
        body(self);
        let size = (self.0.len() - at - 4) as u32;
        self.0[at..at + 4].copy_from_slice(&size.to_le_bytes());
        if size & 1 == 1 {
            self.0.push(0);
        }
    }
    fn wsmp(&mut self, s: &Wsmp) {
        self.chunk(b"wsmp", None, |w| {
            w.u32(20);
            w.u16(s.unity_note);
            w.u16(s.fine_tune as u16);
            w.u32(s.gain as u32);
            w.u32(0);
            w.u32(s.looped.is_some() as u32);
            if let Some((start, len)) = s.looped {
                w.u32(16);
                w.u32(0);
                w.u32(start);
                w.u32(len);
            }
        });
    }
    fn lart(&mut self, c: &[Connection]) {
        if c.is_empty() {
            return;
        }
        self.chunk(b"LIST", Some(b"lart"), |w| {
            w.chunk(b"art1", None, |w| {
                w.u32(8);
                w.u32(c.len() as u32);
                for k in c {
                    w.u16(k.source);
                    w.u16(k.control);
                    w.u16(k.destination);
                    w.u16(k.transform);
                    w.u32(k.scale as u32);
                }
            })
        });
    }
}

/// Serialises `b` as a DLS level 1 bank (`colh`, `lins`, `ptbl`, `wvpl`).
pub fn write(b: &Bank) -> Vec<u8> {
    // Wave pool first, to know the cue offsets (relative to the `wvpl` list data).
    let mut pool = W(Vec::new());
    let mut cues = Vec::with_capacity(b.waves.len());
    for wv in &b.waves {
        cues.push(pool.0.len() as u32);
        pool.chunk(b"LIST", Some(b"wave"), |w| {
            w.chunk(b"fmt ", None, |w| {
                w.u16(1);
                w.u16(1);
                w.u32(wv.rate);
                w.u32(wv.rate * 2);
                w.u16(2);
                w.u16(16);
            });
            w.wsmp(&wv.wsmp);
            w.chunk(b"data", None, |w| {
                for s in &wv.samples {
                    w.0.extend_from_slice(&s.to_le_bytes());
                }
            });
        });
    }
    let mut out = W(Vec::with_capacity(pool.0.len() + 64 * 1024));
    out.chunk(b"RIFF", Some(b"DLS "), |w| {
        w.chunk(b"colh", None, |w| w.u32(b.instruments.len() as u32));
        w.chunk(b"LIST", Some(b"lins"), |w| {
            for i in &b.instruments {
                w.chunk(b"LIST", Some(b"ins "), |w| {
                    w.chunk(b"insh", None, |w| {
                        w.u32(i.regions.len() as u32);
                        w.u32((i.drums as u32) << 31 | (i.bank_msb as u32 & 0x7f) << 8 | (i.bank_lsb as u32 & 0x7f));
                        w.u32(i.program as u32 & 0x7f);
                    });
                    w.chunk(b"LIST", Some(b"lrgn"), |w| {
                        for r in &i.regions {
                            w.chunk(b"LIST", Some(b"rgn "), |w| {
                                w.chunk(b"rgnh", None, |w| {
                                    for v in [r.key_lo, r.key_hi, r.vel_lo, r.vel_hi, 0, r.key_group] {
                                        w.u16(v);
                                    }
                                });
                                if let Some(s) = &r.wsmp {
                                    w.wsmp(s);
                                }
                                w.chunk(b"wlnk", None, |w| {
                                    w.u16(0);
                                    w.u16(0);
                                    w.u32(1);
                                    w.u32(r.wave);
                                });
                                w.lart(&r.articulation);
                            });
                        }
                    });
                    w.lart(&i.articulation);
                    if !i.name.is_empty() {
                        w.chunk(b"LIST", Some(b"INFO"), |w| {
                            w.chunk(b"INAM", None, |w| {
                                w.0.extend_from_slice(&i.name);
                                w.0.push(0);
                            })
                        });
                    }
                });
            }
        });
        w.chunk(b"ptbl", None, |w| {
            w.u32(8);
            w.u32(cues.len() as u32);
            for c in &cues {
                w.u32(*c);
            }
        });
        w.chunk(b"LIST", Some(b"wvpl"), |w| w.0.extend_from_slice(&pool.0));
        if !b.pairs.is_empty() {
            w.chunk(b"LIST", Some(b"stpr"), |w| {
                w.chunk(b"pair", None, |w| {
                    for &(l, r) in &b.pairs {
                        w.u32(l);
                        w.u32(r);
                    }
                })
            });
        }
    });
    out.0
}
