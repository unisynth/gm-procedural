# Procedural GM bank: pilot (change add-procedural-gm-bank, task group 6)

The 6 pilot waves were fitted with `gmfit analyze --force` → `fit --iters 60…120` (→ `correct`). The A/B page is
`build/gm/pilot/index.html`. It holds the original vs generated wave, played as a held note (attack + loop to 1 s),
and the contact sheet.

## Scores (after the calibration changes below)

| wave | instrument | families | score | d_spec | d_env | d_loop | flags | listening (user) |
|---|---|---|---|---|---|---|---|---|
| 376 | Standard kit kick (key 36), unlooped | drum | 0.672 | 0.81 | 1.98 | – | | pending |
| 161 | Standard kit snare (key 38), unlooped | drum + noise | 0.446 | 5.76 | 1.97 | – | env_off | pending |
| 83 | Closed hi-hat (key 42), unlooped | metal + noise | 0.575 | 5.69 | 0.48 | – | | pending |
| 240 | Fingered Bass (prog 33), loop 89 | additive (sustain) | 0.956 | 0.25 | 0.15 | 0.00 | | pending |
| 405 | Strings (prog 48), loop 9710 | additive (unison 2) + noise | 0.605 | 2.45 | 1.44 | 0.71 | pitch_off | pending |
| 293 | Piano 1, wave 293, loop 1339 | additive (sustain) | 0.740 | 2.83 | 0.35 | 0.08 | pitch_off | pending |

Outside the pilot, measured while building the analysis: Organ 1 wave 150 scores 0.994 from `analyze` alone, and
Organ wave 151 scores 0.93.

## What the pilot changed (each found by a failing measurement)

1. **Envelope frames ≥ 2 periods.** 2 ms RMS frames are shorter than one period of a low note. `d_env` then
   tracked waveform phase, and a timbre-perfect organ got `d_env` 2.6. The fix uses windows of at least
   2 periods and 10 ms, and the smallest STFT follows the same rule.
2. **Phases of the first 32 harmonics from the loop DFT**, so the waveform shape matches as well as the spectrum.
3. **Harmonic cluster energy.** Detuned and ensemble voices split each harmonic into side peaks. Using the single
   bin `k·m` lost them: "Detuned Organ" scored 0.01. The fix uses the cluster energy within ±m/2 bins, and
   two strong peaks give `unison` 2 plus `detune` (score 0.41 → 0.47 after fit).
4. **`d_loop` per harmonic cluster** (third-octave bands when unpitched), not per bin. A 9710-sample ensemble loop
   compared bin by bin punished the dense cloud between the voices.
5. **Audibility floor.** `d_spec` compares only down to 50 dB below the peak, and `d_loop` down to 60 dB, so faint
   −70 dB noise-floor differences no longer dominate.
6. **Noise spectrogram smoothing.** Two independent noises of the same colour scored about 5 dB apart. Power is
   now smoothed over 5 frames and mel bands are at least 3 bins wide. A test enforces `d_spec` < 1.5 for such
   noises.
7. **Drum analysis.**
   - Body pitch from zero crossings of the low-passed signal. Autocorrelation locked onto the overtone: 106 Hz
     instead of ~50 Hz falling.
   - Decay from a dB regression after the first 5 ms, so the onset click doesn't set it.
   - All drum, metal and noise scalars are opened for the fit.
8. **Sampled hats and cymbals are broadband.** The `metal` layer alone lacked the low-mid content, so the
   analysis adds a noise layer shaped by the measured spectrum.
9. **Noise band refinement after CMA-ES** (band gains from the average spectrum; the noise analogue of the
   harmonic-table refinement).
10. **Full-covariance CMA-ES.** The diagonal variant stalled on Rosenbrock, and level, decay and tilt are coupled.
11. **Soft knee above 0.9 in the generator.** Fitted waves with the right spectrum but a peakier waveform clipped.

## Round 2: correction tables (after listening feedback "not good")

| wave | round 1 | harmonic + `tf` |
|---|---|---|
| 376 kick | 0.672 | 0.744 |
| 161 snare | 0.446 | 0.696 |
| 83 hat | 0.575 | 0.660 |
| 240 Fingered Bass | 0.956 | 0.985 |
| 405 Strings | 0.605 | 0.679 |
| 293 Piano | 0.740 | 0.839 |

- **`tf` time × band table** (20 bands × ≤ 9 rows, design D4c). It shapes the attack over time, which the static
  harmonic table cannot.
- **Harmonic table on cluster energy.** On a single bin, piano and string partials beside k·m looked missing
  (−24 dB everywhere).
- **What still limits the drums:** `d_env` around 1.5, from envelope detail between the `tf` rows.

## Round 3: fine envelope, refine, string and ensemble models

| wave | round 2 | round 3 | what helped |
|---|---|---|---|
| 376 kick | 0.744 | 0.883 | `env` (fine onset envelope) |
| 161 snare | 0.696 | 0.763 | `env` |
| 83 hat | 0.660 | 0.780 | `env` + `refine` (+0.008) |
| 240 Fingered Bass | 0.985 | 0.985 | – |
| 405 Strings | 0.679 | 0.712 | `spread` (ensemble) + `refine` |
| 293 Piano | 0.839 | 0.849 | stiff-string model 0.840: not committed, A/B alternative |

- **Piano 293, measured.** B = 3.3·10⁻⁴ (h10 +27 cents, h14 +55 cents). Over 0.3 s, h1 drops 4 dB, h3 12 dB and
  h5 20 dB. The stiff-string layer models this (plain 0.76 against 0.74) but loses 0.009 after the corrections,
  partly to budget: 3 layers use 104 numbers, so the tables get fewer rows. The hammer-noise layer added nothing
  (0.7601 against 0.7602).
- **Strings 405, measured.** Every harmonic is a cluster of about 10 peaks within 10 dB, sd ≈ 12 cents, about
  constant in cents. With `spread`, `d_loop` goes 0.14 → 0.05. The remaining `d_spec`/`d_env` is the shimmer
  pattern (random component phases), a noise-like ceiling.
- **`refine` (fit ↔ correct) barely helps.** The scalar fit can't leave the optimum the tables were fitted to.

## Round 4: listening "376, 405, 293-alt end differ"

These are the three the score had rated well, so the score was wrong. Measured with time slices of RMS, centroid,
band shares and zero-crossing pitch:

| wave | what differed | cause | fix |
|---|---|---|---|
| 376 kick | 0–5 ms centroid 2.1 kHz vs 0.3 kHz; band > 4 kHz 20–35 dB low through the decay | no click/texture layer, and the score was blind to it: the 50 Hz body set the floor, frame 0 started at sample 0 (Hann ≈ 0 over the click), and the click was averaged over 0.5 s | A-weighting, centred frames, `d_onset`; broadband noise layer for drums |
| 405 strings | loudness sd 0.2 dB vs 1.22 | the loop is a 2.27 Hz vibrato (one cycle per loop, ±12 cents) with locked loudness wobble; analysis said "12-cent ensemble" and 97 periods instead of 98; the fit flattened any random shimmer | loop vibrato analysis + `vib_phase`, `am_db`/`am_phase`; loop `d_env` by smoothed shape + wobble depth |
| 293-alt end | loop band > 4 kHz 6 dB low | the loop's partials are stretched too; the clusters at k·m lost them above h14 | clusters centred on the stretched partial; the string layer keeps its level |

Scores on the new scale: kick 0.70, snare 0.68, hat 0.65, Bass 0.89, Strings 0.67, Piano 0.75 (stiff-string, now
committed). Organ 1's analysis drops to 0.88: `d_onset` sees its key click (2–3 kHz, first 10 ms), which is real and
not modelled.

## Round 5: "405 needs to improve"

Per-harmonic complex envelopes of the loop:

| harmonic | loudness sd (dB) | residual after 2 / 4 / 8 Fourier terms |
|---|---|---|
| 1 | 1.29 | 0.79 / 0.29 / 0.18 |
| 2 | 5.99 | 3.29 / 3.12 / 2.32 |
| 3 | 3.94 | 1.29 / 1.21 / 0.58 |
| 5 | 4.63 | 1.97 / 1.52 / 1.42 |
| 10 | 6.20 | 6.09 / 4.30 / 3.49 |
| 20 | 5.33 | 4.98 / 4.82 / 4.06 |

- **Low harmonics** (most of the power) follow the vibrato with their own depths and phases (all phases 0 or ½:
  symmetric about the loop start). A fixed body resonance did not explain them better than sinusoids.
- **Above h8** the fluctuation is irregular, like several players beating. Random spread or random modulation
  only made it worse (it cancels the locked part), so it stays unmodelled.
- **Result:** the `am` (4 terms × 12 harmonics) and `vib` (6 terms) tables. Score 0.674 → 0.788, loudness sd 1.21
  against 1.20 dB, pitch sd 11.1 against 11.9 cents. The cluster widths are still about 2/3 of the original's.

## Round 6: "retry" (budget 512)

A harmonic's complex envelope over the loop *is* its cluster of loop-DFT bins. The 5 strongest bins hold 88–100 %
of h1–h5 and 76–92 % of h6–h8. Storing those as `side` rows reproduces each harmonic's AM, FM and their interaction
exactly.

| variant | analysis | full pipeline | d_spec | d_env |
|---|---|---|---|---|
| round 5 (`am` 4 terms + `vib`) | 0.765 | 0.788 | 1.14 | 0.12 |
| `side` 97 %, ≤ 7 bins, h1–12 | 0.754 | 0.772 | 0.93 | 0.53 |
| `side` h1–17 (≤ 5 above h8) | 0.749 | – | – | – |
| `side` 99.5 %, ≤ 11 bins, h1–7 + `am` h8–12 | 0.792 | **0.815** | 0.82 | 0.27 |

- The small components shape the loudness dips. With them, the low-harmonic cluster widths reach 11/12, 12/15
  and 15/17 cents (original/round 5: 12/8, 15/8, 17/7).
- Pitch sd 10.5 against 11.9 cents, and loudness sd 1.13 against 1.20 dB.

## Round 7: "buzz in the centre of the spectrum"

Upper-band peak/mean power within a period (0.5 ms frames):

| band | 1.5–2.5 | 2.5–4 | 4–7 | 7–10 kHz |
|---|---|---|---|---|
| original | 2.48 | 2.96 | 2.83 | 3.00 |
| round 6 (Schroeder phases) | 2.80 | 5.55 | 5.48 | 5.68 |

The levels matched within 0.3 dB, and the score stayed at 0.815 for every phase variant. The scorer averages
over frames, so it is blind to the per-period pulse.

| phases for h8+ | 2.5–10 kHz buzz | score |
|---|---|---|
| Schroeder | 4.96 | 0.8154 |
| measured | 2.24 | 0.8153 |
| random | 2.4–2.9 | 0.815–0.816 |

Fixes: `phase_seed` (1 number instead of a 49-entry phase table), seed chosen per band, and a `buzz` flag in the
scorer.

## Open findings for the bulk fit

- **Organ key click** (2–3 kHz, about 10 ms): a click `noise` attack layer for organ priors.
- **Hat 83 is lowest (0.65).** `d_spec`/`d_onset` around 2.8.

- **Unpitched sampled percussion** (snare, hat) stays near 0.45–0.6. Remaining error: the high band above about
  9 kHz (the analysis bands stop at 0.45·rate) and the temporal-spectral evolution, which a single envelope per
  layer cannot follow. Candidates: a band reaching Nyquist, and a per-band decay in `noise`.
- **Strings ensemble.** A regular beating pattern from 2 voices. `unison` now allows up to 8 voices with
  pseudo-random jitter, but the analysis still picks 2.
- **Piano attack.** It needs `tilt` (brightness decay) and a hammer-noise attack layer. `modal` (task 7.2) is the
  intended model.
- **Score weights** (`exp(−(d_spec/12 + d_env/6 + d_loop/12))`) and the tiers (A ≥ 0.95, B ≥ 0.85) are still to be
  calibrated against the user's listening ratings (task 6.2).
