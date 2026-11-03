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

## Open findings for the bulk fit

- **Unpitched sampled percussion** (snare, hat) stays near 0.45–0.6. Remaining error: the high band above about
  9 kHz (the analysis bands stop at 0.45·rate) and the temporal-spectral evolution, which a single envelope per
  layer cannot follow. Candidates: a band reaching Nyquist, and a per-band decay in `noise`.
- **Strings ensemble.** A regular beating pattern from 2 voices. `unison` now allows up to 8 voices with
  pseudo-random jitter, but the analysis still picks 2.
- **Piano attack.** It needs `tilt` (brightness decay) and a hammer-noise attack layer. `modal` (task 7.2) is the
  intended model.
- **Score weights** (`exp(−(d_spec/12 + d_env/6 + d_loop/12))`) and the tiers (A ≥ 0.95, B ≥ 0.85) are still to be
  calibrated against the user's listening ratings (task 6.2).
