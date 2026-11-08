# Tasks

## 1. Target abstraction (design D1)

- [ ] 1.1 Add `Target` and `TargetId` in `crates/gmfit` with `Target::gm(i)`. Port `analyze`, `fit`, `correct*`,
  `refine`, `gap_noise`, `pick_phase_seed`, `calibrate`, `score_of`, `evaluate`, `sweep` and `main` to `&Target`.
- [ ] 1.2 Verify GM results are unchanged: `gmfit report --check` reports 0 diffs; every committed
  `assets/gm/waves/*.json` re-renders bit-identically (test); the gmfit test suite passes.

## 2. WAV I/O and parameter files (D2)

- [ ] 2.1 Write the WAV reader: RIFF `fmt`/`data`/`smpl`, PCM 8/16/24/32 and float, stereo → mono, leading-silence
  trim, optional windowed-sinc resample. Tests: each format, `smpl` loop, the spec's rejection cases (not WAV,
  compressed, empty, > 30 s).
- [ ] 2.2 Write the parameter file: JSON with a `meta` block (params_json extension) and the binary entry (`i32 -1`
  tag, version, meta, seed, params) in `gmgen` (no_std, no alloc on decode). Tests: JSON ↔ entry ↔ JSON round
  trip with bit-identical render, a 513-number refusal, every truncation refused without panic.
- [ ] 2.3 Implement `Target::wav(path, opts)`: meta from the file, seed = FNV-1a of the params bytes at write
  time, prior from the classifier or `--kind`.

## 3. Loop detection (D3)

- [ ] 3.1 Write `loopfind`: the `smpl` loop; the pitched search over periods × vibrato cycles; the unpitched
  band-spectrum segment; the played-unroll ≥ 0.9 acceptance; the one-shot fallback for decaying sounds. Add
  `--loop` override.
- [ ] 3.2 Test on synthetic signals (vibrato tone, plain tone, noise pad, decaying drum) and on the GM bank:
  detection on every looped GM wave's PCM gives a loop whose unroll scores within 0.01 of the true loop's. Report
  failures in a table.

## 4. Classifier (D4)

- [ ] 4.1 Write `classify`: the measurements (pitch strength, harmonicity/B, inharmonic ratios, sustain/decay,
  onset share/centroid, glide), the rules → `Prior`, and printed measurements.
- [ ] 4.2 Calibrate on all 495 GM waves: write a confusion table vs the GM priors to `build/gm/classify.md`. The
  pilot six must match (spec scenario). Record the thresholds in the doc.

## 5. WAV commands

- [ ] 5.1 Add `gmfit wav analyze|fit|correct|refine|render|make` on parameter files, with keep-if-better against
  the input file's score. `make` writes params, the gen WAV, the sheet and the diagnose report, and prints the
  score and failing checks. `--progress` emits JSON lines per step.
- [ ] 5.2 Test: GM wave 405 exported as a looped WAV goes through `make` and lands within 0.02 of the committed GM
  score. Run the same check on the other pilot waves and record the results.

## 6. Diagnose (D5)

- [ ] 6.1 Write the `diagnose` module: each check of the D5 table with tolerance and hint, comb-picked spacing,
  "n/a" for unpitched targets; JSON + Markdown output; the difference-map PNG.
- [ ] 6.2 Write fault tests: clean synthetic tone vs in-phase upper harmonics, no gap floor, added rumble, and
  vibrato removed. Each flags only its own check, and the buzz hint names `phase_seed`.
- [ ] 6.3 Add the `gmfit diagnose <wave|orig.wav gen.wav>` command; a `gmfit report` column of failing checks per
  wave; failing checks on the pilot A/B page.

## 7. Procedural sample entries in the synth (D6, D7)

- [ ] 7.1 Doubt-review the entry format and the caller-render decision (`doubt-driven-review`) before it stands.
- [ ] 7.2 `unisynth` chunk: parse the procedural entry arm under `us-sample-gen` (structural checks, budget);
  without the feature, `UnsupportedModule`. Tests: bad entries, truncation at every byte, hostile sizes, every
  `project/music` chunk byte-identical after re-encode.
- [ ] 7.3 Sampler: read caller-supplied samples per procedural entry in place (loop and pitch as GSM); silence when
  not supplied; pool size unchanged. Tests: identical output to a test hook playing the same samples, and the
  missing-samples scenario.
- [ ] 7.4 Callers: `crates/song` / player and `synth-wasm` render procedural entries with `gmgen` before load and
  pass them in. Test: native vs wasm entry samples bit-identical.
- [ ] 7.5 Measure size and time (`performance-optimization`): packed player cost of `us-sample-gen` (i686, squishy
  + xz) and load-time render of 10 × 2 s entries native/wasm. Record both in the doc and the size report.

## 8. DAW and server (D8)

- [ ] 8.1 `packages/cli`: `POST /api/procedural` + `GET /api/procedural/<id>`: token/Origin, 16 MB, RIFF check,
  temp dir, `Bun.spawn` argv with a fixed binary path, one job, 10 min limit, cleanup in `finally`, 503 without
  the binary. Tests: progress/result, hostile uploads (413/400/403, no temp file left), timeout.
- [ ] 8.2 Security pass on the route (`security-hardening`): upload validation, spawn, temp files,
  denial of service.
- [ ] 8.3 DAW sampler slot: "Make procedural…" (file pick → resample 44.1 kHz mono → post → progress with step
  scores → store the entry); the slot shows GSM/procedural, size, score and failing checks; the original/generated
  A/B in the dialog (original not stored). Library marks entries GSM or procedural. Tests per the spec scenarios.

## 9. Docs and skill (D9)

- [ ] 9.1 Write `docs/synth/wav-resynthesis.md`: the analysis order and stage → parameter table, limits, diagnose
  checks and hints, entry format, classifier thresholds, size and time figures. Update `docs/synth/unisynth.md`
  (sampler entries) and `docs/synth/daw.md` (Make procedural). Update the AGENTS rows of the touched directories.
- [ ] 9.2 Add the project skill `procedural-from-wav`: make → diagnose → act on hints → re-run; when a missing
  family is the cause.
- [ ] 9.3 Final checks: gmgen, gmfit, unisynth and cli tests; wasm and i686 builds; `gmfit report --check` with
  0 regressions; `openspec validate --strict`.
