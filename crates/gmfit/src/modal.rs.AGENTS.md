# modal.rs

Character analysis for struck, rattling and chirping sounds (design D4l), feeding the `modal` family. See change: add-procedural-gm-bank D4l, 7.2, D4m.

- `onsets` (strike times + levels from 2 ms RMS → `hits`, levels re-fitted); `partials` (strongest prominent peaks of one hit: start level + decay, single-frequency tracker over Hann frames); `glide` (strongest partial: start offset cents + time constant); `sparse_loop` (loop-DFT bins holding 99.5 % of energy with phases; cymbal/belltree loops); `tonal_share` (energy in 8 strongest peaks / total); `strikes`, `rattle`.
- Wavelet (D4m, `wavelet.rs`): trill of strongest partial (FM → `trill_hz`/`trill_cents`, sidebands dropped) and strikes inside its band → `hits`.
- Private: `struck`, `relevel`, `relevel_hits`, `set_click`, `add_residual`.
- `candidates(i, orig, meta)` → whole-wave `modal` param sets; `analyze` keeps the best scoring model (`GMFIT_MODAL_DUMP`). Env `GMFIT_TRACE`.
- Tests: partials freq/level/decay, glide of falling tone, onsets per strike, strikes of rattle loop, sparse_loop prominent bins, periodicity separates tone from noise.
