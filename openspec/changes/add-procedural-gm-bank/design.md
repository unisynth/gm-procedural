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

### D4l. Character analysis and the `modal` family (drum listening after bulk pass 1)
Feedback on the drum A/B page named about 18 waves. Measured on orig vs gen (`measure.py` now prints these):

| Heard | Waves | Measured |
|---|---|---|
| noise where the original is a few pure tones | congas 192/227/270, timbale 439, whistles 229/410, triangle 446, agogo 25, belltree 49 | tonal share (energy in the 8 strongest peaks) 0.82–0.98 vs 0.42–0.69; wrong peak frequencies |
| falling sine / chirp | 196, bird 50 | glide of the strongest partial |
| rattle / scrape | vibraslap 476, guiro 228 | 9–13 onsets in 0.5 s, irregular spacing |
| cymbal loops with other tones | 272, 316, 84, 315 | 1-period loops whose energy is scattered bins (no harmonic series) |

**`modal` family** (task 7.2, extended): modes `[Hz, dB, decay, phase]`, a shared glide restarted per hit, a hit
train (regular, or a measured `hits` table), and a band-passed click per hit. Sustain is exactly periodic: steady
sparse tones on the loop grid, or one loop's hit pattern folded over the loop.

**Analysis** (`gmfit/src/modal.rs`):
- `onsets`: strike times and levels, re-levelled per hit (`relevel_hits`).
- `partials`: prominent peaks of the first hit. The floor is the 20th percentile over ±max(150 Hz, 10 % f,
  6 main lobes), so a tone's own sidebands don't hide it. Each peak gets its start level and decay from a
  single-frequency tracker.
- `glide`: instantaneous frequency of the band around the strongest partial. Accepted only if it settles steadily.
- `sparse_loop`: prominent exact loop bins with phases.
- `set_click`: click band from the residual's centroid, decay from the envelope fall after onsets, level from the
  residual energy.
- `add_residual`: an 8-band noise layer for what the modes leave out (sines plus noise).

`analyze` builds these candidates for every wave and keeps the best score over the prior-based model. That
makes the model choice automatic.

**Score:** `d_per` (periodicity difference, weight 1) and flag `tone_off`. It separates noise from tone on sounds
longer than a frame (guiro 0.20). It cannot help a 29 ms glide (196): at that length the original isn't
periodic either.

### D4m. Wavelet analysis: trill and band strikes
Question: does a wavelet transform help understand an existing sound? Tested on the two failures of D4l, where a
fixed STFT window misled the analysis. A complex Morlet band (frequency-domain Gaussian around f) gives amplitude
and instantaneous frequency per sample:

| wave | | flutter (AM) | pitch movement (FM) | strikes in the 1248 Hz band |
|---|---|---|---|---|
| whistle 229 | original | 3.5 dB @ 100 Hz | ±120 Hz @ 100 Hz | |
| | modal pass 2 | 9.3 dB @ 100 Hz | beating @ 200 Hz | |
| vibraslap 476 | original | | | 23 |
| | generated | | | 10 |

The whistle's sidebands come from a trill (frequency modulation). Fixed modes at the sideband frequencies beat as
amplitude modulation instead. The STFT shows the same sidebands for both.

Changes:
- `gmfit/src/wavelet.rs`:
  - `band`: the Morlet band.
  - `trill`: rate, depth and carrier when the pitch movement is periodic. Band sd is min(f/4, 250 Hz), wide
    enough for a ±100 Hz trill and narrow enough to keep neighbouring partials out.
  - `decay`: from the amplitude smoothed over one trill period.
  - `band_onsets`: strikes inside a resonance band.
- `modal`: `trill_hz`/`trill_cents` (FM of every mode, restarted per hit). With a trill, analysis drops every mode
  within ±(3·rate + depth) of the carrier and skips band strikes, since trill dips look like strikes.
- Strikes: band onsets at the strongest partial replace broadband onsets when they find more (whole and loop).
- Score `d_mod`: flutter difference / 6 dB + pitch-movement difference / 100 cents (each capped at 1) of the
  original's strongest partial. It counts only when the original is tonal (share > 0.6), its partial is stable
  (< 300 cents) and the partial stays loud for ≥ 100 ms. Without these limits it wrecked noise and short strikes
  (kick 376 → 0.00, hat 83 → 0.002, congas). Flag `mod_off` above 0.5.

### D4n. Character descriptors, rank and the noise loop (drum listening at bank speed)

The user re-listened at bank playback speed and named 13 drum waves as still wrong (196, 58, 272, 84, 315, 435, 476,
49, 50, 62, 226, 106, 404). A descriptor study (orig vs gen, all 91 drum waves) ranked 18 candidate measures by how
well they put those 13 above the rest. `1 − score` alone put only 4 in its worst 15 %. Three descriptors find what
the frame score averages away (`gmfit/src/desc.rs`, `Score::describe`, run in `evaluate`, not in the fit loop):

| Descriptor | Measures | Flag | Caught |
|---|---|---|---|
| fine contrast | 97th pct / median of the 8192-point spectrum per octave, energy-weighted, gen − orig | `lines`: ≥ +6 dB where the original is a wash (≤ `WASH_DB` = 20 dB) | crash 106, open hat 272, vibraslap 476 (+7…+25 dB: an additive comb for a noise) |
| percussive share | median-filter H/P separation, energy in transients | `transient_off`: off by ≥ 0.2 | tambourine 435 (0.60 vs 0.22), stream 404, bubble 62, bird 50, ride 315 |
| peaks per frame | spectral peaks within 30 dB of the frame max | `peaks_off`: ratio ≥ 1.6 | wave 196 (43 vs 22: modes smeared) |

Together with a low score they cover all 13. Contrast is only meaningful on a wash: on a clean tone both are lines
and leakage swings it ±15 dB (bass 0, organ 121 gave false `smear`/`lines`), hence the `WASH_DB` gate and no
`smear` flag. Rejected (no separation): loudness flutter, crest factor, high-band kurtosis, per-partial decay
spread, attack-to-loop seam.

**Rank.** Choosing between versions of a wave (`keep_if_better`, `report --check`, `noise_loop`) uses
`rank = score − 0.1 × character flags`. The frame score rewards an additive comb whose harmonic levels copy the loop's
cluster energies; random noise cannot match those bins although the difference is inaudible on a wash, while the
comb's lines are what the user hears. The flags stay out of the score itself (the fitter keeps optimising the frame
metrics). `keep_if_better` ranks the version as it will be on disk (rounded to 5 digits: worth 0.1 on whistle 229).

**Noise loop** (`analyze::noise_loop` / `pick_noise_loop`, in `analyze` and after the corrections in `correct_all`):
when the loop is a wash and the additive comb is ≥ 6 dB peakier, try a loop-length `noise` sustain in 20 third-octave
bands (Q 4.3; the `bands` table now holds ≤ 24 rows), the comb kept at 0/−6/−12/−18 dB or dropped; band gains by 4
passes of original/model band energy, level so comb + noise = loop energy, high-pass at the first band within 40 dB
of the strongest and low-pass at the top band edge (skirts otherwise leak 15–25 dB where the original is silent).
Each candidate gets its `tf`/`env` refitted before it is ranked. Result: Chinese cymbal 84 0.57 → 0.62 and brush 58
0.61 → 0.70, both without flags; crash 106 and open hat 272 stay combs (noise 0.51/0.47, comb 0.60/0.58 with `lines`),
pending listening. Tambourine/stream/bubble need many tiny transients: the `shaker` family (7.5).

Listening: the −6 dB comb + noise mix sounded best on crash 106 but lost the metallic ring — lowering the whole comb
lowers its few prominent partials too. Added candidate: keep only partials ≥ 4 / 6 dB above the median of their ±4
neighbours at full level (with or without `spread`), drop the rest to −99 dB (the additive renderer stops at the
first harmonic below −100 dB), noise fills the wash. Crash 106: 13 of 82 partials + noise, rank 0.560 vs comb 0.554 →
saved (0.56, no flags). Open hat 272: 0.514 vs 0.529, stays a comb pending listening.

Listening approved strong partials + noise on 106 and 272; the flag cost went 0.05 → 0.1 (the comb's frame-score
lead on 272 was 0.065). Re-running the corrections on the remaining `lines` waves moved open hat 272 (0.58 → 0.51) and
vibraslap 476 (0.43 → 0.38, `lines` and `transient_off` cleared: comb + noise loop) to noise; kick 376, 18, 230, 490
kept theirs.

`d_mod` fix found on the way: its trill-rate part applied when the original moved ≥ 20 cents, but a steady tone
reads 15–25 cents of measurement wobble at a random rate — Fingered Bass 240 had dropped 0.88 → 0.30 (`mod_off`)
in drums pass 3 unnoticed (the baseline was rewritten). Now ≥ 50 cents (whistles 93–119): 240 → 0.82, 27 waves up,
none down.

### D4o. `shaker` family (PhISEM) and hit-time rounding (task 7.5)

Tambourine 435, stream 404 and bubble 62 carried `transient_off`: the originals are made of many tiny collisions,
the models were smooth. `gmgen/src/fam/shaker.rs` implements Cook's PhISEM: Bernoulli collision events at `rate` ×
envelope^`follow`, each a noise burst (`sdecay`) ringing ≤ 3 band-pass resonances, with per-event level spread
(`gjit`), detune (`fjit`) and an upward sweep (`chirp`, water drops). Sustain renders one loop-length event pattern
with wrapped rings (exactly periodic). The generator's event count is tested (rate within 10 %), as are the resonance
peaks (10 %) and loop periodicity.

`gmfit/src/shaker.rs` measures collisions on a 0.5 ms envelope (0.25 ms frames counted a burst's own fluctuations:
2.5× the true rate), ≤ 3 resonances (peak-picking with each band-pass response removed), burst decay and level
spread. Candidates: rate ×0.5/1/2, chirp 0/+1 oct, detune 0/300 cents, Q ×1/0.25 (jingles click broadband rather
than ring), alone or added to the current model with 30/50/70 % of the energy (the model's layers give it up; levelling
to the residual fails for noise: independent noises never cancel). `analyze` keeps the best by rank.

| Wave | Before | After |
|---|---|---|
| maracas 230 | 0.64 `lines` | 0.66 shaker, no flags |
| tambourine 435 | 0.56 `transient_off` (modal) | 0.61 modal + shaker, no flags |
| applause 26 | 0.47 | 0.48 shaker |
| stream 404 | 0.38 `env_off` `transient_off` | 0.44 shaped noise + shaker, no flags |
| bubble 62 | 0.14 `transient_off` | 0.30 shaker (transient share 0.64 vs 0.65) |
| guiro 228 | 0.40 | 0.32 shaker (fewer flags) |

Drums pass 4 (whole drum category + applause/stream/bubble/seashore with shaker, noise loop, rounding fix): 26 of 95
improved, no rank regressions; the shaker is in 17 waves (490 0.17 → 0.55, 202 0.05 → 0.23, 445 0.21 → 0.38, 195
0.17 → 0.32, stream 404 → 0.48); crash 106 0.56 → 0.66 and open hat 272 0.51 → 0.65 (noise loops refitted).
Report: A 6, B 23, C 466, mean 0.161.

Hit-time rounding: measured hit times are exact sample positions; `modal` floored `t·rate`, so the file's 5-digit
rounding moved a hit one sample (0.4 cycle at 8 kHz) and cost up to 0.07 (tambourine 435 0.636 in memory → 0.562 on
disk; whistle 229 0.48 → 0.39). Now nearest sample: 229 holds 0.48 on disk. `bulk` gained `GMFIT_BULK_DUMP=<dir>`
(every pipeline result, kept or not).

### D4p. Hearing-model difference (`gmfit hear`)

Listening named differences the frame score and the descriptors average away. `gmfit/src/hear.rs` asks what a
listener hears of the difference, with the ear model of PEAQ (ITU-R BS.1387) and the MPEG psychoacoustic models:
outer/middle-ear weighting (Terhardt), half-Bark bands, frequency spreading (27 dB/Bark down, 24 + 230/f − 0.2·L up),
threshold in quiet, forward masking (8–30 ms), Zwicker specific loudness — compared in the excitation domain, since
two independent noises never match sample by sample. Both sounds play at bank speed (band-limited FFT resample; linear
interpolation mirrored content above the new band edge).

- `added` / `missing`: loudness of the copy above / below the original by more than 3 dB + 1.4 × the original's own
  frame-to-frame fluctuation (robust median over 100 ms), as a share of its loudness; located in bands that hold
  sound themselves (a loss spreads its excitation far upward at high level).
- `flutter`/`moddiff` (PEAQ ModDiff on loudness^0.3) and `swell` (2–20 Hz share of the band envelopes).
- Frame-aligned only over the first 150 ms (when the attack happens is heard); after it each band compares level
  *distributions* (sorted levels paired by rank, tolerance 3 dB + 0.5 × fluctuation): a texture is heard by its
  statistics, not by when each grain falls (McDermott & Simoncelli 2011). Frame-aligned, a reseeded shaker differed
  from itself by up to 0.9 of the loudness (bubble 62: 0.35–0.45); by distribution, reseeds of every committed wave
  stay under the floor (`examples/seedfloor.rs`) except the bird-as-shaker, whose loud events hang on its seed.

Use: rank = score − 0.1 × character flags − 0.5 × audible excess (added + missing − 0.05, moddiff − 0.02, each ≥ 0);
the fit objective (`score_of`) carries the same excess term. The excess is 0 on all seven approved pilot waves and on
59 of the 100 fitted waves, so it only steers what a listener would hear as different (+1–31 ms per evaluation).

Calibration: a same-texture independent noise (Gaussian shaped by the original's smoothed STFT, level-matched) gives
added + missing ≤ 0.047 and moddiff ≤ 0.017 on the 13 named drums → thresholds 0.05 / 0.02. Tests: identical = 0,
same-colour noises below the floor, a removed 8 kHz tone found as missing at 6.5–10.5 kHz, resample pitch/level.
Findings on the named waves (frame-aligned version, before the distribution change): bird 50 misses its loud chirp moments (45 %, 3.2–3.8 kHz at bank speed: steady where
the original bursts), laughing 226 misses 53 % at 7–8.4 kHz in 450–720 ms, bubble 62 sits ~15 % low (missing
1.7–2.0 kHz, extra 1.5–1.7 kHz), stream 404 misses its top and is too loud at 0.55–0.65 kHz, belltree 49 has extra
6–7.6 kHz in its first 200 ms, brush 58 and crash 106 lack the original's slow swell, 272/315 flutter more;
cymbal 84 and tambourine 435 show nothing above the noise floor.

### D4q. `fm` family and the tweet train (task 7.6)

`gmgen/src/fam/fm.rs`: 2–4 sine operators (`ops` rows [ratio, amount, index decay, index sustain]; a carrier's
amount is dB, a modulator's its peak index), stack or two pairs, feedback on the top operator, a start `glide`, and
a repeating pitch/loudness cycle (`cyc` [cents, dB] × ≤ 32 at `cyc_hz`). Sustain renders one loop and tiles it:
cycle rate, base frequency (total phase per loop) and ratios snap to whole cycles, so the loop is exact, feedback
included (one warm-up loop). Tests: 1:1 and 1:3.5 sidebands at |fc ± k·fm| within 1 dB of the signed, folded Bessel
levels (bins on the 220 Hz grid: a half-bin Hann loss of 1.4 dB first read as a 2 dB error), the cycle's plateaus at
440/880 Hz, loop exactness with and without feedback.

The bird (wave 50) is one pure partial: a steady 7.5 → 7.85 kHz tone for 66 ms, then tweets every 34 ms, each an
8.8 → 5.2 kHz sweep in 14 ms and a quiet gap. `gmfit/src/fm.rs` tracks the loop as an analytic signal (FFT band
around the dominant partial, loop tiled ×3), finds the cycles per loop from the repeating level track, averages the
cycles into 32 `cyc` points on a sine carrier and makes the head an attack tone with its glide. Bird 50: 0.136
(pass 4) → 0.505, no flags, nothing heard missing (hearing excess 0.008). Operator presets (DX7-style EP 1:1 + 1:14,
STK TubeBell 1:1.414 pairs, a 1:3.5 pair) take indices, decays and sustains from a scored coordinate descent (the
fitter tunes scalars only): EP2 145 0.43 (placeholder 0.011), tubular bell 464 0.21 (0.057) — above the
placeholders, but additive analysis wins both (0.83, 0.57) and is kept by rank.

### D4r. `formant` family, the laugh and two fit fixes (task 7.3)

`gmgen/src/fam/formant.rs`: a band-limited saw or pulse source with aspiration, jitter and vibrato through ≤ 5
parallel formant band-passes; a syllable table [s, dB, cents] gives the voiced source its own envelopes and a pitch
fall (`syl_glide`), and `breath_db` is noise under the layer envelope. Sustain is per harmonic (source spectrum ×
the formants' complex response) on the loop grid: exact. Tests: formant peaks within 10 % (whole and sustain),
syllables at their times, loop exactness with vibrato.

Laughing (226) is four voiced "ha" syllables at 0.20/0.33/0.46/0.63 s, f0 ≈ 850 Hz, each starting at once with a pitch
arc, after 0.2 s of breathy noise. At that pitch LPC locks onto harmonics (a "formant" at 1830 Hz was harmonic 2), so
high voices take formants from harmonic amplitudes; syllables must be periodic (the opening breath is not); the
breath envelope is fitted to the floor between syllables. Laugh 226: hearing excess 0.54 → 0.10 (missing 53 % → 4 %),
`transient_off` cleared, score 0.19. Choir check: formant beats the placeholders (Choir Aahs 85 0.16 vs 0.08, Voice
Oohs 312 0.18 vs 0.05); additive wins both and is kept (85 → 0.50, 312 → 0.60). The rerun also lifted bubble 62
(additive + noise + shaker: missing 24 % → 4 %).

Two fit bugs surfaced once the hearing penalty made objectives negative: `score_of` returned 0 for a NaN render and
`par_scores` 0 for invalid parameters, so a broken candidate beat every real one ("after 0.0" in bulk logs, a
near-silent laugh). Both now return −1.

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
