# Proposal

## Why

The procedural GM bank work (change add-procedural-gm-bank) built an analysis → fit → correct pipeline that turns
a sampled wave into ≤ 512 synthesis parameters. It also built a set of measurements that found what the score
missed: vibrato and ensemble width, per-band buzz, the noise floor between harmonics, rumble, the top octave, and
level per time and band. All of it is tied to GM wave indices, and the diagnostic part only exists as ad-hoc
scripts.

A song's own samples are stored as GSM, about 9 KB per second. A procedural sample costs about 2 KB whatever its
length. The same pipeline can make any WAV procedural, and the measurements can check any original/generated
pair.

## What Changes

- **Any WAV as a target.** `gmfit` works on a target (samples, rate, length, loop, prior, seed) instead of a GM
  wave index. GM waves become one kind of target and must give byte-identical results.
- **Loop detection.** The loop comes from the WAV's `smpl` chunk or is detected: whole periods, and whole vibrato
  cycles when there is vibrato. A sound without a usable loop is a one-shot.
- **Automatic classifier.** It picks the prior (kind and families) from the sound: pitched or not, harmonic,
  stretched or inharmonic, sustained or decaying, transient, pitch glide. `--kind` overrides it.
- **WAV commands.** `gmfit wav analyze|fit|correct|refine|render|make` turn a WAV into a parameter file (params
  plus wave meta) and a rendered WAV. `make` runs the whole pipeline.
- **Diagnose.** `gmfit diagnose` takes a GM wave index or two WAVs and reports the checks the score misses: each
  as original vs generated, ok/off, and a hint naming the analysis step or parameter that fixes it. It also
  writes a difference-map image. `gmfit report` gains a column of failing checks per wave.
- **Procedural sample entries.** The unified synth's sample section may hold a procedural entry (wave meta + gmgen
  parameters) besides GSM entries. At load the generator renders it into the sample pool, and a sampler slot
  plays it exactly like a GSM sample. Feature `us-sample-gen`. Existing chunks are unchanged.
- **DAW.** A GSM sampler slot offers "Make procedural…" on import. The local server runs the analysis on the WAV
  and stores the entry in the instrument. The slot shows whether its sample is GSM or procedural, its size, and the
  score and diagnose summary.
- **Procedure doc + skill.** "Make a WAV procedural": what is analyzed and in which order, the limits, and how to
  act on diagnose hints.

## Capabilities

### New Capabilities
- `wav-resynthesis`: turning a WAV into generator parameters (target, loop detection, classification, the
  analysis/fit/correct pipeline on a WAV, the parameter-file format) and diagnosing original/generated pairs.

### Modified Capabilities
- `unified-synth`: the sample section may hold procedural entries rendered at load. "Sampler sources": the chunk
  sample source plays GSM or procedural entries.
- `synth-daw`: the sampler slot's import offers "Make procedural…" through a new local-server route, and shows the
  entry kind, size and diagnose summary.

## Impact

- **Code:** `crates/gmfit`: target abstraction across analyze/fit/score/main, plus new `wav`, `loopfind`,
  `classify` and `diagnose` modules. `crates/gmgen`: entry encode/decode, render of a target's meta.
  `crates/unisynth`: chunk sample-section entry kinds, render into the pool. `crates/synth-wasm`: renders entries
  in the browser. `packages/cli`: `/api/procedural` route, which runs the `gmfit` binary. `packages/daw`: sampler
  slot UI.
- **Size:** the player links `gmgen` only with `us-sample-gen`. The packed-size cost is measured and gated like
  other `us-*` features.
- **Dependencies:** builds on add-procedural-gm-bank (gmgen, gmfit, the 512-number budget). That change keeps its
  scope, the GM bank.
- **Compatibility:** chunk versions and source values are unchanged. Old chunks load byte-identically.

## Discipline Skills

`security-hardening` (server route runs a binary on an uploaded file), `doubt-driven-review` (chunk sample-section
format), `performance-optimization` (load-time render cost, player size), `systematic-debugging`.
