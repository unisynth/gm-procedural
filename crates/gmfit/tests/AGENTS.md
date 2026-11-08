# crates/gmfit/tests

Integration tests. Tests needing the originals skip when `GM_DLS` / `GMPLUS_SF2` are unset. Unit tests live in `src/*.rs` (`cargo test --workspace --release`).

| File | Purpose |
|---|---|
| `fit.rs` | Analysis/fit/correct against the original bank (skips without `GM_DLS`): `organ_analysis_scores_high` (organ 150 analyze ≥ 0.9), `fit_is_reproducible`, `worse_result_is_not_written` (no write on regression). See change: add-procedural-gm-bank. |
| `plus.rs` | gmplus targets vs gm.dls (needs `GM_DLS` + `GMPLUS_SF2`): `piano_targets_keep_the_gmplus_pitch` (vs naive gmplus render, < 1 cent), `map_holds_no_audio`, `plus_structure_follows_the_map`. See change: add-gmplus-bank 1.4. |
| `report.rs` | `report --check` regression rule: `lowered_wave_is_named`, `new_waves_and_improvements_pass` (`gmfit::regressions`). See change: add-procedural-gm-bank. |
| `structure.rs` | `assets/gm/structure.json`: `no_pcm_committed` (structure only, never audio), `committed_parses`, `round_trip_against_gm_dls` (lossless vs real bank; staleness). See change: add-procedural-gm-bank. |
