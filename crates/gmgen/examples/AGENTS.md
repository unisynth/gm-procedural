# crates/gmgen/examples

Runnable examples for `gmgen`.

| File | Purpose |
|---|---|
| `bench_sets.rs` | Generation time of full bank: gm set as committed (`bank::build(None)`, ≈ 6.8 s native) and plus skeleton (32 kHz, plus lengths) rendered with gm parameters as stand-in (≈ 54 s native). Prints ms, bytes, samples, sample ratio. Run `cargo run --release -p gmgen --features plus --example bench_sets`. See change: add-gmplus-bank 2.3. |
