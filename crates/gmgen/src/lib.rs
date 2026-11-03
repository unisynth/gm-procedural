//! Procedural General MIDI bank: rebuilds the Windows `gm.dls` layout (instruments, regions, articulation, wave
//! indices, rates, lengths, loops) and synthesises every wave from committed model parameters (`assets/gm/`).
//! Output is a `RIFF 'DLS '` buffer for the existing `dls` reader. See `docs/synth/gm-procedural.md`.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

#[cfg(feature = "std")]
pub mod structure;
