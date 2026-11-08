# fit.rs

`gmfit fit` and `correct`. Each keeps a result only when it scores higher than its start.

- `fit`: full-covariance seeded CMA-ES (`cmaes.rs`) over explicit scalars + gain, then harmonic refinement (`refine_harmonics`, `refine_bands`). Never tunes `unison`/`dly`/`spread`/sustain `vib_rate`/delayed-layer envelope (fixed from analyze). `keep_if_better` (lib) writes + updates scores.json.
- `score_of`/`par_scores`: NaN or invalid = −1 (below every real objective once the hearing penalty makes them negative). `par_scores` runs a persistent worker pool (`GMFIT_THREADS` overrides one-per-core; avoids fresh threads each CMA-ES generation). See change: add-procedural-gm-bank D4o, D4r.
- `correct`: analytic harmonic correction within budget (cluster-energy harmonic table) → `correct_tf` (4 analytic passes over attack band energies, time × band `tf`) → `correct_env` (fine envelope; one-shot end: last point measured on last 5 ms, settled after best pass; See change: add-procedural-gm-bank 6.2i). `correct_all` = harmonic → tf → env, also `gap_noise`, `pick_noise_loop`, `pick_phase_seed`.
- `refine`: alternates fit ↔ `correct_all` (`--rounds`, `--iters`).
- `pipeline` (bulk): `analyze_alts` → fit → `correct_all` → `loop_reset` → `refine`; the closest runner-up model gets the same treatment, best wins. `loop_reset` after correct: looped waves try additive-sustain `inharm` + `pdecay` = 0 and refine from it too (plus 240: 0.583 → 0.680). Tests `correct_fixes_a_mistimbred_model`, `loop_reset_escapes_the_inharm_pdecay_trap`.
- Env `GMFIT_TRACE`, `GMFIT_THREADS`. See change: add-procedural-gm-bank D4b, D4c, 6.2c, 6.2i; add-gmplus-bank 3.2–3.3.
