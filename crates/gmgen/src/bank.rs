//! The generated bank: embedded structure skeleton + committed parameters → a full `RIFF 'DLS '` buffer.
use crate::math::to_i16;
use crate::params::Params;
use crate::render::{render, WaveMeta};
use alloc::vec::Vec;
use dls::write::Bank;
use dls::Dls;

static SKELETON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/skeleton.dls"));
static LENS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lens.bin"));
static PARAMS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/params.bin"));

/// Number of waves in the bank (495 for the Windows layout).
pub fn wave_count() -> usize {
    LENS.len() / 4
}

/// Bank structure with empty waves.
pub fn skeleton() -> Bank {
    Bank::read(&Dls::parse(SKELETON).expect("embedded skeleton parses"))
}

/// Rate, length, unity note, tuning and loop of wave `i`.
pub fn meta(i: usize) -> Option<WaveMeta> {
    let d = Dls::parse(SKELETON).ok()?;
    let w = d.wave(i as u32)?;
    let len = u32::from_le_bytes(LENS.get(i * 4..i * 4 + 4)?.try_into().ok()?) as usize;
    Some(WaveMeta { rate: w.rate, len, unity: w.wsmp.unity_note, fine: w.wsmp.fine_tune, looped: w.wsmp.looped })
}

/// Committed parameters per wave (`None` = no parameter file).
pub fn committed() -> Vec<Option<Params>> {
    let mut rest = PARAMS;
    let mut out = Vec::with_capacity(wave_count());
    for _ in 0..wave_count() {
        let Some((&flag, r)) = rest.split_first() else { break };
        if flag == 0 {
            out.push(None);
            rest = r;
        } else {
            let (p, r) = Params::decode(r).expect("embedded params decode");
            out.push(Some(p));
            rest = r;
        }
    }
    out
}

/// Samples (f32) of wave `i` rendered from `p`. The seed is the wave index, so a wave sounds the same in a
/// full or partial build and in `gmfit`.
pub fn render_wave(i: usize, p: &Params) -> Vec<f32> {
    match meta(i) {
        Some(m) => render(&m, p, i as u32),
        None => Vec::new(),
    }
}

/// Builds the bank. `only`: render just these wave indices; the others keep their length and loop but are
/// silent. Waves without parameters are silent too.
pub fn build(only: Option<&[u32]>) -> Vec<u8> {
    let mut bank = skeleton();
    let params = committed();
    for (i, w) in bank.waves.iter_mut().enumerate() {
        let m = meta(i).expect("wave meta");
        let wanted = only.is_none_or(|o| o.contains(&(i as u32)));
        w.samples = match (&params[i], wanted) {
            (Some(p), true) => render(&m, p, i as u32).iter().map(|x| to_i16(*x)).collect(),
            _ => alloc::vec![0; m.len],
        };
    }
    dls::write::write(&bank)
}
