# crates/gmgen-wasm

`gmgen` for the browser: cdylib (+rlib), wasm32, plain C ABI. Deps `gmgen` (feature `plus`) + `dls` (feature `write`). Built with `--profile fast --target wasm32-unknown-unknown`, copied to `site/assets/gmgen.wasm` by `tools/site/build.ts`. Timing: wasm gm 1.3 s, plus 11.3 s under Bun.

| File | Purpose |
|---|---|
| `Cargo.toml` | Package `gmgen-wasm`. `crate-type = ["cdylib","rlib"]`. Deps `gmgen` (`plus`), `dls` (`write`). |
| `src/` | C-ABI exports → see `src/AGENTS.md` |
