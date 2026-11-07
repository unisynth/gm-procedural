# Proposal

## Why

The procedural GM bank (change `add-procedural-gm-bank`) is fitted to Windows `gm.dls`, so it inherits that bank's
quality: 22.05 kHz mono waves taken from the cut-down Virtual Sound Canvas. `gmplus.sf2` (Release 1) is a fan
soundfont built from Sound Canvas samples. It has the same 235 presets as gm.dls, mostly 32 kHz samples and
hand-tuned articulation, and it aims to sound like gm.dls done properly.

Because our waves are synthesised, a better target costs no bytes. Fitting the same skeleton against gmplus at
32 kHz gives a brighter, cleaner bank that still plays every existing song unchanged. Synthesis can also produce what
the stereo image gmplus gets from panned layers, reproduced procedurally.

## What Changes

- **A second parameter set, `assets/gm-plus/`:**
  - It holds its own `structure.json`, `waves/NNN.json` and `scores.json`, plus `map.json`, the provenance of each
    target.
  - It keeps the gm.dls instruments, regions, articulation and wave indices.
  - Every wave moves to 32 kHz, and its length and loop come from the gmplus target.
  - It never touches `assets/gm/`, which another session is still fitting.
- **Targets from gmplus:**
  - Each of the 495 waves gets a target taken from the gmplus preset and zone that play that wave's key: the raw
    sample, with gmplus's static filter applied and the DC offset removed.
  - The target is resampled to 32 kHz so that its loop is a whole number of samples. Any remaining pitch error goes
    into the fine tune.
  - Each target is level-matched to the gm.dls wave, so the gm.dls articulation and gains still balance.
  - The gmplus file is read from `GMPLUS_SF2` and is never committed.
- **Stereo waves, only where gmplus has stereo:**
  - gmplus pans the layered zones of 60 waves apart (detuned EPs and organs, 12-string, accordion, applause, …). For
    those, the target is stereo: the left channel is the wave's target and the right channel the target of a partner
    wave with its own fitted parameters. Every other wave stays mono; no stereo is designed where gmplus has none.
  - The generated DLS stores the partner as an extra wave plus a stereo-pair chunk. Readers that do not understand
    the chunk play the left channel and stay correct.
- **`gmfit` gains a target-set choice (`--set gm|plus`)** and a `plus-map` command. Every existing command
  (analyze, fit, correct, refine, bulk, render, report) works on either set unchanged.
- **`gmgen` builds either set** (cargo feature `plus`). The player picks one at compile time.
- **`unisynth` plays stereo pairs** behind a new feature: a DLS region whose wave has a pair plays left and right in
  lock-step.
- **Four-way comparison in `tools/gm-songs`:** every song is rendered through
  - original gm.dls,
  - gmplus.sf2,
  - the bank generated from gm.dls,
  - the bank generated from gmplus (stereo).

  The A/B page plays all four, and the metrics compare each generated bank with its own source.

## Capabilities

### New Capabilities
- `gm-plus-bank`: the gmplus-derived procedural bank. It covers target extraction from the soundfont, the 32 kHz
  skeleton, the separate parameter set, stereo generation, set selection in the fit tool and the generator, and the
  four-way comparison.

### Modified Capabilities
- `gm-sound-bank`: the reader exposes a wave's stereo partner, and the writer emits stereo pairs. Banks without the
  chunk read as before.
- `unified-synth`: DLS-instrument and DLS-wave sources play a stereo pair as left and right channels.

## Impact

- **Code:**
  - `crates/gmfit`: SF2 reader, target extraction, `--set`, the stereo checks.
  - `crates/gmgen`: the `plus` set, rate per wave, stereo rendering.
  - `crates/dls`: pair chunk in the reader and writer.
  - `crates/unisynth`: the stereo sampler.
  - `tools/gm-songs`: the four banks and the page.
  - `docs/synth/gm-procedural.md`.
- **Assets:** a new `assets/gm-plus/` tree. `assets/gm/` is unchanged.
- **Player size:** the stereo sampler is feature-gated and only enters builds whose song uses the plus bank, so
  compo builds measure it. Load-time generation grows with 32 kHz and the 60 partner waves.
- **Dependency:** this builds on `add-procedural-gm-bank`, which is still in progress. It uses that change's families,
  score and fit loop.
- **Licence:** the same stance as the gm.dls bank. Parameters derived from the Roland samples are committed; the
  soundfont never is.

## Discipline Skills

- `doubt-driven-review`: the DLS stereo-pair extension format, and committing parameters fitted to Sound Canvas
  samples.
- `performance-optimization`: the load-time generation budget at 32 kHz stereo, and the size of the stereo sampler.
- `systematic-debugging`: waves whose gmplus target maps badly or does not converge.
