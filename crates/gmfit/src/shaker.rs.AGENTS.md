# shaker.rs

Shaker analysis (design D4o): PhISEM candidates for sounds made of many tiny collisions (tambourine jingles, maracas, applause, stream, bubbles). Measures the loop when looped, else the whole wave. See change: add-procedural-gm-bank D4o.

- `collisions`: 0.5 ms envelope onsets (rate, burst decay, level spread); count approximate (dense events merge).
- `resonances`: ≤ 3 peaks by peak-picking the smoothed spectrum (Q from −3 dB width, each peak's band-pass response removed before the next); envelope = one-shot attack + decay time, loop steady.
- `candidates`: rate ×0.5/1/2 × chirp 0/+1 octave (bubbles rise) × detune 0/300 cents × resonance Q ×1/0.25 (jingles click broadband) + droplets; caller keeps best by rank.
- `bands`/`band_rings`/`BandRing`: per third-octave ring index = envelope-power autocorrelation width × bandwidth; `NOISE_INDEX` 0.5, `RING_INDEX` 0.75 (rings ≥ 0.75; τ); `ringing`. `ring_groups` (DP on ln τ), `ring_layers` (one shaker per group, Q = π f τ), `ring_combos` (current hiss bed + ring layers, 50/80 %). `analyze` keeps shaker alone, model + shaker (30/50/70 % energy) or ring combo by rank. Env `GMFIT_TRACE`.
- Tests: recovers rate and resonance; ring index tells droplets from hiss; separates ring layers.
