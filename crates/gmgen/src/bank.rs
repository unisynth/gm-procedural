//! The generated bank: embedded structure skeleton + committed parameters → a full `RIFF 'DLS '` buffer.
use crate::math::to_i16;
use crate::params::Params;
use crate::render::{render, WaveMeta};
use alloc::vec::Vec;
use dls::write::Bank;
use dls::Dls;

/// A packed set: structure skeleton (DLS, empty waves), wave lengths (u32 LE) and encoded parameters, made by
/// `build.rs` from `assets/gm/` (and `assets/gm-plus/` with feature `plus`, change add-gmplus-bank).
pub struct Packed {
    pub skeleton: &'static [u8],
    pub lens: &'static [u8],
    pub params: &'static [u8],
}

/// The gm.dls set, whatever features are on (the fit tool relies on it under feature unification).
pub static GM: Packed = Packed {
    skeleton: include_bytes!(concat!(env!("OUT_DIR"), "/skeleton.dls")),
    lens: include_bytes!(concat!(env!("OUT_DIR"), "/lens.bin")),
    params: include_bytes!(concat!(env!("OUT_DIR"), "/params.bin")),
};

impl Packed {
    /// Number of waves with parameters slots (495 for the Windows layout).
    pub fn wave_count(&self) -> usize {
        self.lens.len() / 4
    }

    /// Bank structure with empty waves.
    pub fn skeleton(&self) -> Bank {
        Bank::read(&Dls::parse(self.skeleton).expect("embedded skeleton parses"))
    }

    /// Rate, length, unity note, tuning and loop of wave `i`.
    pub fn meta(&self, i: usize) -> Option<WaveMeta> {
        let d = Dls::parse(self.skeleton).ok()?;
        let w = d.wave(i as u32)?;
        let len = u32::from_le_bytes(self.lens.get(i * 4..i * 4 + 4)?.try_into().ok()?) as usize;
        Some(WaveMeta { rate: w.rate, len, unity: w.wsmp.unity_note, fine: w.wsmp.fine_tune, looped: w.wsmp.looped })
    }

    /// Committed parameters per wave (`None` = no parameter file).
    pub fn committed(&self) -> Vec<Option<Params>> {
        let mut rest = self.params;
        let mut out = Vec::with_capacity(self.wave_count());
        for _ in 0..self.wave_count() {
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

    /// Samples (f32) of wave `i` rendered from `p` (seed = wave index).
    pub fn render_wave(&self, i: usize, p: &Params) -> Vec<f32> {
        match self.meta(i) {
            Some(m) => render(&m, p, seed_of(&self.skeleton().pairs, i)),
            None => Vec::new(),
        }
    }

    /// Builds the bank. `only`: render just these wave indices; the others keep their length and loop but are
    /// silent. Waves without parameters are silent too.
    pub fn build(&self, only: Option<&[u32]>) -> Vec<u8> {
        let mut bank = self.skeleton();
        let params = self.committed();
        let pairs = bank.pairs.clone();
        for (i, w) in bank.waves.iter_mut().enumerate() {
            let m = self.meta(i).expect("wave meta");
            let wanted = only.is_none_or(|o| o.contains(&(i as u32)));
            w.samples = match (&params[i], wanted) {
                (Some(p), true) => render(&m, p, seed_of(&pairs, i)).iter().map(|x| to_i16(*x)).collect(),
                _ => alloc::vec![0; m.len],
            };
        }
        dls::write::write(&bank)
    }
}

/// Number of waves in the gm.dls set (495).
pub fn wave_count() -> usize {
    GM.wave_count()
}

/// gm.dls bank structure with empty waves.
pub fn skeleton() -> Bank {
    GM.skeleton()
}

/// Rate, length, unity note, tuning and loop of gm.dls-set wave `i`.
pub fn meta(i: usize) -> Option<WaveMeta> {
    GM.meta(i)
}

/// Committed gm.dls-set parameters per wave (`None` = no parameter file).
pub fn committed() -> Vec<Option<Params>> {
    GM.committed()
}

/// Samples (f32) of gm.dls-set wave `i` rendered from `p`. The seed is the wave index, so a wave sounds the same in
/// a full or partial build and in `gmfit`.
pub fn render_wave(i: usize, p: &Params) -> Vec<f32> {
    GM.render_wave(i, p)
}

/// Render seed of wave `i`: its index, or its left wave's for a stereo partner (change add-gmplus-bank D4), so a
/// partner sharing its left's parameters draws the same noise and phases and the pair stays coherent.
pub fn seed_of(pairs: &[(u32, u32)], i: usize) -> u32 {
    pairs.iter().find(|p| p.1 as usize == i).map_or(i as u32, |p| p.0)
}

/// Builds the gm.dls-set bank. `only`: render just these wave indices; the others keep their length and loop but
/// are silent. Waves without parameters are silent too.
pub fn build(only: Option<&[u32]>) -> Vec<u8> {
    GM.build(only)
}
