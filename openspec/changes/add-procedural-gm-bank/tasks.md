# Tasks

Tests that need the original bank read it from `GM_DLS` (local copy `~/Downloads/64k-tools/gmdls/gm.dls`). They
skip when `GM_DLS` is unset.

## 1. DLS writer

- [x] 1.1 Add the `write` feature to `crates/dls` (`alloc`). It provides a `Bank` description (instruments, regions,
  articulation, `wsmp`, 16-bit waves) and `write(&Bank) -> Vec<u8>`.
  - Tests first: a synthetic bank round-trips through `Dls::parse` (instruments, regions, `wsmp`, articulation,
    samples).
  - Verify: `cargo test -p dls --features write` passes.
- [x] 1.2 Add a real-bank round trip: `gm.dls` → `Bank` → bytes → `Dls::parse`. The fingerprint dump must equal the
  original's and every sample must be equal.
  - Verify: `GM_DLS=… cargo test -p dls --features write` passes.
  - Verify: `cargo check -p dls --target i686-pc-windows-gnu` still builds without `write`.
- [x] 1.3 Add a `write` row to `crates/AGENTS.md` (`dls/`) and a writer requirement note to `docs/` if one exists for
  `dls`. Verify the rows are present.

## 2. Bank structure extraction

- [x] 2.1 Create the `crates/gmfit` binary crate (std; deps `dls[write]`, `serde_json`; `rustfft`/`png` added with group 4) with the
  `gmfit extract` command. It writes `assets/gm/structure.json`: instruments, regions, articulation, `wsmp`, and
  per-wave rate/length/loop. It holds no samples.
  - Verify: running it with `GM_DLS` produces the file.
  - Verify: the test "No PCM committed" passes (no DLS magic, no sample arrays, file under 512 KB; measured 330 KB).
- [x] 2.2 Record in `assets/gm/NOTICE.md` (no root NOTICE exists) that `assets/gm/` holds structure and bounded synthesis parameters fitted against
  `gm.dls`, and no sample data. Verify by review.
- [x] 2.3 Add `crates/AGENTS.md` rows (`gmfit/`, `gmgen/`), plus a `assets/gm/AGENTS.md` row. Verify they are
  present.

## 3. Generator core (`crates/gmgen`)

- [x] 3.1 Scaffold `crates/gmgen` (`no_std` + `alloc`, depends on `dls[write]`). Add deterministic math (`sin`, `exp`,
  `pow2`, `db`), an xorshift RNG, and a `clippy::disallowed_methods` config banning `f32` transcendental functions and
  `mul_add`.
  - Tests: accuracy bounds of the approximations.
  - Verify: `cargo test -p gmgen` passes and `cargo clippy -p gmgen` is clean.
- [x] 3.2 Add the `build.rs` that packs `structure.json` and `waves/*.json` into a binary table. It validates the
  family names, the 4-layer / 512-number budget, ranges and missing waves, and errors name the wave.
  - Tests: a fixture directory with an over-budget wave, an unknown family and a missing wave each fails with the
    expected message.
- [x] 3.3 Add the bank builder `build(mask) -> Vec<u8>` with the layer regions `attack` / `sustain` / `whole` (D3),
  periodic loop rendering, gain, f32 → i16 round-half-even, and the masked-out waves as silence of the same length.
  - Start with the `additive`, `noise` (periodic and transient), `drum` and `metal` families. The pilot needs `metal`
    for the hi-hat.
  - Tests: wrap-step bound, determinism (built twice gives identical bytes), mask equality with the full build,
    sanity (finite, peak, no clipping run).
  - Tests: the structure fingerprint equals the original's (with `GM_DLS`).
- [x] 3.4 Add the optional harmonic correction stage (D4b): exact DFT gain on the loop, 129-tap linear-phase FIR on
  the attack, and an empty table that leaves the output byte-identical.
  - Tests: the spec scenarios "Correction fixes loop timbre" and "No table".
- [x] 3.5 Commit placeholder parameter files for all 495 waves so the bank builds end to end: a single `additive`
  layer from the loop length and pitch only. Verify: `cargo test -p gmgen` passes with every wave present.
- [x] 3.6 Document the generator (families, parameters, regions, correction) in `docs/synth/gm-procedural.md` and add
  the `crates/gmgen/AGENTS.md` rows. Verify the doc covers every family listed in design D4.

## 4. Scoring, render and report

- [x] 4.1 Implement the score in `gmfit` (D6): the "played" unroll of the loop, multi-resolution log-mel `d_spec`,
  `d_env`, `d_loop`, and the flags.
  - Tests: an original scored against itself gives 1.0; silence scores below 0.1; a 10-cent detune sets `pitch_off`;
    a forced wrap step sets `loop_click`.
- [x] 4.2 Add `gmfit render <wave>`, which writes `orig.wav`, `gen.wav`, `sheet.png` (waveforms, both spectrograms,
  difference) and the JSON report with the score with and without correction. Exit codes 0 / 2, and non-zero without
  `GM_DLS`.
  - Verify: `gmfit render 289` produces all four artefacts, and the PNG opens.
- [x] 4.3 Add `gmfit sweep <wave> <param> <values>`: one contact-sheet row per variant, no files changed.
  - Verify: a 5-value sweep reports 5 rows and `git status` is clean.
- [x] 4.4 Add `gmfit report [--check]`: scores every wave, writes `build/gm/report.md` grouped by GM family with
  tiers, and the bank-wide sheet. `--check` fails on a drop of more than 0.005 against `assets/gm/scores.json`.
  - Test: an artificially lowered wave makes `--check` exit non-zero, naming it.

## 5. Analysis and fitting

- [x] 5.1 Add the analysis priors table `crates/gmfit/src/priors.rs` from `research/gm-procedural-instruments.md`:
  mode-ratio sets, 808 hat and cowbell oscillators, SATB vowel formants, Hammond footages, piano B by register,
  vibrato and scoop defaults. Map each GM program and drum key to its families and priors.
  - Test: every GM program 0–127 and every drum key used by the 9 kits maps to at least one family.
- [x] 5.2 Add `gmfit analyze <wave>`, which writes the initial parameters: f0, a harmonic table from the loop DFT,
  envelope breakpoints, noise bands, drum pitch sweep, and families and priors from 5.1.
  - Verify: the analysed `additive` init for a pure organ wave scores at least 0.9.
- [x] 5.3 Implement seeded CMA-ES (full covariance plus rank-μ; the diagonal variant stalled on Rosenbrock) over normalised parameter ranges.
  - Tests: it converges on Rosenbrock and a sphere; the same seed gives identical results.
- [x] 5.4 Add `gmfit fit <wave> [--seed --iters]`, which writes only on improvement and updates `scores.json`. Add
  `gmfit correct <wave>`, the analytic correction (D4b) truncated to the remaining budget.
  - Tests: no write on regression; `correct` raises the score of a deliberately mistimbred `ks` fixture.

## 6. Pilot and calibration (checkpoint with user)

- [x] 6.1 Fit the pilot set: Standard kit kick (key 36), snare (38), closed hat (42), Fingered Bass (program 33),
  Strings (program 48), and Piano 1 wave 293. Record the scores, the sheets and an A/B listening note per wave in
  `research/gm-procedural-pilot.md`. Verify the file lists all 6 with scores.
- [x] 6.2 Calibrate the score weights and tier thresholds against listening. Re-baseline `scores.json`. Show the
  pilot A/B to the user and get confirmation before the bulk fit. Verify the user approved and the weights are noted
  in the research file.
- [x] 6.2a Pilot listening feedback ("not good"): add the time × band correction `tf` (D4c) in `gmgen/src/tf.rs`
  (schema grid, params budget/validate/binary/JSON, render after the harmonic correction). `gmfit correct` now fits
  harmonic (cluster energy) + `tf`. Tests: FFT round trip; flat table = identity; band gain lands; loop untouched.
- [x] 6.2b Second feedback round: fine envelope table `env` (D4d, `tf.rs`), `gmfit refine` (fit ↔ correct),
  `dly` envelope scalar, stiff-string attack analysis (`string_attack`), ensemble `spread` on `additive` with
  analysis detection. Tests: `env_gain_lands`, `spread_loops_exactly_and_widens_harmonics`. Pilot: kick 0.88,
  snare 0.76, hat 0.78, Strings 0.71, Piano 0.85, Bass 0.985.
- [x] 6.2c Third feedback round ("376, 405, 293-alt end differ"):
  - Score: A-weighting, centred frames, `d_onset`, loop-texture `d_env`; `scores.json` re-baselined.
  - Analysis: drum broadband noise, loop vibrato + locked AM (`vib_phase`, `am_db`, `am_phase`), stretched loop
    clusters, string layer kept at full level; structural scalars kept out of the fit.
  - Pilot (new score): kick 0.70, snare 0.68, hat 0.65, Bass 0.89, Strings 0.67, Piano 0.75 (stiff-string, now
    committed).
- [x] 6.2d Fourth feedback round ("405 needs to improve"): `additive` tables `am` (per-harmonic loudness curve,
  1×–4× vibrato rate) and `vib` (pitch curve Fourier series 1×–6×), analysis `loop_modulation` from per-harmonic
  complex envelopes. Strings 405: 0.674 → 0.788.
- [x] 6.2e Fifth feedback round ("retry", budget may grow): `MAX_NUMBERS` 256 → 512; `additive` table `side`
  (sideband components per harmonic on the loop grid); analysis fills harmonics 1–12 to 99.5 % energy within
  the room left after the `am` rows. Strings 405: 0.788 → 0.815.
- [x] 6.2f Sixth feedback round (buzz in the mid spectrum): `additive` `phase_seed` (hashed random phases for
  untabled harmonics), seed picked by per-band `score::buzz_band`, scorer flag `buzz`. Strings 405 is flag-free.
- [x] 6.2g Seventh feedback round (pattern in the difference map): `gap_noise` periodic noise sustain layer from
  the mid-gap floor (comb-picked spacing, band/harmonic refinement on the render, hp 0.7·f0, lp by score),
  also re-run at the end of `correct_all`. Strings 405: mid-gap floor matched, score 0.8157 → 0.8122.
- [x] 6.2h Refit the other pilot waves with the current pipeline (keep if better): hat 0.651 → 0.718, Piano 293
  0.753 → 0.757; kick, snare and bass unchanged. No `buzz` flags; gap floors match (bass loop = 1 period).
- [x] 6.2i One-shot env end (D4j): `tf::env_db_end` (last point past a one-shot's end sits at its end),
  `correct_env` measures it on the last 5 ms and settles it. Test `one_shot_last_point_sits_at_the_end`. Kick
  0.700 → 0.709 (end matched), snare 0.677 → 0.741.
- [x] 6.2j Eighth feedback round ("405 has shhhsss noise", D4k): `gap_noise` band gains calibrated by per-band
  periodicity (orig 3–6 kHz 0.80; round 8 0.56 → 0.81); skirt experiment rejected; `measure.py` periodicity
  check; `GMFIT_DUMP` writes the best gap-noise candidate. Strings 405 0.8122 → 0.7974 (accepted on listening).
- [x] 6.3 Write the `.pi/skills/gmfit/SKILL.md` skill (render → read sheet → change layers → fit → correct → A/B →
  commit; pitfalls from the pilot). Verify a fresh agent can fit one new wave by following only the skill.
  Done: `.pi/skills/gmfit/SKILL.md` + `measure.py` (the checks the score misses, with thresholds, n/a per wave
  type). A fresh agent fit Organ 150 0.0101 → 0.9335 from the skill alone; its 12 gaps were fixed in the skill.

## 7. Remaining model families

- [ ] 7.1 Add the `ks` family: extended Karplus-Strong with pick-position comb, dynamics low-pass, all-pass tuning,
  dispersion, commuted body resonances and optional bridge buzz.
  - Tests: pitch accuracy within 2 cents, decay time within 10 %, and a pick position at ½ removes the even
    harmonics.
  - Check it on 3 guitar/bass waves plus the sitar. Verify all 4 score above their placeholder score.
  Done: `gmgen/src/fam/ks.rs` (tests in-file), analysis `gmfit/src/ks.rs`. Analysis-stage objective of the `ks`
  candidate vs placeholder: Fingered Bass 241 0.52 vs 0.02 (beats the additive path, 0.49), Sitar 368 0.28 vs 0.004
  (beats 0.27), Clean Gt. 95 0.15 vs 0.009, Picked Bass 287 −0.15 vs 0.03 (too bright/loud on a dark 286 ms attack;
  the additive path wins there). Bulk 8.2 decides per wave.
- [x] 7.2 Add the `modal` family (modes × ratio/amplitude/decay, strike noise), with tests for mode frequencies and
  decays. Check it on 3 mallet and bell waves. Verify all 3 score above their placeholder score.
  Done as design D4l: absolute-Hz modes, glide, hit train / measured `hits` table, band-passed click, periodic
  sustain (sparse loop bins or folded train); character analysis `gmfit/src/modal.rs`; `d_per` score term.
  Bell/metal checks: agogo 25 0.03 → 0.69, triangle 446 0.00 → 0.50, belltree 49 0.00 → 0.26.
- [x] 7.2a Wavelet analysis (design D4m): `gmfit/src/wavelet.rs` (Morlet band, trill, decay, band onsets),
  `modal` trill, band strikes, score `d_mod`/`mod_off`, `measure.py` flutter/pitch-movement/strike checks.
- [x] 7.2b Character descriptors and rank (design D4n): `gmfit/src/desc.rs` (fine contrast, percussive share, peaks
  per frame), flags `lines`/`transient_off`/`peaks_off`, rank = score − 0.1 per flag in `keep_if_better` and
  `report --check`, `analyze::noise_loop` (third-octave noise sustain for wash loops; `noise` bands ≤ 24),
  `play_speed` for listening pages, `measure.py` descriptor rows. Cymbal 84 and brush 58 moved to noise loops.
- [x] 7.5a Hearing-model difference (design D4p): `gmfit/src/hear.rs` (PEAQ/MPEG ear model, excitation-domain
  added/missing with a noise-texture tolerance, flutter/swell; level distributions after a 150 ms aligned attack),
  `gmfit hear <w> [-p f]` at bank speed; rank and fit objective − 0.5 × audible excess. Named drums: belltree 49 extra
  0.11 → 0.03, stream 404 missing 0.15 → 0.09, brush 58 0.70 → 0.71, vibraslap 476 hears close; bird 50 and laughing
  226 need the FM / formant families (7.6, 7.3), bubble 62 its 1.9 kHz ring.
- [x] 7.3 Add the `formant` family (pulse/saw source, ≤ 5 formants), with tests for formant peak positions. Check it
  on 2 choir/voice waves. Verify both score above their placeholder score.
- [ ] 7.4 Add the `subtractive` family (2 oscillators, resonant filter with envelope), with tests for cutoff response.
  Check it on 3 synth lead/pad/bass waves. Verify all 3 score above their placeholder score.
- [x] 7.5 Add the `shaker` family (PhISEM collision events into ≤ 3 resonances), with tests that the event rate
  and resonance peaks fall within 10 % of the set values. Check it on maracas, tambourine and applause. Verify all 3
  score above their placeholder score. (Design D4o: maracas 230 0.66, tambourine 435 0.61, applause 26 0.48, stream
  404 0.44, bubble 62 0.30; `gmfit/src/shaker.rs` analysis; modal hit times to the nearest sample.)
- [x] 7.6 Add the `fm` family (2–4 operators, index envelopes, feedback, stack or parallel), with tests that the
  sideband positions for a 1:1 and a 1:3.5 pair match theory. Check it on EP2, tubular bells and the bird tweet.
  Verify all 3 score above their placeholder score.
- [ ] 7.7 Update `docs/synth/gm-procedural.md` and the AGENTS rows for the new families. Verify every family has a
  parameter table.

## 8. Bulk fit by GM family

Each task: fit every wave of the group, commit the parameters and scores, and make `gmfit report --check` pass. The
report lists the group with no wave in tier C, or each tier-C wave carries a note in the pilot research file
explaining the limit.

- [ ] 8.1 Drum kits (91 waves). First pass done with `gmfit bulk --cat drums` (analyze → fit → correct_all →
  refine; failed waves logged, not fatal): 4 A, 13 B, 74 C, no placeholders left. ~30 of the 91 are SFX-kit waves
  (Bird, Applause…). Kit percussion on keys 54 and 67–84 fell back to `noise`: it needs `modal` (7.2) and `shaker`
  (7.5). Re-run after those.
- [ ] 8.2 Bass (30) and guitar (48)
  First pass (`bulk --cat bass`, `--cat guitar`, with `ks`): bass 26/29 written (0, 1, 240 kept), guitar 57/58
  (2 kept); `ks` attacks win 23 (9 bass, 14 guitar). Report: A 6 → 10, B 25 → 56, placeholders 388 → 306, mean
  0.169 → 0.263, no regressions. Tier C left (< 0.5): bass 173, 286, 375, 373, 418, 411; guitar 97, 273, 279, 204,
  401, 276, 124, 203, 127, 96, 211 — need listening and notes.
- [ ] 8.3 Organ (28), pipe (27) and reed (46)
- [ ] 8.4 Brass (37), strings (32) and ensemble (15)
- [ ] 8.5 Synth lead (25), pad (25) and synth FX (17)
- [ ] 8.6 Piano (36) and chromatic percussion (19)
- [ ] 8.7 Ethnic (31), percussive (9), SFX (10) and any remaining waves. Verify that no placeholder parameters remain
  (`gmfit report` shows 0 waves with the placeholder flag).

## 9. Runtime integration

- [ ] 9.1 Add `gmgen::policy` (feature `std`): `Choice::{Auto, Proc, Os, Path}`; on Windows `Auto` uses the OS
  `gm.dls` when it parses and falls back otherwise.
  - Tests: the policy with an injected path and platform covers the 4 spec scenarios.
- [ ] 9.2 Add `song render|check --bank proc|os|<path>`. The default is unchanged (`GM_DLS`, otherwise none).
  - Verify: `song render <a GM song> --bank proc -o x.wav` renders a non-silent GM track.
  - Verify: the existing parity tests still pass.
- [ ] 9.3 Add the synth-wasm `gm_build(mask)` export and a Bun test that the wasm bank hash equals the native hash.
  - Verify: `bun test` for the hash test passes.
- [ ] 9.4 Measure generation time (native release full bank ≤ 300 ms, wasm ≤ 1 s on the reference machine) and
  optimise until it is within budget (rotators, culling, single-period loop copy). Record the numbers in
  `docs/synth/gm-procedural.md`. Verify with a timing test or bench output.
- [ ] 9.5 DAW: generate the bank in the bounce Worker and send it to the worklet through the `"bank"` message. Add a
  bank-source selector (Procedural default, OS bank when `/api/bank` or `data/bank.dls` exists), persisted in
  `localStorage`, with `?bank=proc|os`. Bounce and library follow the selection. Add the inspector's platform note.
  - Tests first: DAW controller and browser tests for the `synth-daw` delta scenarios.
  - Verify: `bun test` in `packages/daw` passes.
- [ ] 9.6 Update `packages/AGENTS.md`, `crates/synth-wasm/AGENTS.md`, `crates/song` docs and `README` (bank
  selection). Verify the rows mention `gm_build` and `--bank proc|os`.

## 10. Integration checks

- [ ] 10.1 With `GM_DLS` set: `cargo test --workspace`, `bun test`, and `gmfit report --check` all pass. The final
  `build/gm/report.md` tier counts are posted for the user.
- [ ] 10.2 Without `GM_DLS`: `cargo test --workspace` and `bun test` pass, with the bank-dependent tests skipped and
  the determinism and sanity tests run.
- [ ] 10.3 A/B listen in the DAW on 3 songs from `project/music` that use GM instruments (OS bank vs Procedural), and
  record the findings in `research/gm-procedural-pilot.md`.

## Workflow follow-up

- Run the code-review and code-quality gates before commit.
- Archive the change after the user accepts the bulk-fit report and the A/B listening.
