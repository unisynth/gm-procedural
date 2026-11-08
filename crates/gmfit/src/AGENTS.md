# crates/gmfit/src

`gmfit` lib + `main.rs` CLI. Original waves from `GM_DLS`; sets `gm` / `plus` (`lib.rs`). Env debug: `GMFIT_TRACE` (stderr traces), `GMFIT_THREADS` (pool size), per-stage `GMFIT_*_DUMP`. Long files keep detail in `<file>.AGENTS.md` sidecars (pull-only).

| File | Purpose |
|---|---|
| `analyze.rs` | `gmfit analyze`: initial params from original + priors (loop-grid pitch, harmonic table, envelope, attack layers, modulation, gap/noise loops); keeps best candidate by rank. → see `analyze.rs.AGENTS.md` |
| `audio.rs` | `original_wave`, `loop_of`, `played` (attack + loop unrolled as a sampler holds a note), `write_wav`. |
| `cmaes.rs` | Seeded full-covariance CMA-ES over unit box [0,1]^n; `Cma::{new,ask,tell}`, `minimize`; cyclic Jacobi eigendecomposition (n ≤ ~60); deterministic per seed. Tests: sphere, Rosenbrock, same seed, Jacobi. |
| `desc.rs` | Character descriptors (D4n), reported + flagged, not scored: `contrast`/`contrast_pair`/`contrast_diff` (fine spectral contrast, `WASH_DB` 20), `percussive_share`, `peaks_per_frame`. See change: add-procedural-gm-bank D4n. |
| `dsp.rs` | Host-only target conditioning (add-gmplus-bank D2): Kaiser sinc `resample`/`resample_at` (`TAPS` 64), `dc_block`, `Biquad`, `run_varying`, SF `sf_lowpass`(`_clamped`), `cents_hz`. |
| `fit.rs` | `fit` (CMA-ES + harmonic refinement), `correct`/`correct_tf`/`correct_env`/`correct_all`, `refine`, `pipeline`, `loop_reset`, `score_of`, `par_scores` pool. → see `fit.rs.AGENTS.md` |
| `fm.rs` | `fm` family candidates (D4q, 7.6): `chirp_cycle` (analytic-signal track, cycles/loop, 32-point [cents, dB]), `chirp_candidates` (train + head attack tone), `preset_candidates` (EP / TubeBell / 1:3.5, scored index grid); `analyze` keeps by rank (`GMFIT_FM_DUMP`). |
| `formant.rs` | `formant` family candidates (7.3, D4r): `lpc_envelope`, `formants` (≤ 5 peaks, −3 dB bw), `harmonic_formants` (f0 > 250 Hz), `syllables` (voiced 5 ms peaks), `candidates` (laugh: syllable table, pitch fall, breath; choir: sustain on wave pitch; 3-pass formant/breath relevel). |
| `hear.rs` | PEAQ/MPEG ear-model hearing difference (D4p): `excitation`, `hear`/`listen` → `Heard`, `roughness`, `ring_diff`, `at_speed`. → see `hear.rs.AGENTS.md` |
| `ks.rs` | `ks` plucked-string candidates (7.3, D4r): `t60` (0.8–1.25·f0 band decay), `candidates` (whole or attack + loop hand-over with sustain relevel; joint damp×dyn grid then coordinate search); `GMFIT_KS_DUMP`/`GMFIT_KS_TRACE`. |
| `lib.rs` | Shared helpers: `Set`, `Orig`, `users`, `voice`, `play_speed`, `load_wave`, `evaluate`, `tier`, `rank`, `regressions`, `keep_if_better`, `stereo_check`. → see `lib.rs.AGENTS.md` |
| `main.rs` | `gmfit` CLI: extract, bank, placeholder, render, hear, sweep, analyze, fit, correct, refine, bulk, partners, stereo, dump, plus-map, report. → see `main.rs.AGENTS.md` |
| `modal.rs` | Struck/rattle/chirp character analysis → `modal` candidates (D4l, D4m): `onsets`, `partials`, `glide`, `sparse_loop`, `tonal_share`, `candidates`. → see `modal.rs.AGENTS.md` |
| `plan.rs` | Per-thread cached FFT planner: `forward(n)`, `inverse(n)`. Fresh `FftPlanner` per call fragmented the heap with 16 fit threads (one bulk run reached 5.4 GB). See change: add-procedural-gm-bank D4o. |
| `plus.rs` | gmplus targets: `Targets::{load,wave,write_map,structure,stereo}`, `Target`, `GM_WAVES` 495, `RATE` 32000. Needs `GMPLUS_SF2`. → see `plus.rs.AGENTS.md` |
| `priors.rs` | GM program/drum key → `Prior{kind, families, inharm, vib, scoop, odd}` (`for_program`, `for_drum_key`) + ratio/frequency/formant tables (MARIMBA, TUBULAR_BELL, MEMBRANE, HAT_808, HAMMOND, VOWEL_A…); `family()` = first family implemented in `gmgen::schema::FAMILIES`. Test: priors cover all programs/kit keys. |
| `score.rs` | Similarity score D6 on played signals: `score`, `Score`, flags, `describe`, `buzz`, `periodicity`, `a_weight`, `mel_spec`. → see `score.rs.AGENTS.md` |
| `sf2.rs` | Host SF2 2.04 reader: `Sf2::{parse,open,preset,zones,slice,sample_data}`, `Resolved::{with_mods,cents,bounds,looped,root}`, generator ids `gn::*`; inst gens absolute, preset gens added, ranges match both levels. `tests::Builder`/`fixture` build SF2 bytes. See change: add-gmplus-bank D2. |
| `shaker.rs` | PhISEM `shaker` candidates (D4o): `collisions`, `resonances`, `candidates`, `bands`/`band_rings`, `ring_groups`, `ring_layers`, `ring_combos`. → see `shaker.rs.AGENTS.md` |
| `sheet.rs` | PNG contact sheets: `pair` (orig/gen waveform + log-mel 512/64 bands −80…0 dB + diff, red = gen louder, blue = quieter), `rows`, `bank` grid, `draw_pair`, `Img`. |
| `subtractive.rs` | `subtractive` candidates (7.4): synth leads/pads/basses as 2 oscillators + resonant filter; `profile_fit`, `candidates` (waveform × cutoff grid, then res, osc 2 detuned/±1 oct, filter envelope; level per step); looped = one sustain layer, unlooped = one whole layer. |
| `wavelet.rs` | Morlet analysis (D4m): `band` (amp + inst. freq), `trill` (FM vs AM; whistle pea), `decay`, `band_onsets` (strikes in resonance band). Modal analysis uses trill + band strikes; score `d_mod`, flag `mod_off`. |
