# Design

## Context

- Every bank consumer takes the bank as raw bytes: `Option<&[u8]>` in `song::render`, the unisynth `smp::Inst`, the
  WaveSabre sampler, and synth-wasm `set_bank`. `crates/dls` parses these bytes in place without copying. The DAW
  worklet already receives a `"bank"` message that carries those bytes.
- Facts about Windows `gm.dls`, measured with `dls` `examples/dump`:
  - 235 instruments (9 drum kits) and 495 waves.
  - 1,539,382 samples in total. 492 waves run at 22,050 Hz and 3 at 24,000 Hz.
  - Wave lengths: median 2,005 samples, p90 7,419, maximum 18,202.
  - 439 waves loop. The loops are short, typically 40–800 samples, which is one to a few periods.
  - 448 waves are melodic and 91 belong to drum kits.
- `crates/player` has no audio yet. In this change the bank policy is used by the `song` CLI, synth-wasm and the
  DAW. The player adopts the policy when it gains audio.
- The user's constraints: synthesis models only, ≤ 256 numbers per wave, no stored sample data, and harmonic tables
  are allowed. Code size and parameter size do not matter.

## Goals / Non-Goals

**Goals:**
- Pick parameters by measurement. The fit loop renders exactly the bytes that ship, because it calls the runtime
  generator.
- Make the loop easy for an agent: one command renders, scores and shows a wave, and one command fits it.
- Keep the bank bit-identical between native and wasm, so the DAW and native renders match.

**Non-Goals:**
- Matching macOS `gs_instruments.dls`. The target is the Windows bank only.
- Changing the bank structure (instruments, regions, articulation). It is copied as-is.
- Neural or learned models.
- Real-time generation per note. The bank is generated once at load.
- Wiring the bank into `crates/player`. The player has no audio path yet.

## Decisions

### D1. Output a DLS buffer instead of adding a new sampler source
`gmgen` builds the full `RIFF 'DLS '` buffer with a new `dls` writer (feature `write`). All consumers keep reading it
through `Dls::parse`. That keeps regions, `wsmp`, articulation and the 1 + drums·16384 + bank·128 + program
addressing exactly the same.
- *Rejected: a `Source::Procedural` variant in unisynth and WaveSabre.* It would duplicate region and articulation
  logic in two synths and in the DAW library.

### D2. Committed data layout
- `assets/gm/structure.json` is written once by `gmfit extract` from `GM_DLS`. It holds instruments (with names),
  regions, articulation connections, `wsmp`, and each wave's rate, length and loop.
- `assets/gm/waves/NNN.json` holds one wave each: `{ "wave": N, "layers": [ { "family": "...", "region":
  "attack|sustain|whole", "p": { ... } } ], "gain": g }`.
- `assets/gm/scores.json` maps each wave to its score and components. It is the regression baseline.
- `gmgen`'s `build.rs` uses `serde_json` as a build dependency. It validates the budget, the family names and the
  ranges, and fails the build with the wave number on any error. It packs everything into one little-endian binary
  table, which is pulled in with `include_bytes!`. The runtime crate stays `no_std` and parses no JSON.
- *Rejected: parsing JSON at runtime* (needs a `no_std` JSON parser and is slower).
- *Rejected: one big JSON file* (merge conflicts while many waves are being fitted, and diffs are harder to read).

### D3. Wave = attack part + exactly periodic loop
A looped wave of length `n` with loop `(s, L)` is built so that `[s, s+L)` is one exact period of a periodic signal.
Each layer declares one of three regions:
- `sustain` layers (`additive`, `formant`, `subtractive`, `fm` with integer ratios, periodic `noise`) are periodic
  over `L`. Their partial frequencies are snapped to whole cycles per loop: `f_k = round(k·f0·L/rate)·rate/L`.
  Their envelopes are held constant from `s` on. The wrap is therefore seamless by construction, which satisfies the loop requirement. Before
  `s` the same oscillators run with attack envelopes, so the join into the loop at `s` is continuous as well.
- `attack` layers (`drum`, `ks`, `modal`, `metal`, `shaker`, transient `noise`) are multiplied by a fade that reaches 0
  at `s`. Exception: when the original loop itself carries a decaying `ks`/`modal` tone, the layer is rendered as
  `sustain`, and its partials are snapped to the `rate/L` grid and frozen from `s` on. The original loop is also a
  frozen grid-periodic snapshot, so nothing is lost (research §0).
- `whole` is used only for unlooped waves (one-shot drums and effects).

Periodic noise is a deterministic `L`-sample noise pattern filtered circularly. It is used for bow and breath noise
inside loops.
- *Rejected: crossfade looping after free synthesis.* It smears the loop and makes the score depend on where the
  crossfade sits.

### D4. Model families (fixed set, parameters per layer)

The instrument techniques, priors and sources behind this table are in `research/gm-procedural-instruments.md`.

| family | parameters (numbers) | covers |
|---|---|---|
| `additive` | f0 offset in cents; K harmonic amplitudes in dB (contiguous); tail slope beyond K; optional P phases (low harmonics, otherwise fixed Schroeder phases); inharmonicity B; unison voices (≤ 3) with detune; per-partial double decay (rate pair + mix); odd/even split gain; brightness envelope (tilt) coupled to amplitude (Risset brass rule); attack pitch scoop; vibrato rate and depth; drawbar mode (9 Hammond partials ½–8×) | organ, brass, reed, pipe, strings, pads, leads, piano sustain, accordion |
| `ks` | extended Karplus-Strong: excitation (noise/pulse mix, colour, hardness); pick-position comb; dynamics low-pass; fractional-delay tuning; dispersion all-pass (stiffness); loop damping and brightness; ≤ 4 commuted body resonances in the excitation; optional buzz nonlinearity (sitar/shamisen bridge); unison detune | guitars, harp, basses, harpsichord, clavinet, koto, sitar, banjo, pizzicato, dulcimer |
| `modal` | M modes × (ratio, amplitude, decay) plus strike noise and strike position; fixed-frequency modes (stick sound) allowed | piano attack, mallets, bells, kalimba, steel drum, agogo, woodblock, claves, timpani, triangle, orchestra hit |
| `noise` | up to 8 bands × (centre, Q, gain), amplitude envelope, band-centre sweep, AM rate/depth; periodic or transient | breath, bow, chiff, snare wires, brush, seashore, helicopter, gunshot, key click |
| `drum` | body sine/triangle with pitch sweep (start, end, time); ≤ 3 extra membrane modes (ratio, amp, decay); amplitude decay; beater click; drive | kicks (acoustic and 808), toms, taiko, snare shell modes, synth drum |
| `metal` | ≤ 6 square oscillators at inharmonic ratios of a base frequency, or ≤ 3 ring-modulated square×saw pairs; band-pass (centre, Q) plus high-pass; amplitude envelope (one or two stages) | 808/909 hats, cymbals (ride/crash/splash/china), cowbell, reverse cymbal |
| `shaker` | PhISEM: collision rate, system decay, sound decay, ≤ 3 resonances (frequency, Q, gain), shake envelope | maracas, cabasa, tambourine jingles, sleigh bells, shaker, applause |
| `fm` | 2–4 operators: ratio, fixed-frequency flag, output level, index envelope (attack, decay, sustain), feedback on one operator, algorithm (stack or parallel pairs) | EP2 (DX7 piano), tubular bell, FM brass, bird tweet, synth bells and pads |
| `formant` | source (saw/pulse width/noise mix, f0, jitter); ≤ 5 resonances × (frequency, bandwidth, gain); envelope; vibrato | choir, voice oohs, synth voice, bowed-string body, sax/oboe/bassoon body |
| `subtractive` | 2 oscillators (shape, pulse width, detune, mix, unison ≤ 3); resonant filter (type, cutoff, resonance, envelope amount, key tracking); envelopes | synth leads, pads, synth bass, synth strings, synth brass |

**Analysis priors.** `gmfit analyze` picks the families and their starting values from the GM program or drum key
that uses the wave, out of a committed priors table (`crates/gmfit/src/priors.rs`) that holds:
- mode-ratio sets: marimba 1:3.99:10.65, vibraphone 1:2.01:3.9:14.37, xylophone 1:3:9, free bar
  1:2.756:5.404:8.933, tubular bell, Risset bell, membrane 1:1.593:2.136:2.296:2.653:2.917, timpani 1:1.5:1.98:2.44,
  kalimba ≈ 1:6.3, steel pan 1:2:3:4, agogo 1:4.08:6.669
- 808 hat oscillators (205.3, 304.4, 369.6, 522.7, 540, 800 Hz) and the cowbell (540/800 Hz)
- the Csound SATB vowel formants
- the Hammond footages
- piano inharmonicity B by register
- vibrato and pitch-scoop defaults

The priors also narrow the CMA-ES ranges, for example ±5 % around a mode ratio.

A wave has at most 4 layers and at most 256 numbers. Harmonic table entries count towards the 256. Because of the
contiguous table plus tail slope, about 200 harmonics fit in the budget for low, bright waves. Phases are not counted
in the spectral score (D6), so they are stored only for the first P harmonics, and only when the waveform shape is
audible (sharp pulses).

### D4b. Optional harmonic correction (per wave)
The correction is a separate stage after the layers: `wave = correct(Σ layers, table)`, where `table` = C gains in dB
for harmonics 1..C of the wave's f0. It is clamped to ±24 dB and absent by default.
- **Loop `[s, s+L)`:** one exact period, so take the DFT of the L samples, multiply the bin at each harmonic by
  `10^(g/20)` (bins between harmonics get the interpolated gain), and take the inverse DFT. The result is exact and
  still periodic, so the wrap stays seamless.
- **Attack `[0, s)` and unlooped waves:** the same points are interpolated (in dB, over log-frequency) into a
  zero-phase linear-phase FIR (129 taps, windowed). The filter is applied with its delay compensated. The FIR spans
  only 129 samples, so it also smooths the join into the loop at `s`, and the first loop period is the corrected
  one.

This lets a behavioural model (`ks`, `modal`, `subtractive`, `formant`) keep a small parameter set while the
correction takes over the timbre detail that would otherwise need a large `additive` table. The order for the fit
becomes:
1. Fit the model.
2. `gmfit correct`: compute `corr_k = 20·log10(|orig_k| / |model_k|)` from the two loop DFTs (for unlooped waves,
   from the 2048-sample STFT averaged over the steady part). Clamp it, and keep the largest |corr| until the budget is
   used up. Set the other gains to 0 dB (a contiguous table, trailing zeros trimmed).
3. Optionally refine with CMA-ES jointly with the model.

`additive` keeps its own table for waves that are *purely* additive (organs, pads). The correction then stays
empty, so no budget is spent twice.
- *Rejected: an additive residual layer (orig − model).* Adding partials needs amplitude and phase per harmonic,
  which costs twice the numbers. It also can't remove energy the model puts where the original has none, and a
  multiplicative gain can.

### D5. Determinism
`gmgen` has its own `sin`/`exp`/`pow2` polynomial approximations and an xorshift RNG. It does not use `libm`
transcendental functions and has no `mul_add` (Rust does not fuse on its own, and wasm32 has no FMA). Oscillators
are complex rotators with renormalisation every 256 samples. Output is f32, converted to 16-bit by round-half-even.
A test compares a SHA-256 of the native full bank against a hash produced by the synth-wasm build running under
Bun (`gm_build`).

### D6. Score
The original and generated waves are both "played": the attack, then the loop unrolled to at least 0.5 s, so the
sustain counts in proportion to how it is heard. Components:
- `d_spec`: mean absolute log-magnitude difference (dB, floored at −80 dB) over a mel-spaced band grid. STFT windows
  are 128, 512 and 2048 samples, and the transient frames carry extra weight.
- `d_env`: mean dB difference of RMS taken over 2 ms frames.
- `d_loop`: log-magnitude difference of the loop's DFT (exact bins at multiples of rate/L).

Combined score: `score = exp(-(d_spec/12 + d_env/6 + d_loop/12))`. It equals 1 for identical waves and falls toward 0
for silence. The weights are recalibrated in the pilot (task group 5), and any change re-baselines `scores.json`.
Flags:
- `pitch_off`: the f0 estimate differs by more than 5 cents.
- `env_off`: the envelope error is above 6 dB at some 10 ms span.
- `loop_click`: the wrap-step bound from the spec is exceeded.
- `silent`, `clipped`, `nan`.

Tiers for the report: A ≥ 0.95, B ≥ 0.85, C below that.

### D7. Fit = analysis initialisation, then CMA-ES
`gmfit analyze` derives a starting point:
- f0 from the loop length and autocorrelation.
- The harmonic table from the loop DFT. This is exact for the loop part, apart from truncation to the budget.
- Envelope breakpoints from the RMS curve.
- Noise bands from the spectrum residual after the harmonics are removed.
- Drum pitch sweep from instantaneous frequency.
- The family comes from the GM category of the instruments that use the wave (piano → `modal` + `additive`, guitar
  → `ks` + `additive`, drum-kit wave → `drum`/`noise`, and so on). The agent can override it per wave file.

`gmfit fit` then runs CMA-ES over the free parameters, normalised to [0,1] by per-parameter ranges. It is our own
deterministic implementation (diagonal covariance and rank-μ update, about 200 lines), seeded. It does not use the
`cmaes` crate, which pulls in nalgebra and is not seeded deterministically. Harmonic amplitudes come out of the
analysis already close, so they are tuned last, in a block-coordinate pass. The parameter file is written only on
improvement (spec).
- *Rejected: DDSP / gradient descent.* KS and modal generators are not differentiable in their current form, and a
  Python stack would split the runtime and fit code.

### D8. Agent loop and tooling
`gmfit` mirrors `st tex`: JSON report on stdout, artefacts in `build/gm/<NNN>/` (`orig.wav`, `gen.wav`, `sheet.png`,
`report.json`), exit 0 for ok or 2 for flags.
- `correct` computes the analytic harmonic correction (D4b).
- The report shows the score with and without the correction, so the agent sees how much the model itself gets
  right.
- `sweep` renders contact-sheet rows.
- `report` writes `build/gm/report.md` and a bank-wide sheet sorted by score. `report --check` is the regression gate.

The `.pi/skills/gmfit/SKILL.md` skill teaches this loop:
1. `render`
2. Read the sheet and flags.
3. Change the family or layers.
4. `fit`, then `correct`.
5. Listen A/B in the DAW (`?bank=os` vs procedural).
6. Commit when the score improves.

### D9. Bank selection policy and the DAW
- `gmgen::policy` (feature `std`) implements the spec's policy. `Choice::{Auto, Proc, Os, Path}`. `Auto` reads
  `%SystemRoot%\System32\drivers\gm.dls` on Windows only.
- `song render/check` gains `--bank proc|os|<path>`. The current default (`GM_DLS`, otherwise no bank) stays, so
  the parity tools keep their behaviour.
- synth-wasm exports `gm_build(mask_ptr?)`, which returns the buffer.
- The DAW generates the bank in its existing bounce Worker's wasm instance, which keeps the AudioWorklet free of the
  ~1 s job, and sends it to the worklet with the existing `"bank"` message.
- A bank-source selector sits next to the GM picker. It is stored in `localStorage` and can be set with
  `?bank=proc|os`. `?nobank` keeps its meaning (no bank at all).

### D10. Partial generation
`build(mask)` takes a 495-bit mask. Masked-out waves are written as zeros with the original length, so the structure
and indices stay the same (spec). A later player can compute the mask from the song's sampler slots. In this change
the mask is used for tests and timing.

## Risks / Trade-offs

- [Piano, nylon guitar, sax and choir attacks will not reach tier A within 256 numbers] → The report shows it. The
  score weights sustain as heard. Accept tier B for these and prioritise the waves used by songs in `project/music`.
- [The score disagrees with listening (a high score that sounds wrong)] → A/B listening in the DAW is part of the
  loop. The pilot calibrates the weights against listening before the bulk fit.
- [Native/wasm hash mismatch from a stray `libm` call or float-ordering difference] → A hash test runs in CI. A
  `clippy::disallowed_methods` list covers `f32::sin/exp/powf/mul_add` in `gmgen`.
- [Load time over budget, mostly the additive attack with many harmonics] → Rotators instead of `sin`. Harmonics
  above Nyquist and below −80 dB are culled. The loop period is rendered once and copied. Mask builds remain the
  fallback for the player.
- [Committed parameters are derived from licensed recordings] → They are bounded synthesis parameters with no
  sample data, and `NOTICE.md` records this. The PCM-free check is a test (spec scenario "No PCM committed").
- [Fitting all 495 waves is long] → Every fit is seeded and resumable. `report` lists what is left. Waves are fitted
  family by family, so new family code is exercised early.

## Migration Plan

Purely additive. Existing songs and tools keep their default bank behaviour. The DAW default switches to Procedural,
and OS bank is one click away. Rollback: set the DAW default back to OS bank. Nothing else depends on the generated
bank.

## Open Questions

- Should the generated DAW bank be cached in IndexedDB, keyed by the `gmgen` data hash, if generation in wasm turns
  out to be noticeably slow? Decide after timing it.
- Score weights and tier thresholds will be recalibrated in the pilot. They are data, not structure.
