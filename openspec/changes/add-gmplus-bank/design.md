# Design

## Context

- **Existing pipeline.** `add-procedural-gm-bank` (in progress) fits each of the 495 gm.dls waves with `gmfit`
  (analyze → fit → correct → refine) and packs `assets/gm/{structure.json, waves/}` through `gmgen`'s `build.rs`.
  Scoring compares a generated wave with an `Orig` (rate, samples, loop) played at the bank speed. Another session is
  actively editing `assets/gm/waves/`.
- **gmplus Release 1, measured.**
  - 235 presets with the same (bank, program) set as gm.dls, so the instrument mapping is 1:1.
  - 398 mono samples (370 at 32 kHz, 13 at 44.1 kHz, 9 at 22.05 kHz, 6 at 48 kHz), 106 s in total. No stereo links.
  - SF2.1 modulators only for velocity → filter cutoff.
  - The readme warns about DC offset in some samples.
  - Details in `research/gm-procedural-instruments.md` §4.
- **Consumers.**
  - `dls::Dls` only reads mono PCM.
  - The unisynth sampler slot already outputs `(left, right)` from one mono value and the pan gains.
  - AUMIDISynth (used by `tools/gm-songs`) loads DLS and SF2 and skips unknown chunks.

## Goals / Non-Goals

**Goals:**
- Fit gmplus with the existing machinery, adding only a target source and a set switch.
- Get 32 kHz bandwidth and stereo with no change to how songs address instruments.
- Produce a listening comparison of the four banks that decides whether the plus bank becomes the default.

**Non-Goals:**
- **gmplus's own articulation:** its EG1 times, mod envelope and velocity filter stay out of scope. The gm.dls
  articulation is kept. Adopting gmplus's EG1 is a follow-up if listening shows a decay mismatch.
- **Changing the default bank** of the player or the DAW. A DAW "Procedural+" source is a follow-up.
- **Stereo in WaveSabre Adultery.** It keeps playing the left channel.
- **gmex.sf2,** the variant with scaled-pitch sound effects.

## Decisions

### D1. Keep the gm.dls skeleton and swap only the targets
- Each gm.dls wave is mapped to one gmplus zone. gmplus's own zone layout is not used.
- *Alternative:* rebuild the structure from the SF2 (zones, filters, layered splits). The player would then need a
  filter and a mod envelope per voice, and song compatibility would be lost. Rejected.

### D2. Target extraction (`gmfit plus-map`, SF2 reader in `gmfit`)
- **Choosing the key.** For wave `w`, take the region `play_speed` already picks (`main_user`): the unity key clamped
  into the region, or the drum key in the Standard kit first.
- **Resolving the zone.** Resolve gmplus preset (bank, program), or bank 128 for kits, at that key and velocity 100,
  through the preset and instrument zones. Generators are summed and defaulted per SF2 2.04. **Every matching zone
  is rendered and summed** (93 waves have layered zones), with `initialAttenuation` scaled by 0.4 as E-mu hardware and
  FluidSynth do (`ALT_ATTENUATION_SCALE`; read literally, Ice Rain's own sample at 550 cB would sit 55 dB under its
  pad), gains relative to the loudest zone. Pan is recorded per layer for the stereo block (D4); the mono target
  ignores it. *Revised by the 3.3 review: the first-zone rule dropped real layers (12-string octave, Atmosphere's pad).*
- **Layered loops.** The first looped zone sets the integer loop. Another looped zone locks when the loop holds whole
  source loops of it within 1 cent; otherwise (detuned unisons, noise layers, fifths: 76 waves) it keeps its exact
  pitch and its loop end is crossfaded (equal power, 20 ms) into the audio before the loop start, and the attack runs
  up to the kept period so the loop is entered continuously. One-shot zones end before the loop (10 ms fade).
- **Modulators.** Only the modulators the file defines are applied. The SF2 default velocity→cutoff modulator is not,
  since gmplus defines its own.
- **Building the target:**
  1. Take the raw sample.
  2. High-pass at 10 Hz to remove DC (the readme warns about DC offset).
  3. Resample to 32 kHz (Kaiser sinc, 64 taps, cutoff 0.95 × the new Nyquist when downsampling) so that the gm.dls
     region playing it at the key sounds as gmplus does.
  4. Apply the mod envelope (delay, attack, hold, decay, sustain, key-scaled hold and decay) through the attack:
     - `modEnvToPitch` becomes a variable read step;
     - the zone low-pass follows `initialFilterFc` + `modEnvToFilterFc` × envelope, with the cutoff lowered by the
       play speed, recomputed every 16 samples.

     363 waves sweep the filter and 48 bend the pitch. The attack runs until the envelope settles, at most 1 s of
     played time, past the source loop start if needed. From there pitch and filter are frozen and the loop starts.
     LFOs are not baked: the gm.dls articulation brings its own.
  5. Make the loop a whole number of source loops (m ≤ 32) whose resampled length is an integer. The leftover pitch
     error (≤ 1.4 cents measured) goes into the plus `wsmp` fine tune.
- **Pitch.** gmplus's tuning is kept, including its stretched piano (−4.7 cents at C2 to +2.7 cents at C6 against
  gm.dls). The target matches gmplus's note within 0.25 cent.
- **Cutting the length.** A looped target is the attack plus one loop. An unlooped target is the whole sample, at most
  3 s or the scaled gm.dls length, whichever is longer.
- **Level.** Gain-match to the gm.dls wave on the A-weighted RMS of the first 0.5 s played at its bank speed. A target
  peaking above 0.85 (below the generator's 0.9 soft knee) is scaled down. The cut is added to the wave's (and its
  regions') `wsmp` gain (`wsmp_gain_db` in the map), so loudness in the bank is unchanged.
- **Provenance.** `map.json` records the choices, with no PCM. Targets are rebuilt from `GMPLUS_SF2` in memory on
  demand: the whole bank takes about 7 s on 12 threads, so no disk cache.
- **Measured cost.** Targets total about 338 s of audio (gm.dls about 70 s), mostly the ≤ 1 s baked attacks. Task 2.3
  measures what that does to generation time. `MAX_FREEZE` is the lever if it is too slow.

### D3. Plus structure
- `assets/gm-plus/structure.json` is the gm.dls structure with per-wave `rate` 32000, `len`, `wsmp` loop, fine tune
  and gain from the map, plus stereo pairs.
- Region `wsmp` overrides are rewritten through the same fine-tune and gain deltas.
- `gmgen`'s `build.rs` packs `assets/gm-plus/` when the `plus` feature is on, and `assets/gm/` otherwise.
- `gmfit` loads either set at runtime through `--set`. A shared `Set` (paths, rate, target loader) replaces the
  hard-coded `assets/gm` and `build/gm` paths.

### D4. Stereo partners, fitted where gmplus pans layers
*Revised 2026-11-05 (user decision): fit-only. The first plan designed a stereo block for every wave per family; then
the 3.3 review found gmplus pans the layered zones of 60 waves apart, a real stereo image to fit.*
- **Which waves.** A wave is stereo when its zones' pans differ. Its left and right targets sum the zones with
  equal-power pan (FluidSynth: L = cos, R = sin of (pan + 500)/1000 · π/2), normalised so the louder channel takes
  the level match. All other waves stay mono.
- **Partners.** The right channel is wave `495 + k` (k-th stereo wave in index order), appended to the skeleton and
  never referenced by a region; it shares rate, length and loop with its left wave (both rendered on one timeline).
  `map.json` lists `stereo: {partner}` on the left row, and partner rows carry `left`.
- **Fitting.** *(Revised after the first stereo fit: independently fitted partners had unrelated phases, loop
  correlation −0.76…0.18 against gmplus's 0.97–1.00, and the mid cancelled.)* A partner is its left wave's
  parameters, rendered with the left's seed (`gmgen::bank::seed_of`), plus the analytic correction (`correct_all`)
  against the right target (`gmfit --set plus partners`): median loop correlation 1.000, mid 0.511 vs left 0.508,
  right 0.490 vs 0.391 for a plain copy. A pair whose target channels correlate below 0.5 (different samples per side;
  none in gmplus Release 1) is fitted on its own (`bulk --partners`). Its "user" for play speed and category is the
  left wave's (`gmfit::voice`).
- **Checks.** The left wave alone within 0.05 of the pair's mid against the mono (centre) target; loop correlation
  ≥ 0.3 or a `stereo_low_corr` flag.
- *Alternatives:* a designed ≤ 8-number block per family (the first plan; rejected by the user: no stereo where gmplus
  has none), and a block fitted to the right target (fewer numbers, but a block cannot follow two differently
  detuned layers; a full partner can).

### D5. DLS stereo-pair chunk
- A top-level `LIST 'stpr'` holding `u32` pairs `(left, right)`.
- Right waves are appended to the wave pool at index ≥ 495 and are never referenced by a region.
- Choosing a `LIST` with a private form type keeps standard readers skipping it.
- `dls::Dls::partner(w)` validates rate, length and loop equality lazily and returns `None` on any mismatch.
- *Alternative:* DLS2 `wlnk` channel steering with two regions per key. AUMIDISynth's DLS2 support is uncertain, and
  every reader would see doubled regions. Rejected.

### D6. unisynth stereo sampler (`us-sample-stereo`)
- At region pick, the sampler stores the partner `Wave` next to the left one. `State::next` advances one position
  and reads both. The slot emits `(yL·pl, yR·pr)`.
- The feature is not derived by `needs.rs`: the chunk does not say which bank the build ships, and without the
  feature a pair plays its left wave (correct mono). The build that ships the plus bank adds it. *(Revised in 4.4.)*
- The size cost is measured with `measure-unisynth-size-change`.

### D7. Four-way comparison in `tools/gm-songs`
- `prepare` makes the render copies `orig.dls` (gmfit bank --orig), `gmplus.sf2` (copied as-is), `gen.dls`, and
  `gen-plus-L.dls` / `gen-plus-R.dls`. The R copy rewrites each paired wave's PCM with its partner.
- AUMIDISynth plays only mono waves, so gen-plus stereo is rendered as two passes. Each pass is deterministic
  offline: the left output of the L bank and the right output of the R bank are interleaved.
- `prepare` checks that the trick is exact: an unpaired bank rendered both ways must be bit-identical.
- Metrics pair each bank with its source as in the spec. The page grows to four synchronised players (one `<audio>`
  group per row, shared seek).
- *Alternative:* render through unisynth. Rejected, because unisynth ignores DLS articulation, and the comparison must
  differ only in the waves.

## Risks / Trade-offs

- [The gmplus zone for a key is a different sample than gm.dls used, e.g. a different split point or a later SC
  model.] → `map.json` records the sample name, and `gmfit --set plus render` shows the gm.dls wave as a third trace.
  Mismatches are reviewed per category in the pilot.
- [gmplus's mod-envelope motion is lost beyond 1 s.] → Accepted. The attack bakes it up to `MAX_FREEZE` (1 s), and
  the loop holds the frozen value.
- [gm.dls articulation with gmplus waves decays differently from gmplus itself.] → The four-way page shows it. EG1
  adoption is the documented follow-up.
- [Load time grows by about 32/22.05 × (1 + stereo share).] → Measure the full build at 32 kHz in task 2; the budget
  is set there with `performance-optimization`. Stereo can be limited to the families where it is audible.
- [Stereo decorrelation causes comb filtering in mono.] → The mid-score gate and the correlation floor (D4), plus a
  mono-sum check on the song page (mid ΔLUFS).
- [Parameters fitted to Roland samples.] → The same stance as the gm.dls set. The soundfont is never committed,
  `map.json` has no audio, and `doubt-driven-review` runs before the first bulk commit.

## Migration Plan

- The change is additive. `assets/gm/`, the default `gmgen` build and every existing command keep their behaviour.
- Rollback means dropping the `plus` feature and `assets/gm-plus/`.

## Open Questions

- The exact per-family default `width`, set after the pilot listening pass. The mechanism and gates are fixed.
