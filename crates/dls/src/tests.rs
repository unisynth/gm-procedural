use super::*;
use std::vec::Vec;

// ---- tiny RIFF builder (test only) ----
fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend_from_slice(&(data.len() as u32).to_le_bytes());
    v.extend_from_slice(data);
    if data.len() & 1 == 1 {
        v.push(0);
    }
    v
}
fn list(kind: &[u8; 4], parts: &[Vec<u8>]) -> Vec<u8> {
    let mut body = kind.to_vec();
    for p in parts {
        body.extend_from_slice(p);
    }
    chunk(b"LIST", &body)
}
fn le(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}
fn u16b(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}
fn u32b(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}
fn wsmp(unity: u16, fine: i16, gain: i32, lp: Option<(u32, u32)>) -> Vec<u8> {
    let n = lp.is_some() as u32;
    let mut d = le(&[&u32b(20), &u16b(unity), &fine.to_le_bytes(), &gain.to_le_bytes(), &u32b(0), &u32b(n)]);
    if let Some((s, l)) = lp {
        d.extend(le(&[&u32b(16), &u32b(0), &u32b(s), &u32b(l)]));
    }
    chunk(b"wsmp", &d)
}
fn art1(blocks: &[(u16, u16, u16, u16, i32)]) -> Vec<u8> {
    let mut d = le(&[&u32b(8), &u32b(blocks.len() as u32)]);
    for &(s, c, dst, t, sc) in blocks {
        d.extend(le(&[&u16b(s), &u16b(c), &u16b(dst), &u16b(t), &sc.to_le_bytes()]));
    }
    list(b"lart", &[chunk(b"art1", &d)])
}
fn fmt(bits: u16, rate: u32) -> Vec<u8> {
    let ba = bits / 8;
    chunk(b"fmt ", &le(&[&u16b(1), &u16b(1), &u32b(rate), &u32b(rate * ba as u32), &u16b(ba), &u16b(bits)]))
}

/// Two instruments (melodic prog 5 with 2 regions; drum kit), two waves (8-bit, 16-bit).
fn bank() -> Vec<u8> {
    let rgn = |lo: u16, hi: u16, wave: u32, extra: Vec<Vec<u8>>| {
        let mut parts = vec![
            chunk(b"rgnh", &le(&[&u16b(lo), &u16b(hi), &u16b(0), &u16b(127), &u16b(0), &u16b(0)])),
            chunk(b"wlnk", &le(&[&u16b(0), &u16b(0), &u32b(1), &u32b(wave)])),
        ];
        parts.extend(extra);
        list(b"rgn ", &parts)
    };
    let ins0 = list(
        b"ins ",
        &[
            chunk(b"insh", &le(&[&u32b(2), &u32b(0x0000_0100), &u32b(5)])),
            list(
                b"lrgn",
                &[
                    rgn(0, 59, 0, vec![]),
                    rgn(60, 127, 1, vec![wsmp(72, -3, 0, Some((2, 4))), art1(&[(0, 0, 0x020a, 0, -100)])]),
                ],
            ),
            art1(&[(0, 0, 0x0206, 0, 1234), (0, 0, 0x0209, 0, 5678)]),
            list(b"INFO", &[chunk(b"INAM", b"Piano\0")]),
        ],
    );
    let ins1 = list(
        b"ins ",
        &[
            chunk(b"insh", &le(&[&u32b(1), &u32b(0x8000_0000), &u32b(0)])),
            list(b"lrgn", &[rgn(36, 36, 1, vec![])]),
        ],
    );
    let w0 = list(b"wave", &[fmt(8, 22050), wsmp(60, 0, 0, None), chunk(b"data", &[0x80, 0xFF, 0x00])]);
    let w1 = list(
        b"wave",
        &[fmt(16, 44100), wsmp(64, 7, -65536, Some((1, 2))), chunk(b"data", &le(&[&0i16.to_le_bytes(), &i16::MIN.to_le_bytes(), &16384i16.to_le_bytes()]))],
    );
    let w0_len = w0.len() as u32;
    let ptbl = chunk(b"ptbl", &le(&[&u32b(8), &u32b(2), &u32b(0), &u32b(w0_len)]));
    let body = le(&[
        b"DLS ",
        &chunk(b"colh", &u32b(2)),
        &list(b"lins", &[ins0, ins1]),
        &ptbl,
        &list(b"wvpl", &[w0, w1]),
    ]);
    chunk(b"RIFF", &body)
}

#[test]
fn rejects_non_dls() {
    assert_eq!(Dls::parse(b"").err(), Some(Error::NotDls));
    assert_eq!(Dls::parse(&chunk(b"RIFF", b"WAVE")).err(), Some(Error::NotDls));
    let mut b = bank();
    b.truncate(b.len() - 10); // RIFF size now overruns the buffer
    assert_eq!(Dls::parse(&b).err(), Some(Error::Truncated));
}

#[test]
fn missing_sections_are_errors() {
    let body = le(&[b"DLS ", &chunk(b"colh", &u32b(0))]);
    assert_eq!(Dls::parse(&chunk(b"RIFF", &body)).err(), Some(Error::Missing(*b"lins")));
}

#[test]
fn instruments_and_locale() {
    let b = bank();
    let d = Dls::parse(&b).unwrap();
    assert_eq!(d.instrument_count(), 2);
    let ins: Vec<_> = d.instruments().collect();
    assert_eq!(ins.len(), 2);
    assert_eq!((ins[0].program, ins[0].bank_msb, ins[0].bank_lsb, ins[0].drums), (5, 1, 0, false));
    assert_eq!(ins[0].name, Some(&b"Piano"[..]));
    assert!(ins[1].drums);
    assert_eq!(ins[1].name, None);
    assert_eq!(d.find(false, 1, 5).map(|i| i.program), Some(5));
    assert_eq!(d.find(true, 0, 0).map(|i| i.drums), Some(true));
    assert!(d.find(false, 1, 6).is_none() && d.find(false, 0, 5).is_none());
}

#[test]
fn regions_and_wsmp_fallback() {
    let b = bank();
    let d = Dls::parse(&b).unwrap();
    let ins = d.instruments().next().unwrap();
    let r: Vec<_> = ins.regions().collect();
    assert_eq!(r.len(), 2);
    assert_eq!((r[0].key_lo, r[0].key_hi, r[0].vel_hi, r[0].wave), (0, 59, 127, 0));
    // region 0 has no wsmp -> falls back to the wave's
    let (_, s0) = d.sampler(&r[0]).unwrap();
    assert_eq!((s0.unity_note, s0.fine_tune, s0.looped), (60, 0, None));
    // region 1 overrides
    let (_, s1) = d.sampler(&r[1]).unwrap();
    assert_eq!((s1.unity_note, s1.fine_tune, s1.looped), (72, -3, Some((2, 4))));
    assert_eq!(ins.region_for(30, 100).map(|r| r.wave), Some(0));
    assert_eq!(ins.region_for(60, 100).map(|r| r.wave), Some(1));
}

#[test]
fn articulation_blocks() {
    let b = bank();
    let d = Dls::parse(&b).unwrap();
    let ins = d.instruments().next().unwrap();
    let a: Vec<_> = ins.articulation().collect();
    assert_eq!(a.len(), 2);
    assert_eq!((a[0].destination, a[0].scale), (0x0206, 1234));
    assert_eq!((a[1].destination, a[1].scale), (0x0209, 5678));
    let r1 = ins.regions().nth(1).unwrap();
    let ra: Vec<_> = r1.articulation().collect();
    assert_eq!((ra[0].destination, ra[0].scale), (0x020a, -100));
    assert_eq!(ins.regions().next().unwrap().articulation().count(), 0);
}

#[test]
fn waves_decode() {
    let b = bank();
    let d = Dls::parse(&b).unwrap();
    assert_eq!(d.wave_count(), 2);
    let w0 = d.wave(0).unwrap();
    assert_eq!((w0.rate, w0.bits, w0.len()), (22050, 8, 3));
    assert_eq!(w0.sample(0), 0.0);
    assert_eq!(w0.sample(1), 127.0 / 128.0);
    assert_eq!(w0.sample(2), -1.0);
    let w1 = d.wave(1).unwrap();
    assert_eq!((w1.rate, w1.bits, w1.len()), (44100, 16, 3));
    assert_eq!((w1.sample(1), w1.sample(2)), (-1.0, 0.5));
    assert_eq!((w1.wsmp.unity_note, w1.wsmp.gain, w1.wsmp.looped), (64, -65536, Some((1, 2))));
    assert!(d.wave(2).is_none());
}

#[test]
fn bad_ptbl_offset_is_none_not_panic() {
    let mut b = bank();
    // corrupt the second ptbl cue to point past the pool
    let p = b.windows(4).position(|w| w == b"ptbl").unwrap();
    b[p + 20..p + 24].copy_from_slice(&u32b(0xFFFF));
    let d = Dls::parse(&b).unwrap();
    assert!(d.wave(1).is_none());
    assert!(d.wave(0).is_some());
}

/// Real-world bank shipped with every macOS (Roland GS / QuickTime Music Synthesizer).
#[test]
fn macos_gs_instruments() {
    let path = "/System/Library/Components/CoreAudio.component/Contents/Resources/gs_instruments.dls";
    let Ok(b) = std::fs::read(path) else {
        eprintln!("skip: {path} not present");
        return;
    };
    let d = Dls::parse(&b).unwrap();
    assert_eq!(d.instrument_count(), 235);
    assert_eq!(d.instruments().count(), 235);
    assert_eq!(d.wave_count(), 495);
    for i in 0..d.wave_count() {
        let w = d.wave(i).unwrap();
        assert!(w.len() > 0 && (w.bits == 8 || w.bits == 16));
    }
    // GM program 0 (Acoustic Grand) and the standard drum kit must exist and resolve.
    for ins in [d.find(false, 0, 0).unwrap(), d.find(true, 0, 0).unwrap()] {
        assert!(ins.regions().count() > 0);
        for r in ins.regions() {
            assert!(d.sampler(&r).is_some(), "region wave {} unresolved", r.wave);
        }
    }
    assert!(d.find(true, 0, 0).unwrap().region_for(36, 100).is_some()); // kick
}

/// Windows `gm.dls` (never committed — Roland/Microsoft license). Point `GM_DLS` at a local copy.
#[test]
fn windows_gm_dls() {
    let Some(path) = std::env::var_os("GM_DLS") else {
        eprintln!("skip: GM_DLS not set");
        return;
    };
    let b = std::fs::read(path).unwrap();
    let d = Dls::parse(&b).unwrap();
    assert_eq!((d.instrument_count(), d.wave_count()), (235, 495));
    for i in 0..d.wave_count() {
        let w = d.wave(i).unwrap();
        assert_eq!(w.bits, 16);
    }
    for ins in d.instruments() {
        for r in ins.regions() {
            assert!(d.sampler(&r).is_some());
        }
    }
}
