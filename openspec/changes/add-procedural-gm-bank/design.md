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
- The user's constraints: synthesis models only, ≤ 512 numbers per wave, no stored sample data, and harmonic tables
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

A wave has at most 4 layers and at most 512 numbers. Harmonic table entries count towards the 512. Because of the
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
  linear-phase FIR (Hann-windowed) whose length spans at least 4 periods of f0 (129–2049 taps), so that neighbouring
  harmonics get their own gains. *Measured:* a fixed 129-tap filter cannot resolve 65 Hz harmonics, and it lowered the
  score. The filter is applied with its delay compensated.

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

### D4c. Optional time × band correction `tf` (per wave)
The harmonic table is static, so it cannot follow brightness that changes over time (piano and snare attacks,
hats) or unpitched drums. The second stage, `wave = tf(correct(Σ layers))`, is a grid of dB gains:
- 20 log-spaced bands (centres 55 Hz · 1.315^b, up to about 10 kHz).
- Up to 9 time rows (0, 5, 10, 20, 40, 80, 160, 320, 640 ms). Only the rows the attack needs are stored
  (`rows_for`), at most 180 numbers.

Gains interpolate linearly in time (held after the last row) and in log2 frequency (held outside the bands). They
are applied through an STFT (N = 512, hop N/4, Hann analysis + synthesis, deterministic radix-2 FFT). On a looped
wave only the attack is shaped, and the gains fade to unity over the last 10 ms before the loop start. The loop
stays exact and periodic, and remains the harmonic table's job.

`gmfit correct` fits the harmonic table first, then `tf`. It measures band energies on the same grid
(`tf::measure`, frame power shared by the interpolation weights) for the original and the model attack. Each cell
moves by the dB difference, over 4 passes, keeping the best. Cells more than 60 dB below the original's loudest cell
stay 0. If the budget is short, the harmonic table's tail is trimmed first.

*Measured (pilot):*

| wave | before | after (harmonic + `tf`) |
|---|---|---|
| snare 161 | 0.45 | 0.70 |
| kick 376 | 0.67 | 0.74 |
| hat 83 | 0.57 | 0.66 |
| Piano 293 | 0.74 | 0.84 |
| Fingered Bass 240 | 0.956 | 0.985 |

A 12-band grid scored up to 0.02 lower than 20 bands. The harmonic table now compares *cluster* energy (±m/2 bins
around k·m). Single bins called inharmonic piano partials "missing" and set −24 dB.

### D4d. Fine envelope `env`, string attack, ensemble spread
These came from the round-2 listening feedback ("is there any way to improve?").
- **`env`.** dB gains at 24 fixed times (0–400 ms, about 1 ms apart at the onset), applied per sample to the
  attack after `tf`. On a looped wave it fades to 0 dB before the loop start, and the loop is untouched. The drums'
  error was mostly envelope detail finer than the 23 ms `tf` STFT. It is fitted analytically (RMS around each
  point, 6 passes).
  - *Measured:* kick 0.74 → 0.88, hat 0.66 → 0.77, snare 0.70 → 0.76.
- **`refine`.** Alternates `fit` with the tables in place and `correct`.
  - *Measured:* +0.008 on the hat, 0 elsewhere. The scalars sit in a local optimum that the tables were fitted
    to. It is kept because it is cheap.
- **Stiff-string attack** for struck and plucked strings. A free-running `additive` `attack` layer takes B,
  `pdecay` and its table from early and late spectra, with a new envelope scalar `dly` (structural, never fitted)
  so the sustain layer starts only at the loop.
  - *Measured on Piano 293:* B = 3.3·10⁻⁴ (harmonic 14 is +55 cents sharp), and harmonic 5 decays 20 dB in 0.3 s
    against 4 dB for harmonic 1. The plain model goes 0.74 → 0.76, but corrected it reaches 0.840 against 0.849
    for the harmonic model. The higher score stays committed, and the alternative goes to listening.
- **`spread`** (cents sd) on `additive`. The Strings loop measures each harmonic as a cluster of about 10 peaks
  with a near-constant 12-cent sd, a chorus that 2-voice `unison` cannot make. Every partial becomes a Gaussian
  cluster of components on the loop grid, so the loop stays exact.
  - *Measured:* Strings 0.679 → 0.712, `d_loop` 0.14 → 0.05. The remaining error is the shimmer *pattern*, which,
    like noise, can't match sample for sample.

### D4e. Round 4 analysis and model fixes
- **Drum noise layer.** Kicks, like snares, get a broadband `noise` layer. Bands below 2× the body are removed,
  and the level is set from the original's energy share above 2× the body. *Kick 376:* 0.41 → 0.70 (new score).
- **Loop vibrato.** The pitch track of harmonic 1 over the tiled loop gives the mean pitch (the periods per loop;
  autocorrelation said 97 where it is 98), and a sinusoid at whole cycles per loop gives `vib_rate`, `vib_cents`
  and the new `vib_phase`. The loudness track at the same rate gives the new `am_db`/`am_phase`: amplitude
  modulation locked to the vibrato phasor, periodic on the loop. With vibrato present, neither `spread` nor
  `unison` is set (sidebands look like twin peaks and a cluster).
  - *Strings 405:* what looked like a 12-cent ensemble was a 2.27 Hz (one cycle per loop) vibrato of about ±12
    cents with ±1.5 dB loudness wobble. Score 0.58 → 0.67, and loudness sd 1.06 dB against the original's 1.22.
- **Stretched loop.** For inharmonic priors, the loop clusters centre on √((1+Bk²)/(1+B))·k·m with a running B.
  *Piano 293:* the loop now matches exactly (level, centroid, high band, the 334 Hz stretched pitch read). Score
  0.68 → 0.75 with the stiff-string attack layer, which is now committed.
- **The string layer carries the attack alone.** The generic "attack layer = half the onset level" recalibration
  is skipped for it.
- **Structural scalars stay out of CMA-ES:** `unison`, `dly`, `spread`, `vib_rate` on sustain layers, and the
  envelope of a delayed layer. Each of them broke a hand-over or a grid lock when fitted.
- The attack time comes from a 0.5 ms envelope.

### D4f. Per-harmonic modulation of a vibrato loop (Strings 405, round 5)
Listening: "405 needs to improve". Measured on the loop's per-harmonic complex envelopes (cluster bins → analytic
→ inverse DFT):
- The harmonic levels already matched within 0.3 dB.
- Each harmonic has its own loudness curve: up to ±5.7 dB (h2) against the global ±1.5 dB.
- The pitch curve is not a sine: the components at 4–6× the loop rate are as strong as at 1×.

Tried and measured:
- Fixed body resonance (gain = H(k·f(t)), slope and curvature per harmonic): no better than sinusoids, so rejected.
- Ensemble `spread` on top: −0.14 score, and it *reduced* the loudness wobble (random phases cancel the locked
  modulation), so rejected.
- Random per-harmonic modulation (`am_rand`): worse on every measure, so removed.

Adopted:
- `additive` table `am`: per harmonic, [dB, phase] at 1×–4× the vibrato rate (8 numbers; the analysis fills the
  first 12 harmonics). The harmonic table holds the unmodulated level, with the modulation's power excess removed.
- `additive` table `vib`: the pitch curve as [cents, phase] at 1×–6× the rate, overriding `vib_cents`/`vib_phase`.
  It is taken from harmonic 1's instantaneous frequency, because a median over harmonics 1–4 flattened it.
- Both run on the vibrato phasor, so the loop stays exact. The 4-term rows leave 0.3 dB residual on h1 and 1.2 dB
  on h3; above h8 the loudness fluctuation is irregular (3–5 dB even with 8 terms) and stays unmodelled.

*Measured:* Strings 0.674 → 0.788; loop loudness sd 1.21 dB against the original's 1.20; pitch sd 11.1 against 11.9
cents.

### D4g. Sideband rows and a 512-number budget (Strings 405, round 6)
Feedback: "retry", then "increase budget if needed". The budget went from 256 to 512 numbers per wave.

New `additive` table `side`: rows of [harmonic, loop-bin offset, dB re the harmonic's level, phase]. A harmonic
with rows is exactly those components on the loop grid, so its own AM, its own FM and their interaction are
reproduced. Unison, `spread`, `vib` and `am` don't apply to it.

The analysis takes, for harmonics 1–12 while the room lasts, the strongest bins of each cluster until 99.5 % of its
energy (≤ 11), scaled to the full cluster energy. The `am` rows remain for the harmonics past them.

| Measured | 97 % / ≤ 7 bins | 99.5 % / ≤ 11 bins |
|---|---|---|
| d_spec | 0.93 (better) | 0.82 |
| d_env | 0.53 (worse) | 0.27 |

At 97 % the dropped small components are what shape the loudness dips.

Strings 0.788 → 0.815. Low-harmonic cluster widths are now 11/12, 12/15 and 15/17 cents against the original.

Rejected:
- `spread` on the harmonics past the `am` rows: −0.02.
- Partials up to 0.495·rate: +0.008 on 405, but it regressed 33 placeholder waves.

### D4h. Phase seed against buzz (Strings 405, round 7)
Feedback: "much better, but there is a buzz in the centre of the spectrum".

Harmonics without a stored phase used Schroeder phases. On a steady upper spectrum these line up into one
sharp pulse per period. Above 2.5 kHz the generated sound peaked at about 5× its average power within each
period, against about 3× in the original. The levels matched within 0.3 dB, and the score (frame metrics)
could not see the difference.

- New `additive` scalar `phase_seed`: 0 keeps the Schroeder phases, so no other wave changes; n gives hashed
  random phases. It is excluded from the CMA-ES fit.
- `gmfit` picks the seed from 0–16 whose per-band `buzz` best matches the original's in the bands 1.5–2.5,
  2.5–4, 4–7 and 7–10 kHz. It picks during analysis and again after the correction tables.
- New scorer flag `buzz`: upper band more than 1.4× the original's buzz. It does not change the score.

| | 1.5–2.5 | 2.5–4 | 4–7 | 7–10 kHz |
|---|---|---|---|---|
| original | 2.67 | 2.64 | 2.67 | 2.55 |
| seed 0 (round 6) | 3.09 | 5.24 | 4.41 | 3.09 |
| seed 10 | 2.67 | 2.56 | 2.43 | 2.85 |

### D4i. Gap noise (Strings 405, round 8)
Feedback: "there is a pattern in the 405 differences". The difference map was red on the harmonics and blue
between them. The original has a noise floor in the middle of the gaps (bow/rosin noise, section smear). Above
2.5 kHz most of its energy lies between the harmonics (73 % against 33 % for round 7), and the model's mid-gap
floor sat 12–60 dB below it.

`gmfit::analyze::gap_noise` adds a periodic `noise` sustain layer. It runs during analysis and again at the end
of `correct_all` (the correction tables move the upper harmonics).
- The noise density per band is the original's mid-gap floor (≥ per/5 bins from every harmonic) minus the
  model's. It is averaged with the sign kept: a bin-wise positive part counts random mismatch and gave −16 dB of
  phantom noise.
- The harmonic spacing comes from the strongest harmonic comb. The pitch estimate was one bin off (97 against
  98), which put the "gaps" on the upper harmonics.
- 3 passes on the render set each band's gain to the far-gap ratio, and re-level each harmonic so that line +
  noise = the original's region total. Upper harmonics of a section are mostly floor.
- The noise is high-passed at 0.7·f0. Without it, band skirts and the loop pattern put rumble +10 dB below
  60 Hz.
- Its low-pass is picked by score from {0.18, 0.23, 0.27, 0.32}·rate and open; one wide top band cannot follow
  the falling top octave. Open: 0.62. 5.1 kHz: 0.812.
- Budget: the weakest `side` components go first, then the harmonic tail.

Result: the mid-gap floor is within 1 dB of the original's in 6 of 8 bands, and every harmonic region up to
h44 is within 1 dB. Score 0.8157 → 0.8122: `d_spec` and `d_onset` improve, while `d_loop` worsens
0.29 → 0.38 (the noise adds ±0.5 dB inside the harmonic groups). This is within the regression gate and was
written on listening feedback.

### D4j. One-shot ends and the pilot checkpoint (round 9)
Before asking for approval, the other pilot waves were refit with the current pipeline. Hat 0.651 → 0.718 and
Piano 0.753 → 0.757; the kick, snare and bass refits scored lower and were not kept.

The `gmfit` skill's measuring script (`.pi/skills/gmfit/measure.py`) then found the kick's round-3 "differs in the
end": its last 12 ms ran at −32…−38 dB, where the original fades to −54 dB, and was then cut off. The env grid's
last point (0.130 s) lay past the 0.112 s wave, so its window was empty and skipped.

Two fixes:
- **Generator:** on a one-shot, a last `env` point past the end takes effect at the last sample.
- **`correct_env`:** it measures that point on the last 5 ms. After the best pass, it settles the last point
  alone, and keeps the result unless the score drops by more than 0.002.

The original one-shots end anywhere from −5.8 to −90 dB (median −37), so a fixed fade-out in the generator would
be wrong.

Result: the kick's last 5 ms match (−45.8/−45.8 dB), kick 0.6996 → 0.7091, and snare 0.6769 → **0.7408** (its
end fade was unreachable too). Hat unchanged.

### D4k. Gap noise calibrated by periodicity (Strings 405, round 10)
Feedback: "405 has shhhsss noise".

The round-8 noise matched the mid-gap energy, but in the 3–6 kHz band it carried about 36 % of the band energy
as unpitched noise. In the original most of that mid-gap energy is pitched smear: wide harmonic clusters, more
energy 10–20 bins out than at the line.

Per-band periodicity, the best normalised autocorrelation at one period ±4 % over 50 ms frames (1 = pitched):

| band | original | round 8 | round 7 (no noise) | round 10 |
|---|---|---|---|---|
| 1.5–3 kHz | 0.93 | 0.89 | 0.96 | 0.92 |
| 3–6 kHz | 0.80 | **0.56** | 0.90 | **0.81** |
| 6–9.5 kHz | 0.45 | 0.44 | 0.73 | 0.61 |

The fix: `gap_noise` keeps the energy-based start, but its 3 refinement passes now set each noise band so that
the noise's aperiodic share, (1 − p_model) − (1 − p_model without noise), equals the original's. A band that
needs none drops to −60 dB. The low-pass is still picked by score.

Tried and rejected:
- **An `additive` `skirt` table** (Gaussian grid components around each line, level fitted on the near profile)
  fought the per-harmonic re-levelling: the lines dropped 10–25 dB and `d_loop` went 0.38 → 1.60. Removed.
- **Open or high low-pass with the periodicity calibration:** the top band became hissy (0.26 / 0.35) and scored
  0.66–0.74.

The score drops 0.8122 → 0.7974. `d_loop` rises 0.38 → 0.59: the 10–10.8 kHz groups lose the noise above the
generator's partial limit, and h19/h22 sit 3–5 dB low. The score does not hear the hiss, so the result was
written and re-baselined on listening feedback. `measure.py` now has the periodicity check.

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
- `d_env`: mean dB difference of RMS envelopes, with windows ≥ 2 periods of the unity pitch and ≥ 10 ms, every 2 ms.
  *Measured:* 2 ms frames are shorter than a low note's period, so they tracked waveform phase rather than the
  envelope. The smallest STFT is likewise ≥ 2 periods.
- `d_loop`: log-magnitude difference of the loop's DFT (exact bins at multiples of rate/L; DC excluded).

**Round 4 (listening: "376, 405, 293-alt end differ"; the score had ranked them well).** Measured causes:
- The kick lacked its 2 kHz click and a broadband texture 20–35 dB down. `d_spec` didn't see it, for three
  reasons:
  - The 50 Hz body set the peak, so the −50 dB floor hid the high band.
  - The first STFT frame began at sample 0, so its Hann window was nearly 0 over the click.
  - A 2 ms click was averaged over 0.5 s.
- The strings' shimmer was punished for its pattern, so the fit flattened it.

Changes:
- **A-weighting** of every mel band and loop group before the floor.
- **Frames centred from t = 0.**
- **`d_onset`**: the A-weighted spectrogram distance over the frames in the first 50 ms only.
- **`d_env` over the looped part** compares 100 ms-smoothed envelopes plus the difference of fine-envelope sd
  (wobble depth). The attack stays exact.

The scale dropped (pilot mean −0.1), and `scores.json` was re-baselined. Organ 1's analysis fell from 0.99 to 0.88
on `d_onset`: its 2–3 kHz key click in the first 10 ms, which is real and not modelled yet.

Combined score: `score = exp(-(d_spec/12 + d_onset/12 + d_env/6 + d_loop/12))`. It equals 1 for identical waves and falls toward 0
for silence. The weights are recalibrated in the pilot (task group 5), and any change re-baselines `scores.json`.
Flags:
- `pitch_off`: the f0 estimate differs by more than 5 cents.
- `env_off`: the envelope error is above 6 dB at some 10 ms span.
- `loop_click`: the wrap-step bound from the spec is exceeded.
- `silent`, `clipped`, `nan`.

Tiers for the report, calibrated on the pilot listening (task 6.2): A ≥ 0.85, B ≥ 0.70, C below that. A wave with a
defect flag (`buzz`, `clipped`, `loop_click`, `silent`, `nan`) is C whatever its score. The approved pilot waves
scored 0.71–0.89, and every rejected round had a cause the score could not see (a missing click, an abrupt end,
buzz, hiss); those causes now have flags or `measure.py` checks. The weights stay as in round 4.

### D7. Fit = analysis initialisation, then CMA-ES
`gmfit analyze` derives a starting point:
- f0 from the loop length and autocorrelation.
- The harmonic table from the loop DFT. This is exact for the loop part, apart from truncation to the budget.
- Envelope breakpoints from the RMS curve.
- Noise bands from the spectrum residual after the harmonics are removed.
- Drum pitch sweep from instantaneous frequency.
- The family comes from the GM category of the instruments that use the wave (piano → `modal` + `additive`, guitar
  → `ks` + `additive`, drum-kit wave → `drum`/`noise`, and so on). The agent can override it per wave file.

`gmfit fit` then runs CMA-ES over the free parameters, normalised to [0,1] by per-parameter ranges (log mapping
for wide positive ranges). It is our own deterministic, seeded implementation with **full covariance** (cyclic Jacobi
eigendecomposition; n ≤ ~60). *Measured:* the separable (diagonal) variant first planned stalls on Rosenbrock, and fit
parameters are coupled the same way (level, decay and tilt). It does not use the `cmaes` crate, which pulls in
nalgebra and is not seeded deterministically. The harmonic table is refined afterwards analytically: 3 passes moving
each entry by the loop's harmonic level error, each kept only if the score improves. The parameter file is written
only on improvement (spec).
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

- [Piano, nylon guitar, sax and choir attacks will not reach tier A within 512 numbers] → The report shows it. The
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
