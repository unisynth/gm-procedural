# crates

Rust workspace members. `dls` and `gmgen` are `no_std` (consumed by unisynth in 64k as git dependencies); `gmfit` is a host tool. Nightly toolchain (`rust-toolchain.toml`).

| Crate | Purpose |
|---|---|
| `dls/` | no_std zero-copy DLS1/2 reader for OS GM bank; feature `write` = DLS1 writer; stereo `stpr` pairs. Bounds-checked, never panics → see `dls/AGENTS.md` |
| `gmfit/` | Host tool fitting procedural bank vs `gm.dls` (`GM_DLS`) or gmplus SF2 (`--set plus`): extract/bank/analyze/fit/correct/refine/bulk/render/hear/report/dump/plus-map, scoring, hearing model → see `gmfit/AGENTS.md` |
| `gmgen/` | no_std+alloc procedural GM bank (`build`, `render_wave`, `bank::*`); features `std`/`plus`; deterministic math (`clippy.toml`); families in `src/fam/` → see `gmgen/AGENTS.md` |
| `gmgen-wasm/` | `gmgen` for the browser (cdylib, wasm32): C ABI `begin`/`step`/`finish`/`out_ptr`/`release`; byte-identical to `Packed::build(None)` → see `gmgen-wasm/AGENTS.md` |
