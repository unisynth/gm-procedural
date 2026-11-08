# analyze.rs

`gmfit analyze`: initial parameters from the original wave and the GM priors (`prior_for_wave`). `analyze`/`analyze_alts` run the prior model, then keep the best by rank among candidates from `modal`, `shaker` (alone, model + shaker, ring combo), `fm`, `ks`, `subtractive`, `formant` families.

- Pitch from loop grid: `m = round(L·f0/rate)` periods, pitch exactly `m·rate/L`, harmonic `k` on loop bin `k·m` (generator snaps partials to the same grid).
- Looped sustain: `additive` layer, harmonic table = cluster-energy loop DFT at bins `k·m` + unison/detune/inharm (stiff-string B from peak positions when prior inharmonic)/phases; periodic `noise` layer for energy between harmonics when > −25 dB; attack envelope from 2 ms RMS vs loop RMS; per-layer RMS calibration (`calibrate`, renders layer alone).
- Attack/whole: `drum`/`metal`/`noise` percussive attack layers from the prior (`attack_layer`: pitch glide, decay, spectral centroid, 8 log bands; drum broadband noise, 0.5 ms attack).
- `string_attack`: stiff-string attack layer (B, `pdecay` from early/late spectra, sustain `dly` hand-over) for struck/plucked strings; `spread` for wide loop clusters; stretched loop clusters. `HANDOVER_MAX_S` 0.3: plus set skips the hand-over when loop starts later (mid-note break on 240/293); gm set unchanged. See change: add-procedural-gm-bank 6.2c; add-gmplus-bank.
- `loop_vibrato`/`loop_am` (6.2c); `loop_modulation`: per-harmonic complex envelopes → `am`/`vib` tables (h1 pitch). See change: add-procedural-gm-bank 6.2d.
- `side` rows for h1–12 (99.5 % cluster energy, ≤ 11 bins) within room after `am` rows. See change: add-procedural-gm-bank 6.2e.
- `pick_phase_seed` (also after `fit::correct_all`). See change: add-procedural-gm-bank 6.2f.
- `gap_noise`: periodic noise sustain from mid-gap floor; also at end of `fit::correct_all`; band gains by per-band periodicity (aperiodic share = original). See change: add-procedural-gm-bank 6.2g, 6.2j.
- `noise_loop`/`pick_noise_loop`: third-octave noise sustain for wash loops (also in `correct_all`), `bands_n`. See change: add-procedural-gm-bank D4n.
- Candidate selection: modal vs prior model (`GMFIT_MODAL_DUMP`); shaker alone, model + shaker (30/50/70 % energy) or ring combo by rank (`GMFIT_SHAKER_DUMP`, `GMFIT_SHAKER_ALL`); fm by rank (`GMFIT_FM_DUMP`). See change: add-procedural-gm-bank D4l, D4o, D4q, 7.6.
- Env: `GMFIT_TRACE`, `GMFIT_DUMP=<file>` writes best candidate even when not kept (listening).
- Exports for other modules: `avg_power`, `bands`, `bands_n`, `rms`, `calibrate`, `open_for_fit`.
