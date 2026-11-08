# crates/gmgen

Procedural GM bank generator (change add-procedural-gm-bank): rebuilds the Windows `gm.dls` layout from `assets/gm/` parameters. `no_std` + `alloc` (features `std`, `plus` add host helpers / the gmplus set). Design: `docs/synth/gm-procedural.md`.

Determinism rule (design D5): bank bytes identical on native and wasm32. `clippy.toml` bans f32 transcendentals (sin/cos/exp/ln/log/pow/sqrt/floor/round/rem_euclid…) and `mul_add`; use `math`. A change that alters bank bytes must say so (check `gmfit bank` hash, `gmfit report --check`).

Features: `std` = `structure`, `params_json` (`WaveFile` JSON), `pack` (dep `serde_json`). `plus` = `build.rs` also packs `assets/gm-plus/` (`plus_*` blobs); crate-level `build`/`render_wave`/`wave_count` then = plus set; `bank::GM` stays gm.dls. See change: add-gmplus-bank 2.2–2.4.

| File | Purpose |
|---|---|
| `Cargo.toml` | Deps `dls` (feature `write`), optional `serde_json` (`preserve_order`). Features `std`, `plus`. Build-deps `dls`, `serde_json`. |
| `build.rs` | Packs `assets/gm/` via `pack.rs` (`#[path]` includes params/params_json/schema/structure/pack) into `OUT_DIR` `skeleton.dls`/`lens.bin`/`params.bin`. Invalid file = build error naming wave. Missing wave file = `cargo:warning`, silent wave. Feature `plus`: also `assets/gm-plus/` → `plus_*` blobs. See change: add-gmplus-bank. |
| `clippy.toml` | `disallowed-methods`: f32 sin/cos/tan/tanh/exp/exp2/ln/log2/log10/powf/powi/sqrt/mul_add/sin_cos/floor/round/rem_euclid. Determinism (D5). |
| `examples/` | Bench example → see `examples/AGENTS.md` |
| `src/` | Generator library: bank, render, families, corrections, schema/params, tests → see `src/AGENTS.md` |
