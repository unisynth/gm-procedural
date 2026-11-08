# docs/synth

Synth architecture docs (Mermaid diagrams). Source of truth = crates; update the doc when a crate's behaviour changes.

| `gm-procedural.md` | Procedural GM bank (change add-procedural-gm-bank): data flow diagram (GM_DLS → gmfit extract/fit → assets/gm → gmgen build.rs → `build(only)` → DLS consumers), `structure.json` + `waves/NNN.json` formats, budget (4 layers / 512 numbers) + build errors, regions (sustain exact integer phasors + frozen envelopes, attack 10 ms fade, whole), correction (loop DFT, attack 129-tap FIR), determinism (`math.rs`, clippy ban), family parameter tables (additive, noise, drum, metal), commands. Time × band correction `tf` section + pilot before/after table. `env`, `dly`, `spread`, string attack, `refine`; pilot table across 3 rounds. Round 4: A-weighted score + `d_onset`, loop vibrato/AM, stretched loop, drum noise. Round 5: `am`/`vib` tables. Round 6: `side` table, 512 budget. |
