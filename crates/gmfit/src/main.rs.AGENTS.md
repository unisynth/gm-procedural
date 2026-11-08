# main.rs

`gmfit <command>` CLI. Every command takes `--set gm|plus` (default gm): `plus` reads/writes `assets/gm-plus` and `build/gm-plus` and fits the gmplus soundfont targets (`GMPLUS_SF2`) instead of the gm.dls waves. Exit: 0 ok, 2 flags raised (`render`) / regression (`report --check`), 1 error. Needs `GM_DLS`. `report(check, baseline)` helper at file end.

## Commands

- `extract [-o]` → `assets/gm/structure.json` (no audio). `--set plus extract` = `Targets::structure` (32 kHz, target loops, fine + `fine_delta`, gain + `wsmp_gain` in 1/655360 dB, regions alike). See change: add-gmplus-bank 2.1–2.4.
- `bank [--orig] [-o]` → `build/gm/gen.dls` (`gmgen::build(None)`); `--orig` = GM_DLS re-written by `dls::write`, same layout, for `tools/gm-songs`. `--set plus bank` writes `gmgen::plus::build` to `build/gm-plus/gen.dls`.
- `placeholder` → missing `waves/NNN.json`.
- `render <w> [-p f]` → `build/gm/NNN/{orig,gen}.wav` (played 1 s), `sheet.png`, `report.json` (score, components, flags, `score_without_correction`, `tier`, `play` = `play_speed` key/speed/instrument, plus `gmplus` row on plus). Exit 2 on flags. Plus sheet adds the gm.dls wave as 3rd row.
- `hear <w> [-p f]` → hearing-model difference at bank playback speed (`hear::listen`), JSON on stdout. See change: add-procedural-gm-bank D4p.
- `sweep <w> <path> <v,..>` → one scored, rendered variant per value + `sweep.png`; paths `gain_db`, `L.scalar`, `L.table.i`, `correction.i`; files untouched.
- `analyze <w> [--force] [-o]` → initial params from original + priors (placeholders only unless `--force`); see `analyze.rs.AGENTS.md`.
- `fit <w> [--iters --seed --vars]` → `fit::fit` (CMA-ES + harmonic refinement); `keep_if_better` writes + updates scores.json.
- `correct <w>` → analytic harmonic → tf → env correction (`fit::correct_all`) within budget, kept if better.
- `refine <w> [--rounds 3 --iters 80 --seed 1]` → alternates fit ↔ `correct_all`.
- `bulk (--cat c | w…) [--iters 160 --rounds 2 --jobs 4 --partners]` → `fit::pipeline` per wave from the original, waves in parallel, written only if rank beats committed; panics logged (`{"error":"panic"}`), batch continues. Stereo partners skipped unless `--partners` (use `partners`). Env `GMFIT_BULK_DUMP=<dir>` keeps every pipeline result written or not. See change: add-procedural-gm-bank D4l, 7.2, D4o.
- `report [--check] [--baseline]` (threads) → `build/gm/report.md` by GM category + tiers, `build/gm/bank.png`; `--baseline` writes `assets/gm/scores.json`; `--check` exit 2 on rank drop > 0.005 (`regressions`).
- `dump <dir> [--secs 2]` → every wave of the set `NNN-{orig,gen}.wav` (played, loop repeated) + `index.json` (score, tier, flags, placeholder, users, gmplus `source`), all threads; feeds `tools/gm-songs` waves.html. See change: add-gmplus-bank 2.x.
- `plus-map` → `assets/gm-plus/map.json` (`Targets::write_map`, needs `GMPLUS_SF2`); exits 1 on error rows.
- `partners` (plus) → each stereo partner = left wave's params + `fit::correct_all` vs right target, written as pair decision (not by own rank); skips pairs with target_corr < 0.5 (fit with `bulk --partners`); prints stereo check row. See change: add-gmplus-bank D4, 4.2–4.3.
- `stereo` (plus) → `stereo_check` of every pair (left vs mid score, loop correlation).
