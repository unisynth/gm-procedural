//! Analysis priors (design D4, `research/gm-procedural-instruments.md`): for each GM program and drum key, the
//! preferred model families (best first) and physical starting values. `analyze` takes the first family that is
//! implemented in `gmgen::schema::FAMILIES`, so the table can name families before they exist.

/// Mode-ratio sets for struck objects.
pub const MARIMBA: &[f32] = &[1.0, 3.99, 10.65];
pub const VIBRAPHONE: &[f32] = &[1.0, 2.01, 3.9, 14.37];
pub const XYLOPHONE: &[f32] = &[1.0, 3.0, 9.0];
pub const FREE_BAR: &[f32] = &[1.0, 2.756, 5.404, 8.933];
pub const TUBULAR_BELL: &[f32] = &[1.0, 2.76, 5.40, 8.93, 13.34, 18.64];
pub const RISSET_BELL: &[f32] = &[0.56, 0.563, 0.92, 0.923, 1.19, 1.70, 2.00, 2.74, 3.00, 3.74, 4.07];
pub const MEMBRANE: &[f32] = &[1.0, 1.593, 2.136, 2.296, 2.653, 2.917];
pub const TIMPANI: &[f32] = &[1.0, 1.5, 1.98, 2.44];
pub const KALIMBA: &[f32] = &[1.0, 6.3];
pub const STEEL_PAN: &[f32] = &[1.0, 2.0, 3.0, 4.0];
pub const AGOGO: &[f32] = &[1.0, 4.08, 6.669];
pub const WOODBLOCK: &[f32] = &[1.0, 2.573, 4.888];
/// TR-808 metallic oscillators (Hz) and cowbell pair.
pub const HAT_808: &[f32] = &[205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
pub const COWBELL_808: &[f32] = &[540.0, 800.0];
/// Hammond footages as harmonics of the 16′ sub: 16′, 5⅓′, 8′, 4′, 2⅔′, 2′, 1⅗′, 1⅓′, 1′.
pub const HAMMOND: &[f32] = &[1.0, 3.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 16.0];
/// Csound SATB vowel formants F1–F5 (Hz): /a/ and /u/.
pub const VOWEL_A: [[f32; 5]; 4] =
    [[800., 1150., 2900., 3900., 4950.], [800., 1150., 2800., 3500., 4950.], [650., 1080., 2650., 2900., 3250.], [600., 1040., 2250., 2450., 2750.]];
pub const VOWEL_U: [[f32; 5]; 4] =
    [[325., 700., 2700., 3800., 4950.], [325., 700., 2530., 3500., 4950.], [350., 600., 2700., 2900., 3300.], [350., 600., 2400., 2675., 2950.]];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// Steady harmonic tone (organ, pad, lead, brass, reed, pipe, strings).
    Harmonic,
    /// Plucked string.
    Plucked,
    /// Struck object with mode ratios.
    Struck(&'static [f32]),
    /// Voice / choir with a formant set.
    Voice(&'static [[f32; 5]; 4]),
    /// Drum body (kick, tom, timpani-like) with pitch glide.
    Drum,
    /// Snare: drum body + noise wires.
    Snare,
    /// Metallic noise (hats, cymbals, cowbell) with oscillator frequencies.
    Metal(&'static [f32]),
    /// Particle noise (shakers, applause).
    Shaker,
    /// Plain filtered noise (breath, seashore, effects).
    Noise,
}

#[derive(Clone, Copy, Debug)]
pub struct Prior {
    pub kind: Kind,
    /// Families, best first.
    pub families: &'static [&'static str],
    pub inharm: f32,
    pub vib_rate: f32,
    pub vib_cents: f32,
    pub scoop_cents: f32,
    /// Suppress even harmonics (clarinet).
    pub odd: bool,
}

const fn p(kind: Kind, families: &'static [&'static str]) -> Prior {
    Prior { kind, families, inharm: 0.0, vib_rate: 0.0, vib_cents: 0.0, scoop_cents: 0.0, odd: false }
}

const HARM: &[&str] = &["additive"];
const PLUCK: &[&str] = &["ks", "additive"];
const STRUCK: &[&str] = &["modal", "additive"];
const FM: &[&str] = &["fm", "additive"];
const BOWED: &[&str] = &["formant", "additive", "noise"];
const BLOWN: &[&str] = &["additive", "noise"];
const VOICE: &[&str] = &["formant", "additive"];
const SYNTH: &[&str] = &["subtractive", "additive"];
const DRUM: &[&str] = &["drum", "noise"];
const SNARE: &[&str] = &["drum", "noise"];
const METAL: &[&str] = &["metal", "noise"];
const SHAKE: &[&str] = &["shaker", "noise"];
const NOISE: &[&str] = &["noise"];

/// Prior for a melodic GM program (0–127).
pub fn for_program(prog: u8) -> Prior {
    let mut x = match prog {
        0..=3 => Prior { inharm: 4e-4, ..p(Kind::Struck(&[1.0]), &["additive", "modal"]) },
        4 => p(Kind::Struck(&[1.0, 2.0]), &["modal", "fm", "additive"]),
        5 => p(Kind::Harmonic, FM),
        6 | 7 => p(Kind::Plucked, PLUCK),
        8 | 9 => p(Kind::Struck(FREE_BAR), STRUCK),
        10 => p(Kind::Struck(&[1.0, 6.27]), STRUCK),
        11 => Prior { vib_rate: 5.5, ..p(Kind::Struck(VIBRAPHONE), STRUCK) },
        12 => p(Kind::Struck(MARIMBA), STRUCK),
        13 => p(Kind::Struck(XYLOPHONE), STRUCK),
        14 => p(Kind::Struck(TUBULAR_BELL), &["modal", "fm", "additive"]),
        15 => Prior { inharm: 2e-4, ..p(Kind::Plucked, PLUCK) },
        16..=20 => p(Kind::Harmonic, HARM),
        21 | 23 => Prior { vib_rate: 5.0, vib_cents: 5.0, ..p(Kind::Harmonic, HARM) },
        22 => p(Kind::Harmonic, BLOWN),
        24..=31 => p(Kind::Plucked, PLUCK),
        32..=37 => p(Kind::Plucked, PLUCK),
        38 | 39 => p(Kind::Harmonic, SYNTH),
        40..=44 | 48 | 49 => Prior { vib_rate: 5.5, vib_cents: 15.0, ..p(Kind::Harmonic, BOWED) },
        45 | 46 => p(Kind::Plucked, PLUCK),
        47 => p(Kind::Struck(TIMPANI), &["drum", "modal"]),
        50 | 51 => p(Kind::Harmonic, SYNTH),
        52 => Prior { vib_rate: 5.0, vib_cents: 20.0, ..p(Kind::Voice(&VOWEL_A), VOICE) },
        53 => Prior { vib_rate: 5.0, vib_cents: 15.0, ..p(Kind::Voice(&VOWEL_U), VOICE) },
        54 => p(Kind::Voice(&VOWEL_A), VOICE),
        55 => p(Kind::Struck(&[1.0, 1.25, 1.5, 2.0]), &["modal", "additive", "noise"]),
        56..=63 => Prior { scoop_cents: -30.0, ..p(Kind::Harmonic, if prog >= 62 { SYNTH } else { HARM }) },
        64..=70 => Prior { vib_rate: 5.0, vib_cents: 10.0, ..p(Kind::Harmonic, BLOWN) },
        71 => Prior { odd: true, ..p(Kind::Harmonic, BLOWN) },
        72..=79 => Prior { vib_rate: 5.0, vib_cents: 8.0, ..p(Kind::Harmonic, BLOWN) },
        80..=95 => p(Kind::Harmonic, SYNTH),
        96..=103 => p(Kind::Harmonic, &["subtractive", "fm", "additive", "noise"]),
        104..=107 => Prior { inharm: 1e-4, ..p(Kind::Plucked, PLUCK) },
        108 => p(Kind::Struck(KALIMBA), STRUCK),
        109 => p(Kind::Harmonic, HARM),
        110 => Prior { vib_rate: 5.5, vib_cents: 15.0, ..p(Kind::Harmonic, BOWED) },
        111 => p(Kind::Harmonic, BLOWN),
        112 => p(Kind::Struck(RISSET_BELL), STRUCK),
        113 => p(Kind::Struck(AGOGO), STRUCK),
        114 => p(Kind::Struck(STEEL_PAN), STRUCK),
        115 => p(Kind::Struck(WOODBLOCK), STRUCK),
        116 | 117 | 118 => p(Kind::Drum, DRUM),
        119 => p(Kind::Metal(HAT_808), METAL),
        120 | 121 => p(Kind::Noise, NOISE),
        122 => p(Kind::Noise, NOISE),
        123 => p(Kind::Harmonic, FM),
        124 => p(Kind::Struck(RISSET_BELL), STRUCK),
        125 => p(Kind::Noise, NOISE),
        126 => p(Kind::Shaker, SHAKE),
        _ => p(Kind::Noise, NOISE),
    };
    if !matches!(x.kind, Kind::Struck(_)) && prog <= 7 {
        x.inharm = 0.0;
    }
    x
}

/// Prior for a GM drum-kit key (General MIDI percussion map 27–87; others → noise).
pub fn for_drum_key(key: u8) -> Prior {
    match key {
        35 | 36 => p(Kind::Drum, DRUM),
        38 | 40 => p(Kind::Snare, SNARE),
        37 | 39 => p(Kind::Noise, NOISE),
        41 | 43 | 45 | 47 | 48 | 50 => p(Kind::Drum, DRUM),
        42 | 44 | 46 => p(Kind::Metal(HAT_808), METAL),
        49 | 51 | 52 | 53 | 55 | 57 | 59 => p(Kind::Metal(HAT_808), METAL),
        56 => p(Kind::Metal(COWBELL_808), METAL),
        54 | 69 | 70 | 82 => p(Kind::Shaker, SHAKE),
        58 => p(Kind::Noise, NOISE),
        60..=66 => p(Kind::Struck(MEMBRANE), &["drum", "modal", "noise"]),
        67 | 68 => p(Kind::Struck(AGOGO), STRUCK),
        71 | 72 => p(Kind::Harmonic, BLOWN),
        73 | 74 => p(Kind::Noise, NOISE),
        75 | 76 | 77 => p(Kind::Struck(WOODBLOCK), STRUCK),
        78 | 79 => p(Kind::Harmonic, &["additive", "noise"]),
        80 | 81 => p(Kind::Struck(FREE_BAR), STRUCK),
        _ => p(Kind::Noise, NOISE),
    }
}

/// First implemented family of a prior.
pub fn family(prior: &Prior) -> &'static str {
    prior.families.iter().copied().find(|f| gmgen::schema::family(f).is_some()).unwrap_or("noise")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_program_and_drum_key_maps_to_an_implemented_family() {
        for prog in 0..=127u8 {
            let f = family(&for_program(prog));
            assert!(gmgen::schema::family(f).is_some(), "program {prog} → {f}");
        }
        // Every key used by the 9 kits of the bank.
        let bank = crate::skeleton();
        for ins in bank.instruments.iter().filter(|i| i.drums) {
            for r in &ins.regions {
                for key in r.key_lo..=r.key_hi.min(127) {
                    let f = family(&for_drum_key(key as u8));
                    assert!(gmgen::schema::family(f).is_some(), "kit key {key} → {f}");
                }
            }
        }
    }

    #[test]
    fn known_priors() {
        assert_eq!(for_drum_key(42).kind, Kind::Metal(HAT_808));
        assert_eq!(for_program(12).kind, Kind::Struck(MARIMBA));
        assert!(for_program(71).odd);
    }
}
