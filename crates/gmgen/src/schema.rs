//! Fixed model-family schema: named scalar parameters (range + default) and tables (row width, max rows) per family.
//! Shared by the runtime, `build.rs` (through `pack.rs`) and `gmfit`. Adding a scalar with a default keeps old
//! parameter files valid; renaming or removing one does not.

pub struct Scalar {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

pub struct Table {
    pub name: &'static str,
    /// Numbers per row.
    pub width: usize,
    pub max_rows: usize,
}

pub struct Family {
    pub name: &'static str,
    pub scalars: &'static [Scalar],
    pub tables: &'static [Table],
    /// Can render the `sustain` region (exactly periodic over the loop).
    pub periodic: bool,
}

/// Most layers per wave.
pub const MAX_LAYERS: usize = 4;
/// Most numbers per wave (explicit scalars + table entries + correction entries + wave scalars).
pub const MAX_NUMBERS: usize = 256;
/// Most harmonic-correction entries.
pub const MAX_CORRECTION: usize = 128;

const fn s(name: &'static str, min: f32, max: f32, default: f32) -> Scalar {
    Scalar { name, min, max, default }
}

/// Wave-level scalars.
pub const WAVE: &[Scalar] = &[
    s("gain_db", -60.0, 24.0, 0.0),
    // MIDI pitch of the wave's fundamental; NaN-free default is resolved to the wave's unity note at render.
    s("pitch", -1.0, 140.0, -1.0),
];

/// Array of the shared envelope scalars (linear attack `a` s, hold `h` s, exponential decay `d` s toward sustain
/// level `s` 0–1) followed by the given family scalars.
macro_rules! with_env {
    ($($x:expr),* $(,)?) => {
        [s("a", 0.0, 2.0, 0.002), s("h", 0.0, 2.0, 0.0), s("d", 0.001, 30.0, 1.0), s("s", 0.0, 1.0, 1.0), $($x),*]
    };
}

static ADDITIVE: [Scalar; 18] = with_env![
        s("level_db", -80.0, 24.0, 0.0),
        s("cents", -1200.0, 1200.0, 0.0),
        s("tail", -48.0, 0.0, -96.0 / 8.0), // dB/octave beyond the table
        s("tilt", -24.0, 24.0, 0.0),        // dB/octave added at onset, relaxes to 0 with `tilt_t`
        s("tilt_t", 0.001, 10.0, 0.1),
        s("pdecay", 0.0, 10.0, 0.0), // extra decay rate per partial index (1/s per harmonic above 1)
        s("inharm", 0.0, 0.02, 0.0), // stiff-string B
        s("even_db", -48.0, 24.0, 0.0),
        s("unison", 1.0, 8.0, 1.0),
        s("detune", 0.0, 50.0, 0.0), // cents spread of the unison voices (outermost pair)
        s("vib_rate", 0.0, 12.0, 0.0),
        s("vib_cents", 0.0, 100.0, 0.0),
        s("scoop_cents", -200.0, 200.0, 0.0),
        s("scoop_t", 0.001, 1.0, 0.03),
];

static NOISE: [Scalar; 11] = with_env![
        s("level_db", -80.0, 24.0, 0.0),
        s("hp", 10.0, 20000.0, 10.0),
        s("lp", 20.0, 20000.0, 20000.0),
        s("sweep", -4.0, 4.0, 0.0), // octaves the bands move by at onset, relaxing with `sweep_t`
        s("sweep_t", 0.001, 5.0, 0.05),
        s("am_rate", 0.0, 40.0, 0.0),
        s("am_depth", 0.0, 1.0, 0.0),
];

static DRUM: [Scalar; 10] = [
    s("level_db", -80.0, 24.0, 0.0),
    s("freq", 20.0, 2000.0, 60.0), // end frequency of the body
    s("ratio", 1.0, 8.0, 2.0),     // start / end frequency
    s("sweep_t", 0.001, 1.0, 0.03),
    s("decay", 0.005, 10.0, 0.3),
    s("a", 0.0, 0.05, 0.0005),
    s("shape", 0.0, 1.0, 0.0), // 0 sine .. 1 triangle-ish (odd harmonics)
    s("drive", 0.0, 1.0, 0.0),
    s("click_db", -80.0, 12.0, -80.0),
    s("click_decay", 0.0005, 0.1, 0.004),
];

static METAL: [Scalar; 10] = [
    s("level_db", -80.0, 24.0, 0.0),
    s("base", 20.0, 4000.0, 205.3),
    s("bp", 100.0, 16000.0, 8000.0),
    s("q", 0.3, 20.0, 1.5),
    s("hp", 20.0, 16000.0, 6000.0),
    s("a", 0.0, 0.05, 0.0005),
    s("d", 0.002, 10.0, 0.05),
    s("d2", 0.002, 10.0, 0.05), // second decay stage after `split`
    s("split", 0.0, 1.0, 1.0),  // level where the second stage takes over (1 = single stage)
    s("noise", 0.0, 1.0, 0.0),  // mix of white noise into the oscillator bank
];

pub const FAMILIES: &[Family] = &[
    // Harmonic table `harm` (dB per harmonic 1..K), optional `phase` (cycles, harmonics 1..P).
    Family {
        name: "additive",
        scalars: &ADDITIVE,
        tables: &[Table { name: "harm", width: 1, max_rows: 240 }, Table { name: "phase", width: 1, max_rows: 32 }],
        periodic: true,
    },
    // `bands`: [centre Hz, Q, gain dB].
    Family { name: "noise", scalars: &NOISE, tables: &[Table { name: "bands", width: 3, max_rows: 8 }], periodic: true },
    // `modes`: extra membrane modes [ratio to body frequency, gain dB, decay s].
    Family { name: "drum", scalars: &DRUM, tables: &[Table { name: "modes", width: 3, max_rows: 3 }], periodic: false },
    // `ratios`: square oscillators at base × ratio (default TR-808 set when empty).
    Family { name: "metal", scalars: &METAL, tables: &[Table { name: "ratios", width: 1, max_rows: 6 }], periodic: false },
];

pub fn family(name: &str) -> Option<usize> {
    FAMILIES.iter().position(|f| f.name == name)
}

pub fn scalar(f: &Family, name: &str) -> Option<usize> {
    f.scalars.iter().position(|s| s.name == name)
}

/// Layer render regions (design D3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Region {
    /// Faded to silence at the loop start.
    Attack = 0,
    /// Exactly periodic over the loop from the loop start on.
    Sustain = 1,
    /// Whole wave, no loop handling (unlooped waves).
    Whole = 2,
}

impl Region {
    pub fn from_name(n: &str) -> Option<Region> {
        match n {
            "attack" => Some(Region::Attack),
            "sustain" => Some(Region::Sustain),
            "whole" => Some(Region::Whole),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        ["attack", "sustain", "whole"][self as usize]
    }
    pub fn from_u8(v: u8) -> Option<Region> {
        [Region::Attack, Region::Sustain, Region::Whole].get(v as usize).copied()
    }
}

/// Time × band correction grid (`tf`, design D4c): band count and row times in seconds.
pub const TF_BANDS: usize = 20;
/// Ratio between neighbouring `tf` band centres (55 Hz · ratio^b, up to ~10 kHz).
pub const TF_RATIO: f32 = 1.315;
pub const TF_TIMES: [f32; 9] = [0.0, 0.005, 0.01, 0.02, 0.04, 0.08, 0.16, 0.32, 0.64];
