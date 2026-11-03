//! Procedural General MIDI bank: rebuilds the Windows `gm.dls` layout (instruments, regions, articulation, wave
//! indices, rates, lengths, loops) and synthesises every wave from committed model parameters (`assets/gm/`).
//! Output is a `RIFF 'DLS '` buffer for the existing `dls` reader. See `docs/synth/gm-procedural.md`.
//!
//! Determinism: only basic IEEE float ops and own approximations (`math`), so native and wasm32 builds produce the
//! same bytes.
#![cfg_attr(not(any(feature = "std", test)), no_std)]

extern crate alloc;

pub mod bank;
pub mod correct;
pub mod fam;
pub mod math;
pub mod params;
pub mod render;
pub mod schema;

#[cfg(feature = "std")]
pub mod pack;
#[cfg(feature = "std")]
pub mod params_json;
#[cfg(feature = "std")]
pub mod structure;

pub use bank::{build, render_wave, wave_count};

#[cfg(test)]
mod tests;
