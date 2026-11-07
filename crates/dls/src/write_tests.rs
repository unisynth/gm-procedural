use super::write::*;
use super::*;
use std::vec;
use std::vec::Vec;

fn conn(s: u16, d: u16, scale: i32) -> Connection {
    Connection { source: s, control: 0, destination: d, transform: 0, scale }
}

/// Melodic instrument with 2 regions (one with own wsmp + articulation, odd-length name), a drum kit without
/// articulation, a looped and a one-shot wave.
fn desc() -> Bank {
    let ws = |u, f, g, l| Wsmp { unity_note: u, fine_tune: f, gain: g, looped: l };
    Bank {
        instruments: vec![
            Ins {
                drums: false,
                bank_msb: 8,
                bank_lsb: 1,
                program: 5,
                name: b"Piano".to_vec(),
                articulation: vec![conn(0, 0x206, -65536), conn(1, 0x20a, 12)],
                regions: vec![
                    Rgn { key_lo: 0, key_hi: 59, vel_lo: 0, vel_hi: 127, key_group: 0, wave: 1, wsmp: None, articulation: vec![] },
                    Rgn {
                        key_lo: 60,
                        key_hi: 127,
                        vel_lo: 10,
                        vel_hi: 100,
                        key_group: 3,
                        wave: 0,
                        wsmp: Some(ws(72, -7, -655360, Some((2, 3)))),
                        articulation: vec![conn(2, 3, 4)],
                    },
                ],
            },
            Ins {
                drums: true,
                bank_msb: 0,
                bank_lsb: 0,
                program: 0,
                name: b"Standard".to_vec(),
                articulation: vec![],
                regions: vec![Rgn {
                    key_lo: 36,
                    key_hi: 36,
                    vel_lo: 0,
                    vel_hi: 127,
                    key_group: 1,
                    wave: 1,
                    wsmp: Some(ws(36, 0, 0, None)),
                    articulation: vec![],
                }],
            },
        ],
        waves: vec![
            WaveData { rate: 22050, wsmp: ws(60, 3, 0, Some((1, 4))), samples: vec![0, 1000, -1000, 32767, -32768, 5] },
            WaveData { rate: 24000, wsmp: ws(36, 0, 0, None), samples: vec![7, -7, 0] },
        ],
        pairs: vec![],
    }
}

#[test]
fn synthetic_round_trip() {
    let d = desc();
    let bytes = write(&d);
    let p = Dls::parse(&bytes).unwrap();
    assert_eq!((p.instrument_count(), p.wave_count()), (2, 2));
    let piano = p.find(false, 8, 5).unwrap();
    assert_eq!((piano.bank_lsb, piano.name), (1, Some(&b"Piano"[..])));
    let r = piano.region_for(70, 50).unwrap();
    assert_eq!((r.key_group, r.wave, r.wsmp.unwrap().looped), (3, 0, Some((2, 3))));
    assert_eq!(p.wave(0).unwrap().sample(3), 32767.0 / 32768.0);
    assert_eq!(p.wave(1).unwrap().rate, 24000);
    assert_eq!(Bank::read(&p), d);
}

#[test]
fn empty_articulation_writes_no_lart() {
    let bytes = write(&desc());
    let p = Dls::parse(&bytes).unwrap();
    assert_eq!(p.find(true, 0, 0).unwrap().articulation().count(), 0);
    assert_eq!(p.find(false, 8, 5).unwrap().regions().next().unwrap().articulation().count(), 0);
}

/// Windows `gm.dls` via `GM_DLS` (never committed): read → write → parse is lossless.
#[test]
fn gm_dls_round_trip() {
    let Some(path) = std::env::var_os("GM_DLS") else {
        eprintln!("skip: GM_DLS not set");
        return;
    };
    let b = std::fs::read(path).unwrap();
    let orig = Bank::read(&Dls::parse(&b).unwrap());
    assert_eq!((orig.instruments.len(), orig.waves.len()), (235, 495));
    let again = write(&orig);
    let p = Dls::parse(&again).unwrap();
    assert_eq!(Bank::read(&p), orig);
    let fp = |d: &Dls| -> Vec<String> {
        let mut v: Vec<String> = d
            .instruments()
            .map(|i| {
                let w: Vec<String> = i.regions().map(|r| format!("{}-{}:{}", r.key_lo, r.key_hi, r.wave)).collect();
                format!("{} {} {} {:?} {} {} {}", i.drums, i.bank_msb, i.program, i.name, i.regions().count(), i.articulation().count(), w.join(" "))
            })
            .collect();
        v.extend((0..d.wave_count()).map(|i| {
            let w = d.wave(i).unwrap();
            format!("{} {} {} {:?}", w.rate, w.len(), w.wsmp.unity_note, w.wsmp.looped)
        }));
        v
    };
    assert_eq!(fp(&p), fp(&Dls::parse(&b).unwrap()));
}

/// Stereo pairs (change add-gmplus-bank D5): a right wave appended to the pool, never referenced by a region.
fn paired() -> Bank {
    let mut b = desc();
    let r = WaveData { samples: vec![1, 2, 3, 4, 5, 6], ..b.waves[0].clone() };
    b.waves.push(r);
    b.pairs = vec![(0, 2)];
    b
}

/// Spec "Stereo wave pairs": the writer emits the pairs, the reader reports the partner, and read → write → read
/// keeps them.
#[test]
fn pairs_round_trip() {
    let b = paired();
    let bytes = write(&b);
    let d = Dls::parse(&bytes).unwrap();
    assert_eq!(d.partner(0), Some(2));
    assert_eq!(d.partner(1), None);
    assert_eq!(d.partner(2), None);
    assert_eq!(Bank::read(&d), b);
}

/// Spec: a pair whose waves differ in rate, length or loop, or that names a missing wave, is ignored; the bank
/// still parses.
#[test]
fn bad_pairs_are_ignored() {
    let mut b = paired();
    b.waves[2].wsmp.looped = Some((0, 4)); // loop differs from wave 0's (1, 4)
    b.pairs = vec![(0, 2), (1, 9)];
    let bytes = write(&b);
    let d = Dls::parse(&bytes).unwrap();
    assert_eq!((d.partner(0), d.partner(1)), (None, None));
    let mut b = paired();
    b.waves[2].samples.pop(); // length differs
    assert_eq!(Dls::parse(&write(&b)).unwrap().partner(0), None);
}

/// A bank without pairs has no `stpr` list: existing output stays byte-identical.
#[test]
fn no_pairs_no_chunk() {
    let bytes = write(&desc());
    assert!(!bytes.windows(4).any(|w| w == b"stpr"));
}
