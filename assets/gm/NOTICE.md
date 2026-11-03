# Procedural GM bank data: provenance

This directory feeds the procedural General MIDI bank (`crates/gmgen`, change `add-procedural-gm-bank`).

- `structure.json` is the *structure* of Microsoft/Roland's Windows `gm.dls`, written by `gmfit extract`: instrument
  names, bank/program numbers, key and velocity regions, DLS articulation values, and per-wave rate, length, unity
  note, tuning and loop points. It contains no audio.
- `waves/NNN.json` and `scores.json` hold bounded synthesis-model parameters (at most 4 layers and 256 numbers per
  wave) fitted by `gmfit` so that the generated waves sound like the original ones. No sample data is stored. The
  model families are fixed in code, and a parameter set cannot carry a waveform, a residual or a wavetable.

The original `gm.dls` (© Microsoft / Roland) is never committed or shipped. The fitting tool reads a local copy from
`GM_DLS`. On Windows, production builds play the OS file itself. Other platforms and the DAW play the bank generated
from this directory.
