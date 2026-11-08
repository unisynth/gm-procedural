# crates/dls

`no_std` zero-copy DLS1/2 reader for the OS GM bank (Win `drivers\gm.dls`, macOS `gs_instruments.dls`). `Dls::parse(&[u8])` → `instruments()`/`find(drums,bank,prog)`, `Instrument::regions()`/`region_for(key,vel)`/`articulation()` (`Connection` art1/art2), `wave(i)` via `ptbl` (`Wave::sample` 8u/16s → f32), `sampler(&Region)` = wave + effective `Wsmp` (region overrides wave). Bounds-checked: malformed → `Err`/`None`, never panic. Parses offsets (never WaveSabre's hard-coded 0x44602). Consumed by 64k unisynth and `gmgen`. Tests: `cargo test -p dls --release` (add `--features write` for the writer tests).

| File | Purpose |
|---|---|
| `Cargo.toml` | Package `dls`, no deps. Feature `write` (empty, needs `alloc`) gates `dls::write`; default build stays core only, allocation-free. See change: add-procedural-gm-bank. |
| `examples/` | `dump` example: TSV bank fingerprint → see `examples/AGENTS.md` |
| `src/` | Reader, optional writer, tests → see `src/AGENTS.md` |
