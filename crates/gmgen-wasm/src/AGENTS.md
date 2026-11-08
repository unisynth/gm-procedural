# crates/gmgen-wasm/src

| File | Purpose |
|---|---|
| `lib.rs` | C ABI, wave-by-wave build so the page shows progress: `begin(set)` (0 gm, 1 plus) → wave count; `step(i)` renders wave i (unparametrised waves stay silent, as in `Packed::build`); `finish()` → DLS length, bytes at `out_ptr()`; `release()` frees output. State in thread_locals `JOB`/`OUT`. Byte-identical to `Packed::build(None)` (test `matches_native_build`, gm set). |
