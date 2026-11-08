# crates/gmgen/src/correct

Submodule of `../correct.rs` (module file and this directory coexist).

| File | Purpose |
|---|---|
| `fft.rs` | Deterministic FFT, any length, `no_std`: `fft(re, im, inverse)`. Radix-2 iterative for powers of two; Bluestein (chirp-z via power-of-two convolution) otherwise, chirp index k² mod 2L in integers (long loops keep exact twiddles). Twiddles from `math::sin_cycles`; IEEE + − × ÷ only. Replaced O(L²) loop DFT (95 % of bank generation time, D9). |
