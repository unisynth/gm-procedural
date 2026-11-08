# crates/dls/examples

Runnable examples of the `dls` crate.

| File | Purpose |
|---|---|
| `dump.rs` | `cargo run -p dls --example dump -- <bank.dls>`. TSV listing: header `# path: N instruments, M waves`; `ins` lines (drums, bank_msb, program, name, region count, articulation count, `keylo-keyhi:wave` list); `wave` lines (rate, bits, len, unity, fine, loop; `UNREADABLE` if `wave(i)` is None). Structural fingerprint for bank diffing (macOS gs_instruments.dls vs Windows gm.dls): survives 8-bit vs 16-bit requantization. |
