# crates/gmfit

Host tool fitting the procedural GM bank (`crates/gmgen`) against Windows `gm.dls` (`GM_DLS`, never committed); `--set plus` fits gmplus SF2 targets (`GMPLUS_SF2`). Binary `gmfit` (`src/main.rs`) + lib. Deps: `dls` (write), `gmgen` (`std`, `plus`), `serde_json`, `rustfft`, `png`. Hot-loop tuning env: `GMFIT_THREADS`, `GMFIT_TRACE`. See change: add-procedural-gm-bank, add-gmplus-bank.

Commands (every one takes `--set gm|plus`, default gm; exit 0 ok, 2 flags/regression, 1 error; detail → `src/main.rs.AGENTS.md`):
`extract` structure.json · `bank [--orig]` gen.dls · `placeholder` · `analyze` / `fit` / `correct` / `refine` / `bulk` fit pipeline · `render` / `hear` / `sweep` / `dump` A/B and listening · `report [--check] [--baseline]` scores.json regression · `plus-map` / `partners` / `stereo` gmplus.

| File | Purpose |
|---|---|
| `Cargo.toml` | Package `gmfit`; deps `dls` (feature `write`), `gmgen` (`std`, `plus`), `serde_json` (`preserve_order`), `rustfft` 6, `png` 0.17. Depends on gmgen `plus`, so gm set calls `gmgen::bank::*`. |
| `examples/` | `seedfloor` hearing-model seed-floor check → see `examples/AGENTS.md` |
| `src/` | Library modules (analysis, fit, score, hearing, gmplus targets) + `main.rs` CLI → see `src/AGENTS.md` |
| `tests/` | Integration tests (structure, report rule, fit, gmplus; most need `GM_DLS` / `GMPLUS_SF2`) → see `tests/AGENTS.md` |
