---
name: gmfit
description: Fit and iterate procedural GM bank waves (assets/gm/waves/NNN.json) with gmfit — analyze → fit → correct → refine, render the original/generated pair and sheet, read score terms and flags (pitch_off, env_off, buzz, loop_click, clipped), sweep a parameter, check for buzz / missing noise floor / rumble that the score averages away, keep only improvements, re-baseline scores.json. Use on "fit wave N", "this GM wave sounds wrong", "improve the strings/piano/kick", "procedural gm bank", "gmfit", "the generated wave buzzes", "A/B the pilot", "bulk fit a GM family".
---

# gmfit: fit a GM wave by measuring, then listening

Every GM wave is a parameter file `assets/gm/waves/NNN.json` (≤ 4 layers, ≤ 512 numbers) that `gmgen` renders into
the bank. `gmfit` compares the render with the Windows `gm.dls` wave and fits parameters. **The score is a guide,
not the judge.** Five of eight pilot rounds fixed problems the score could not see (design D4c–D4i). Measure the
specific thing you hear before you change anything.

Reference: `docs/synth/gm-procedural.md` (formats, families, commands, score). History and lessons:
`research/gm-procedural-pilot.md`, `openspec/changes/add-procedural-gm-bank/design.md` D4a–D4i.

## Setup

```bash
export GM_DLS=$HOME/Downloads/64k-tools/gmdls/gm.dls      # licensed original; never commit it
cargo build -q --release -p gmfit                          # gmgen's build.rs packs assets/gm (fails on a bad file)
G=./target/release/gmfit
```

Find a wave: `assets/gm/structure.json` = `{format, instruments, waves}`. `instruments[].regions[].wave` gives the
wave index. `waves[N]` = `{rate, len, wsmp: [unity, fine, gain, loop start, loop length]}`; an unlooped wave has
only 3 `wsmp` entries. Pilot: kick 376, snare 161, hat 83, Fingered Bass 240, Strings 405, Piano 293. Build and
release runs take minutes: `fit --iters 200` about 1 min, `refine` 2–5 min, `report` about 1 min.

## Loop

```bash
$G render 405                       # → build/gm/405/{orig,gen}.wav, sheet.png; JSON: score, d_spec, d_onset, d_env, d_loop, flags
$G analyze 405 --force              # initial params from the original (prior from the GM category); -o f.json to try without writing
$G fit 405 --iters 200              # CMA-ES over the explicit scalars (structural ones excluded)
$G correct 405                      # harmonic → tf → env tables, gap noise, phase seed
$G refine 405 --rounds 3 --iters 100
$G sweep 405 0.level_db -12,-6,0    # <layer>.<scalar> (or a wave scalar); rows sheet build/gm/405/sweep.png
$G report --check                   # whole bank vs assets/gm/scores.json: fails on any drop > 0.005
$G report --baseline                # after an accepted change: rewrite scores.json
```

**What writes what.**
- `fit`, `correct` and `refine` write the wave file **and its row in `assets/gm/scores.json`** only when the score
  improves (`keep_if_better`).
- `analyze --force` overwrites the wave file whatever its score. Back it up first
  (`cp assets/gm/waves/NNN.json /tmp/NNN.bak.json`) and restore it with `cp` if the full pipeline ends lower.
- `report --check` re-renders the whole bank against `scores.json`, so after a single-wave fit it checks that no
  *other* wave moved (the shared generator code).
- `report --baseline` rewrites every row and `build/gm/report.md`. Run it after a change to the generator or the
  score, which moves many waves.

**Output.** Every command prints JSON on stdout: `render` gives score, terms, flags, f0s and
`score_without_correction`; `analyze` gives only score and flags (run `render` for the terms); `fit`, `correct`
and `refine` give before, after and written. `build/gm/NNN/report.json` holds the same as `render`.
`GMFIT_TRACE=1` prints refine rounds, correction passes, gap-noise candidates and phase-seed choices.
`GMFIT_DUMP=/tmp/x.json` writes the best gap-noise candidate even when it is not kept, so you can listen to it or
render it with `-p`. `render` exits with code 2 for a tier-C wave (scores below B), so chain with `;` not `&&`.

**Placeholders** (`"placeholder": true`, score ≈ 0.01) often show `pitch_off` with f0 at 2×. Run `analyze --force`.

1. `render`: read the sheet (waveforms, spectrograms, difference) and the terms. Large `d_onset` → attack; `d_env` →
   loudness contour; `d_loop` → loop spectrum; `d_spec` → overall.
2. Run the pipeline. Keep the result if it is better, and restore the backup if it is not.
3. Listen, or let the user listen: rebuild `build/gm/pilot/index.html` rows (orig/gen/sheet), serve the folder with
   `serve_mockup` and open the **localhost** URL on the canvas. A `kind:file` canvas of that page failed with "live-server
   target not found".
4. For what is heard but not scored, **measure it** (below), fix the analysis or generator, and re-run.
5. Test, run `report --check`, `report --baseline`, document (design D4x, tasks, pilot research), and commit.

## Measure what the score averages away

```bash
$G render 376 >/dev/null && uv run -q --with numpy python .pi/skills/gmfit/measure.py 376
```

`measure.py` runs the checks below on `build/gm/NNN/{orig,gen}.wav` (mono 16-bit, the played second: attack, then
the loop repeated). Each line gives orig, gen, OK/OFF and the fix. Checks that cannot apply print n/a:
- **buzz:** needs a pitched loop.
- **cluster width and gap floor:** need at least 10 periods per loop. A single-period loop (organ, bass) is exact
  harmonics by construction.

Thresholds are provisional (add-wav-resynthesis design D5). Treat OFF as "listen here", not as a verdict.

| Heard | Measure | Fix (where) |
|---|---|---|
| static / "synthy" sustain | per-harmonic loop-DFT cluster width (cents), pitch-track sd/range, 10 ms RMS sd | vibrato + `vib` table, `am` rows, `side` rows (analyze `loop_modulation`) |
| buzz in the middle/top | per-period peak/mean of the 2.5–10 kHz band (`score::buzz_band`); flag `buzz` | `phase_seed` (random upper phases) |
| pattern in the difference map / no "air" | mid-gap floor (bins ≥ per/5 from every harmonic) per band, signed mean | `gap_noise` layer |
| hiss / "shhh" | per-band periodicity (autocorrelation at one period): lower than the original = too much noise | `gap_noise` bands (periodicity-calibrated, design D4k) |
| rumble | power below 0.7·f0 | noise `hp` |
| too bright / dull top | 500 Hz-band levels 6–11 kHz | noise `lp`, harmonic tail |
| attack click / wrong decay / cut at the end | level per 50 ms × band (`tf` grid); a one-shot's last 10 ms vs the original | `tf`, `env` (one-shot end point, design D4j), drum noise layer |

## Pitfalls (each cost a round)

- **Harmonic spacing.** `score::pitch` can be one period per loop off under vibrato (405: 97 vs 98), which puts
  "gaps" on the upper harmonics. Choose the spacing by harmonic-comb energy.
- **Signed differences.** A bin-wise positive part of orig − gen invents energy (phantom −16 dB noise). Average
  the signed difference per band.
- **Random texture fails.** Random spread or AM scrambles harmonic-locked modulation (405: spread 12 → 0.45).
  Deterministic sidebands (`side`) work.
- **Truncated sidebands.** Cut to 97 % energy, `side` rows matched the spectrum but broke the loudness dips. Use
  99.5 %, ≤ 11 bins.
- **Budget overflows.** Raising a table width can push committed files over 512 numbers and break the build.
  Re-run analysis or trim (weak `side` rows first, then the harmonic tail).
- **Changing a schema grid** (`TF_BANDS` …) invalidates committed `tf` rows. Strip them and reset the score
  before refitting.
- **Structural scalars** (`unison`, `dly`, `spread`, `vib_rate`, `phase_seed`, delayed-layer `a h d s`) stay
  out of CMA-ES; the fit drifts them off the loop grid.
- **Drums.** A fresh `analyze` can be worse than a refined committed file (snare: 0.557 `env_off` vs 0.677).
  Always compare with the backup.
- **Energy is not texture.** Matching the mid-gap *energy* with noise gave hiss (405 round 8): the original's
  mid-gap energy was mostly pitched smear. Check periodicity per band (`measure.py`) after adding any noise.
- **One-shot ends.** The originals end anywhere from −5.8 to −90 dB re peak. A generated one-shot that stays loud
  until its last sample is cut off, and the score barely sees it. Check the last 10 ms (`measure.py` attack grid).
- **Score drops that are not regressions.** A change can fix what you hear and still lose a little score (gap
  noise: −0.0035 from ±0.5 dB inside harmonic groups). Write it only on listening feedback, within the 0.005 gate,
  and record why.

## Verify

- `cargo test -q -p gmgen --features std` and `cargo test -q --release -p gmfit` pass.
- `cargo +nightly build -p gmgen --target wasm32-unknown-unknown -Zbuild-std=core,alloc` passes (determinism, no_std).
- `gmfit report --check` reports no regressions, then `--baseline`.
- `openspec validate add-procedural-gm-bank --strict`.
