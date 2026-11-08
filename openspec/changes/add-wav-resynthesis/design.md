# Design

## Context

- **The pipeline is already general.** `gmgen` (no_std + alloc, deterministic, native = wasm) renders a wave from
  `WaveMeta` + `Params` + seed. `gmfit` analyzes, fits, corrects and scores (host only: `rustfft`, threads).
  Analysis is tied to GM only through the wave index `i`, which supplies `gmgen::bank::meta(i)` (rate, length,
  loop, unity), `prior_for_wave(i)` (GM category → kind and families), the render seed, and the original samples.
- **Session findings.** The add-procedural-gm-bank rounds (design D4a–D4i, `research/gm-procedural-pilot.md`) showed
  that listening problems hid in measurements the score averages away. These are diagnose's checks.
- **No heap in the instrument.** `unisynth` is no_std with no heap and caller-owned memory. GSM entries decode into
  the caller's pool. The GM bank (procedural or OS) is produced by the caller (`crates/song`, `synth-wasm`, the
  player) and passed in as bytes the instrument reads in place.
- **Chunk layout.** A GSM sample-section entry starts with an `i32` compressed size, and negative sizes are
  rejected (`crates/gsm`). The sampler source parameter is a 3-way choice (0.0 / 0.5 / 1.0), so a 4th value
  would change what stored values mean.
- **DAW server.** The DAW talks to a local Bun server (`packages/cli`) with token, Origin and confined-path rules.
  The browser already imports WAVs (resample to 44.1 kHz mono → GSM).

## Goals / Non-Goals

**Goals:**
- One analysis code path for GM waves and WAVs; GM results byte-identical.
- An automatic classifier good enough to pick the families; `--kind` stays as an override.
- Procedural entries play in DAW, bounce and player exactly alike.
- Diagnose turns this session's ad-hoc measurements into tested checks with fix hints.

**Non-Goals:**
- **New model families.** `modal`, `ks`, `fm` and `formant` belong to add-procedural-gm-bank's open tasks. WAVs
  that need them get the best fit the existing families allow, and diagnose says so.
- **Multi-note, chord or phrase WAVs,** stereo synthesis, and sounds longer than 30 s.
- **Analysis inside the browser (wasm).** It is minutes of multi-threaded CPU with `rustfft`, and the local server
  is always present when the DAW edits.
- **Automatic re-analysis.** A stored entry is never re-analyzed when the tool improves. The user re-runs "Make
  procedural…".

## Decisions

### D1. `Target` replaces the wave index in `gmfit`
`struct Target { orig: Vec<f32>, meta: WaveMeta, prior: Prior, seed: u32, id: TargetId }`, where
`TargetId::{Gm(i), Wav(path)}`.
- `render(target, params)` = `gmgen::render(&meta, params, seed)`.
- `Target::gm(i)` builds exactly today's values (seed = `i`, prior = `prior_for_wave(i)`).
- `analyze`, `fit`, `correct*`, `refine`, `gap_noise`, `pick_phase_seed`, `calibrate`, `score_of` and `evaluate`
  take `&Target`.
- `keep_if_better` stays GM-only (scores.json). WAV runs compare with the input file's own score.

Alternative: keep the index and add a global "current WAV" lookup. Rejected, because it is hidden state and not
thread-safe with the parallel scorer.

Proof: `gmfit report --check` reports zero diffs, and the committed GM wave JSONs re-render byte-identically.

### D2. WAV reading and the parameter file
- **WAV reading:** a small reader in `gmfit` (RIFF: `fmt`, `data`, `smpl`; PCM 8/16/24/32 and float 32). Stereo
  is averaged, leading silence is trimmed, and resampling uses windowed sinc only when `--rate` is given. The DAW
  path always sends 44.1 kHz mono, which matches GSM import.
- **Parameter file JSON:** `{ "meta": {rate, len, loop: [s, l] | null, unity, fine}, ...WaveFile }`. It reuses
  `params_json` with a `meta` block added. GM wave files keep their format; their meta comes from the skeleton.
- **Entry (binary, little-endian):**

  | field | bytes |
  |---|---|
  | `i32 -1` (never a valid GSM size) | 4 |
  | `u8` version = 1 | 1 |
  | `u32` rate | 4 |
  | `u32` len | 4 |
  | `u32` loop start, `u32` loop len (len 0 = none) | 8 |
  | `u8` unity | 1 |
  | `i16` fine | 2 |
  | `u32` seed | 4 |
  | `u16` params byte count, then `gmgen::Params` encoding | 2 + n |

  The entry is at most about 2.1 KB with 512 numbers.

Seeds: a GM wave uses its index. A WAV entry uses an FNV-1a hash of its parameter bytes, so equal entries render
equally.

### D3. Loop detection (`loopfind`)
1. **The `smpl` loop** is used if it is a forward loop inside the data.
2. **Pitched sounds:** take f0 from autocorrelation, and the vibrato rate from the pitch track (as `loop_vibrato`
   does). Search loop lengths L near `k·rate/f0` that are also near `m·rate/vib`, for each candidate start in the
   sustain (after the attack peak and once the envelope settles within ±3 dB). Score each candidate by:
   - the cross-correlation of the segment after the end with the segment after the start;
   - the wrap-curvature rule (`loop_click`);
   - the played-unroll score of the original against itself, which must be ≥ 0.9.
3. **Unpitched sustained sounds:** pick the segment whose third-octave spectrum is closest to the sustain average,
   with L ≥ 50 ms.
4. **Fallback:** if no loop is accepted, or the sound decays below −40 dB within its length, the target is a
   one-shot (`looped: None`, whole region).

The GM waves have known loops, so they are the test oracle: detection on their unlooped PCM must land on a loop
that scores within 0.01 of the true loop's played unroll. Exact samples are not required.

### D4. Classifier (`classify`)
Measurements, all printed:
- pitch strength (autocorrelation peak) and f0;
- harmonicity: median |cents| of the first 12 peaks from k·f0, and the fitted B;
- inharmonic peak ratios when harmonicity fails;
- the envelope: sustain level after 300 ms re peak, and the decay time;
- the onset: energy share in the first 10 ms, and its spectral centroid;
- the pitch glide in the first 50 ms (zero-crossing track).

Rules map these to the existing `Prior` kinds (Harmonic, Plucked, Struck(inharm), Drum, Metal, Noise/Shaker) and
families. The thresholds are calibrated on the GM bank: for every GM wave, the classifier runs on its PCM, and its
choice is compared with the GM prior's family list. The confusion table goes in the report, and the pilot six must
match (spec scenario).

Alternative: a learned classifier. Rejected, because 495 labelled waves are too few and a rule set is explainable.

### D5. Diagnose (`diagnose`)
Each check is a function `(orig, gen, target) → Check { name, orig, gen, tol, ok, hint }`. All of them use the
target's harmonic spacing chosen by comb energy (the D4i lesson: the pitch estimate can be one bin off).

| check | measurement | tolerance | hint |
|---|---|---|---|
| cluster width | per-harmonic loop-DFT sd in cents, h1–20 | ±30 % | `side` rows / `spread` |
| pitch movement | sd and range of the h1 pitch track | ±25 % | `vib`, vibrato analysis |
| loudness wobble | sd of the 10 ms RMS over the loop | ±25 % | `am` rows / `side` |
| buzz | `score::buzz_band` in 4 bands | gen ≤ 1.25 × orig | `phase_seed` |
| gap floor | mid-gap density per band | ±6 dB where orig > −70 dB | `gap_noise` |
| harmonic regions | region level, h1–44 | ±2 dB | correction tables |
| rumble | power below 0.7·f0 | gen ≤ orig + 6 dB | noise `hp` |
| top octave | level 0.35–0.5·rate | ±6 dB | noise `lp`, tail |
| attack grid | `tf` band × time-row levels | ±3 dB | `tf` / `env` |
| loop groups | `loop_distance` per group | ≤ 3 dB | correction |

The difference-map PNG is gen − orig over STFT frames and mel bands, ±10 dB in red/blue, masked below the
original's −60 dB floor. Tests synthesize a clean tone plus one fault each (spec scenario).

Only the pitched checks run on unpitched targets; the others report "n/a".

### D6. Rendering entries: the caller, not the instrument
`unisynth` stays no-heap. `Chunk::parse` recognizes entries starting with `i32 -1` and validates them structurally
(version, sizes, meta ranges, params decode with the budget). That needs `gmgen::params` decoding, which allocates
nothing. Rendering is the caller's job:
- `crates/song` / the player and `synth-wasm` walk the song's chunks, render each procedural entry with
  `gmgen::render`, and pass `&[&[f32]]` per instrument, as they pass the bank bytes today.
- The sampler reads those samples in place, as it reads DLS waves; the pool size is unchanged by procedural entries.
- No samples passed → silence (spec).

`us-sample-gen` gates the parse arm and the sampler path. The player links `gmgen` only when a song has procedural
entries. The macOS/Linux players already link it for the procedural GM bank.

Alternatives:
- **Decode into the pool inside `unisynth`:** this would need a heap-free `gmgen` (correction DFT, tf STFT and
  noise pattern all allocate today). That is a large rewrite with the same output, so it is deferred. If the
  compo size measurement shows the caller-side path costs more, it can be revisited without changing the format.
- **A 4th source value:** rejected, because it re-quantizes stored source values (Context).

### D7. Pitch rule and rate
Like GSM, the sampler plays one sample per output frame at note 60. The DAW import resamples to 44.1 kHz before
analysis, so an entry behaves exactly like the GSM import of the same WAV, and the user tunes it the same way. The
entry's `unity` and `fine` are stored for the library display and for later use. The sampler ignores them, as it
ignores them for GSM.

### D8. Server route
`POST /api/procedural` (token + Origin, 16 MB) does the following:
1. Validates the RIFF header in the server.
2. Writes `mkdtemp()/in.wav`.
3. Spawns `target/release/gmfit wav make in.wav --progress --json -o out` via `Bun.spawn([...])`, with no shell
   and a fixed path resolved at start-up. It answers 503 if the binary is missing.
4. Keeps one job at a time (409 while busy) and returns a job id.
5. Serves `GET /api/procedural/<id>` with the progress lines and, at the end, `{ entry: base64, score, checks }`.

The time limit is 10 min (kill, rm -rf the temp dir, `timeout`). The temp dir is removed in `finally`. The DAW
stores the entry like a GSM entry and renders it via `synth-wasm`.

The security-relevant surface is the upload, the spawn and the temp files, so the task list runs
`security-hardening` on it.

### D9. Doc and skill
- `docs/synth/wav-resynthesis.md`: what is analyzed and in which order, the stage → parameter table, the limits,
  diagnose checks with hints, and the entry format.
- Project skill `procedural-from-wav`: the loop of make → diagnose → act on hints → re-run, and when to add a
  family.

## Risks / Trade-offs

- [Classifier thresholds tuned on GM waves don't generalize to other sources] → `--kind` override; the DAW dialog
  shows the measured class and offers the kinds; the confusion table is in the report.
- [Loop detection fails on noisy or evolving sounds] → One-shot fallback (correct, just larger tables), `--loop`
  override; the dialog shows the detected loop.
- [Entry render time in the browser at song load (many entries, long sounds)] → Each entry renders its own length
  only. The budget follows the GM bank's (≤ 1 s wasm for 1.5 M samples), and a test measures 10 entries of 2 s.
- [Compo size: Windows player links `gmgen` only for procedural entries] → Measured with the
  `measure-unisynth-size-change` method. Recorded in the report. The user decides per song (GSM vs procedural) from
  the shown sizes.
- [Analysis quality below expectations for families not yet implemented] → Diagnose names it; non-goal here.
- [Running a binary from a web route] → Fixed path, argv only, loopback + token + Origin, size and time limits,
  one job, temp dir confined and removed; security review task.

## Migration Plan

No data migration: chunks with only GSM entries are unchanged (tested on every `project/music` song). Rollback is
to remove the feature. Songs that use procedural entries then fail to load with `UnsupportedModule`, as any song
needing a missing module does.

## Open Questions

- The exact classifier thresholds and diagnose tolerances come out of the calibration tasks. They are recorded in
  the doc and do not change the specs.
