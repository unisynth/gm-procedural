//! The gmplus set (feature `plus`, change add-gmplus-bank): the gm.dls skeleton at 32 kHz with parameters fitted
//! to the gmplus soundfont, packed by `build.rs` from `assets/gm-plus/`. Same API as [`crate::bank`].
use crate::bank::Packed;
use crate::params::Params;
use crate::render::WaveMeta;
use alloc::vec::Vec;
use dls::write::Bank;

pub static PLUS: Packed = Packed {
    skeleton: include_bytes!(concat!(env!("OUT_DIR"), "/plus_skeleton.dls")),
    lens: include_bytes!(concat!(env!("OUT_DIR"), "/plus_lens.bin")),
    params: include_bytes!(concat!(env!("OUT_DIR"), "/plus_params.bin")),
};

pub fn wave_count() -> usize {
    PLUS.wave_count()
}
pub fn skeleton() -> Bank {
    PLUS.skeleton()
}
pub fn meta(i: usize) -> Option<WaveMeta> {
    PLUS.meta(i)
}
pub fn committed() -> Vec<Option<Params>> {
    PLUS.committed()
}
pub fn render_wave(i: usize, p: &Params) -> Vec<f32> {
    PLUS.render_wave(i, p)
}
pub fn build(only: Option<&[u32]>) -> Vec<u8> {
    PLUS.build(only)
}
