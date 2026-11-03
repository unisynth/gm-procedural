# Proposal

## Why

The synth's GM sampler (WaveSabre Adultery, unisynth "GM.DLS wave" / "GM.DLS instrument") needs the Windows
`gm.dls`. Only Windows has that bank: Linux has none, and macOS's `gs_instruments.dls` is 8-bit and differs in 36
region maps and 65 tunings. The DAW gets the bank from `/api/bank` or ships a copy (`data/bank.dls`). We want one bank
that sounds like Windows `gm.dls` on every platform. Windows production builds keep using the real OS file. macOS,
Linux and the DAW use a **generated** bank. That bank is built at load time from fitted synthesis parameters and keeps
the same instruments, regions and wave indices, so existing songs play unchanged.

The bank is small enough to make close matching realistic. It holds 495 waves and 1.54 M samples in total. The median
wave is 2005 samples. 439 waves are a short attack followed by a loop that is a few periods long. A harmonic table
describes such a loop precisely. Each wave is therefore **synthesised by per-family models** (additive with harmonic
tables, Karplus-Strong, modal, noise, drum, formant, subtractive) within a budget of **≤ 512 numbers per wave**. No
waveform or residual PCM is stored, so the result is synthesis and not a compressed copy of the recording.

## What Changes

- **`crates/gmgen` (new, `no_std` + `alloc`):** a deterministic generator.
  - Inputs: a committed per-wave parameter set plus a committed bank-structure table (instruments, regions,
    articulation, `wsmp`, wave rate/length/loop).
  - Synthesis: each wave is a sum of up to 4 layers. Each layer comes from a fixed model family: `additive`
    (harmonic table with envelopes, inharmonicity, vibrato), `ks` (extended Karplus-Strong), `modal`, `noise`,
    `drum`, `metal` (808-style inharmonic squares), `shaker` (PhISEM), `fm`, `formant` and `subtractive`. Per-instrument
    techniques and priors: `research/gm-procedural-instruments.md`. A wave may add an optional **harmonic correction table** (per-harmonic dB gains on
    the layer sum). It is exact on the loop and works as a smooth EQ on the attack. It fixes the timbre that the
    behavioural model misses. A wave holds ≤ 512 numbers in total, and harmonic table entries count towards that.
    No sample data is stored.
  - Output: a complete `RIFF 'DLS '` buffer that the existing `dls::Dls::parse` reads. Consumers need no changes.
  - The output is bit-identical on native and wasm32, because it uses its own math and no `libm` transcendental
    functions.
- **`crates/dls` gains a writer** (feature `write`): it serialises a bank structure plus 16-bit waves to DLS. A
  round-trip is tested against the real bank.
- **`crates/gmfit` (new host tool):** the iterative fitting loop against the original `gm.dls` (read from `GM_DLS`,
  never committed).
  - `extract`: writes the bank-structure table.
  - `analyze`: produces an initial parameter set from analysis.
  - `render`: writes an A/B wav, a spectrogram contact sheet and a JSON report with the score, metrics and flags.
  - `correct`: computes the harmonic correction table analytically from the loop spectra.
  - `fit`: runs an optimiser (CMA-ES with analysis initialisation) on the multi-resolution spectral, envelope and
    loop losses.
  - `sweep`: renders parameter variants.
  - `report`: produces a whole-bank score table.
  - It reuses the `gmgen` code, so the fitted wave is exactly the runtime wave.
- **Committed artefacts:**
  - `assets/gm/structure.json` (bank structure, no audio)
  - `assets/gm/waves/<NNN>.json` (per-wave model parameters)
  - `assets/gm/scores.json` (last fitted score per wave, used as the regression baseline)
- **Bank selection:** a single `gm_bank()` policy.
  - Windows: OS `gm.dls` when readable, otherwise procedural.
  - macOS and Linux: procedural.
  - `song render/check --bank proc|os|<path>`.
- **DAW:** the bank source defaults to *Procedural*. It is generated in the worklet via the synth-wasm export
  `gm_build`. *OS bank* stays available for A/B listening when `/api/bank` serves one.
- **Agent skill** `.pi/skills/gmfit/SKILL.md`: the render → compare → fit → listen loop, modelled on `texgen`.

## Capabilities

### New Capabilities
- `procedural-gm-bank`: a generated GM bank that matches the Windows `gm.dls` layout.
  - Generator determinism and layer models.
  - Structure and wave-index compatibility.
  - Parameter storage.
  - Platform bank-selection policy.
  - The fitting tool: extract, analyze, render, fit, sweep and report, with the similarity metrics and a score
    regression gate.

### Modified Capabilities
- `gm-sound-bank`: adds the DLS writer requirement. The "never shipped" requirement is refined: the generated bank
  and its parameters are shippable, and the OS file is still never committed.
- `synth-daw`: the GM bank source becomes selectable, with Procedural as the default and OS bank for A/B. "Bank
  missing" applies only when the procedural build fails.

## Discipline Skills

`performance-optimization` (load-time generation budget in the player and the worklet), `doubt-driven-review`
(committing parameters derived from the Roland recordings, cross-target bit-identity), `systematic-debugging` (waves
that do not converge).

## Impact

- **New crates:** `crates/gmgen`, `crates/gmfit`.
- **New dependencies (host tool only):** `rustfft` and `png` (`png` is already used by `player`).
- **New directory:** `assets/gm/`.
- **Changed:**
  - `crates/dls` (writer feature)
  - `crates/song` (`--bank proc|os`)
  - `crates/synth-wasm` (`gm_build` export)
  - `packages/daw` (bank-source selector, worklet generation)
  - `packages/daw/build-static.ts`: the static DAW no longer needs `data/bank.dls`. It is still included when present.
  - `packages/cli` (`/api/bank` unchanged)
- **Load time:** generating 1.54 M samples must fit a startup budget (design: ≤ 300 ms native release, ≤ 1 s
  wasm). The player can build only the waves a song uses.
- **Licensing:** the parameters are fitted against the licensed recordings, but they are bounded synthesis parameters
  and no sample data. `NOTICE.md` records this. The OS file itself is still never committed.
- **Expected fidelity:** synthesis is close, not identical. Sustained, harmonic, plucked and drum waves are expected to
  match closely. Acoustic piano attacks, noisy ensembles and voice will match less well. The whole-bank report
  measures this.
