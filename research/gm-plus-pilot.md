# gmplus pilot (change add-gmplus-bank, task 3.2) — 2026-11-05

The six pilot waves of `gm-procedural-pilot.md`, fitted to the gmplus targets with one `gmfit --set plus bulk` run
(analyze → fit → correct → refine, 160 iterations, 2 rounds, 6 jobs; 93–329 s per wave). The gm column is the gm.dls
set's committed score from `assets/gm/scores.json`, after many rounds of hand iteration. The plus column is a single
unattended run.

| Wave | Instrument | gmplus sample | gm score | plus score | plus tier | plus d_spec / d_env / d_loop | plus flags | plus layers | Listening (A/B, to fill in) |
|---|---|---|---|---|---|---|---|---|---|
| 376 | Kick (Standard 36) | BassDrum 2 | 0.721 | 0.618 | C | 1.50 / 1.32 / – | – | drum + noise | |
| 161 | Snare (Standard 38) | Fat Snare | 0.763 | 0.711 | B | 1.22 / 0.59 / – | pitch_off | modal + noise + shaker | |
| 83 | Closed hat (Standard 42) | Closed Hat | 0.696 | 0.638 | C | 2.31 / 0.39 / – | – | metal + noise | |
| 240 | Fingered Bass | fingerbs 1 | 0.816 | 0.583 | C | 0.42 / 0.59 / 2.74 | – | additive attack + sustain | |
| 405 | Strings | Strings 0 | 0.779 | 0.707 | B | 1.02 / 0.49 / 0.01 | – | additive sustain (506 numbers) | |
| 293 | Piano 1 (C4) | Piano C4 | 0.714 | 0.741 | B | 1.00 / 0.50 / 0.01 | – | additive attack + sustain | |

Listen: `build/gm-plus/NNN/{orig,gen}.wav` (orig = gmplus target at 32 kHz, gen = fitted), sheet rows target /
generated / gm.dls in `build/gm-plus/NNN/sheet.png`. Re-render with
`GM_DLS=… GMPLUS_SF2=… gmfit --set plus render NNN`.

## Findings

- **Piano improves over the gm set in one run** (0.741 against 0.714). Its target is a 1 s baked filter sweep (base
  591 Hz, +2200 cents from a 4 s mod-envelope decay) plus a loop of whole source loops; the model follows it.
- **Fingered Bass is the outlier:** `d_loop` is 2.74 against ≤ 0.01 for the other looped waves.
  - Pitch is right: 246.71 Hz target, 246.86 Hz generated.
  - The target holds 0.75 s of baked filter sweep (base 1000 Hz, +3000 cents envelope). Its loop of 389 samples holds
    3 periods of the target pitch (gmplus loops 3 periods of its 32 kHz sample).
  - The loop spectrum at the frozen cutoff is missed. To look at with `systematic-debugging` before the bulk fit:
    the analysis of the multi-period loop, and how the attack hands over to the sustain layer.
- **The drums score lower than on the gm set** (kick 0.62 against 0.72, hat 0.64 against 0.70). gmplus's drums are
  different, longer recordings (SC-55 / R8). The gm-set drum fits needed several hand-tuned family additions (D4l–D4s)
  that a single bulk run does not repeat.
- **Strings** use 506 of 512 numbers in one sustain layer: the 32 kHz target needs more harmonics than the 22.05 kHz
  one. That is close to the budget.

## Fingered Bass (240): `inharm` + `pdecay` trap

`d_loop` 2.74 came from the sustain layer: the target's loop is strictly harmonic (3 periods), but the fit held
`inharm` 0.0064 (partials 4+ off the loop's harmonic bins, −75 dB there) and `pdecay` 0.39 (upper partials decayed by
the loop). Sweeping either alone barely moves the score; zeroing both gives `d_loop` 0.96. `fit::loop_reset` (after
`correct`, looped waves) zeroes both on additive sustain layers and refines from that candidate too; the better
refine wins. 240: 0.583 → 0.680. Shared with the gm set (it only adds a candidate; keep-if-better).

## Doubt-driven review (task 3.3), 2026-11-05

Cross-model reviewer `@propose-review-1`, two cycles, on `plus.rs`, `dsp.rs`, `sf2.rs` against the target contract
(zone choice, pitch, seamless loops, DC/aliasing, layering, level, determinism, licence, bulk-fit suitability).

| Finding | Class | Resolution |
|---|---|---|
| Bag ranges inclusive (`sf2.rs`) | noise | `zones()` builds zones from `windows(2)`; b1−b0 zones exactly |
| Only the first layered zone rendered | valid | all zones summed (attenuation × 0.4, E-mu/FluidSynth) |
| Loop source margin ignores the read step | valid | margin `TAPS_HALF·step`; test `high_step_loop_keeps_its_level` (11 waves had step > 4) |
| Loops > 16384 samples, `dev` reported as 0 | trade-off + valid | long single source loops kept (noise/pads); `dev` now real |
| Level matched on the first 0.5 s | valid | loudest 100 ms of the first 1 s (both sides cut at 1 s); `wsmp` > 12 dB flagged (only 301 Breath Noise) |
| > 1 cent on 3 waves (`MAX_PERIODS` 32) | valid | 256; max residual 0.448 cent |
| High-pass start-up transient repeated in early loops | valid | looped sources lose their loop mean instead |
| Loop past sample data could panic | valid | `Err`; `plus-map` exits 1 on any error row |
| Attacks up to 2.8 s | trade-off | the source's loop starts late; covered by the sample budget |
| Layer residuals 100–2300 cents (cycle 1 fix) | valid | primary-set loop + lock-or-crossfade; tests `octave_layer_locks_and_sums`, `detuned_layer_crossfades` |
| Short one-shot layer ends abruptly (107) | valid | 10 ms fade at every one-shot end |
| `worst_dev_cents` hid crossfaded layers | valid | renamed `locked_dev_cents`; crossfaded count in `xfade_zones` |
| `fine_delta` always 0 | trade-off | DLS fine tune is whole cents; ≤ 0.45 cent stays |
| Mono, pan ignored | contract (stereo is group 4) | pan recorded per layer |
| Degenerate inputs (`l == 0`, fade 0) | valid (latent) | `continue` / `max(1)`, one-shot fallback |

**Licence stance (unchanged, checked):** `gmplus.sf2` is read from `GMPLUS_SF2` only and never copied into the repo
(`tools/gm-songs` copies it into the git-ignored `build/`); `map.json` holds names, numbers and provenance, no PCM
(test-enforced); committed parameters are fits, not samples. The target audio exists only in `build/`.

Result after the fixes: 495 targets, 1 fallback (366), 0 errors, 93 layered (76 crossfaded), max residual 0.448 cent,
12.46 M samples (guard 13 M). The six pilot fits were made on the old targets; the bulk fit (task 5) refits all.

## Listening feedback, 2026-11-05

- **239, 293: "a break at the half of the instrument".** The string-attack layer handed over to the sustain layer at
  the loop start, after up to 1 s of baked attack: 293 had a 2–3 dB hole at 0.9 s and a 2.3 dB step at 0.99 s;
  240 a cancellation frame at 0.74 s then +2 dB. Level, brightness and clicks elsewhere were clean. Fix: on the plus
  set, loops later than 0.3 s skip the hand-over (sustain from the start). 239 0.748 → 0.762, 240 0.719 → 0.774;
  293 0.740 → 0.709 by the score but without the break (installed by hand; the score averages the hole away). The
  bulk fit re-runs the 73 fitted waves that had a late hand-over.
- **405: "upper harmonics missing".** Band energies match within 1.4 dB up to 16 kHz; what is missing is the
  ensemble smear between the harmonics (gap energy target −20 / −5 dB at 1–2 / 2–4 kHz, fit −47 / −17.5 dB). The
  fit spent 506 of 512 numbers on one harmonic table, leaving no room for `spread` or a gap-noise layer.

## Bulk fit and four-way songs, 2026-11-06

- All 495 waves and 60 stereo partners fitted (`report --baseline`): tiers A 83 / B 135 / C 337, mean 0.607.
- Songs (70, AUMIDISynth, median mix `tf`): gen-gm ↔ gm.dls 4.30, gmplus ↔ gm.dls 4.22, **gen-plus ↔ gmplus 4.35**
  (7.42 before the bulk fit, with placeholders). gen-plus is now as close to gmplus as gen-gm is to gm.dls.
- Stereo: independently fitted partners were incoherent with their left waves (loop correlation −0.76…0.18; gmplus's
  own channels 0.97–1.00). Partners are now the left's parameters + correction with the left's seed: correlation 1.000.
- 405 Tremolo Strings: `spread` 16 by hand (ensemble smear between harmonics at 2–8 kHz now matches); the 1–2 kHz bow
  noise needs a noise layer the 512-number budget has no room for.
- Weak waves (48 < 0.3): pitched sustained sounds fitted with `modal`/`fm` (SynBrass 340–342, Polysynth 309–310,
  Tenor Sax 470, 5ths Lead 210, 60s E.Piano 10/12, Chiffer/Calliope leads), mostly layered crossfade waves the
  additive model cannot follow; drums/SFX `env_off`. Follow-up: family selection for layered pitched targets.

## Listening guide for task 6.4 (per category, 2026-11-07)

Median stem `tf` of each generated bank against its own source (lower = closer); listen to the closest and furthest
gen-plus stem per category on `bun tools/gm-songs/run.ts serve` (keys 1–4 switch at the same position).

| category | stems | gen-plus ↔ gmplus | gen-gm ↔ gm.dls | closest gen-plus | furthest gen-plus |
|---|---|---|---|---|---|
| ensemble | 102 | 6.30 | 3.44 | duke-nukem-3d-stalker Strings 3.0 | realms-of-arkania-2-ingame Choir Aahs 14.3 |
| brass | 75 | 6.38 | 8.34 | betrayal-at-krondor-battle Tuba 2.5 | under-a-killing-moon-tex-s-office Synth Brass1 10.6 |
| bass | 59 | 4.15 | 8.87 | betrayal-at-krondor-battle Acoustic Bs. 1.6 | descent-l01-lunar-outpost Synth Bass 1 14.7 |
| drums | 59 | 4.84 | 2.84 | betrayal-at-krondor-battle Standard 3.1 | descent-l01-lunar-outpost TR-808 9.3 |
| guitar | 53 | 6.16 | 9.30 | sam-n-max Jazz Gt. 2.2 | gus-cyber Gt.Harmonics 12.9 |
| piano | 53 | 5.33 | 7.08 | gus-orientex Piano 1 2.4 | stonekeep-1 Piano 1 12.4 |
| pipe | 37 | 6.59 | 12.96 | under-a-killing-moon Recorder 2.9 | gus-equinox4 Bottle Blow 19.2 |
| reed | 34 | 3.85 | 8.04 | discworld-unseen-university Bassoon 0.5 | stonekeep-2 Clarinet 9.4 |
| strings | 34 | 6.72 | 5.51 | gabriel-knight-bookstore Cello 1.4 | gus-videokil Viola 11.0 |
| chromatic | 32 | 7.92 | 13.64 | tie-fighter-intro Vibraphone 3.3 | arena-raining Glockenspiel 16.3 |
| pad | 25 | 8.98 | 7.68 | descent-l01 Sweep Pad 3.9 | gus-psr510-4 Space Voice 14.9 |
| lead | 24 | 7.00 | 7.69 | gus-whatislo Saw Wave 3.8 | shadow-warrior-intro Syn.Calliope 18.9 |
| percussive | 22 | 9.43 | 2.97 | descent-intro Agogo 3.1 | descent-l01 Synth Drum 18.5 |
| ethnic | 17 | 6.67 | 11.21 | shadow-warrior-intro Shamisen 4.2 | gus-calypso2 Kalimba 11.9 |
| organ | 17 | 5.10 | 10.32 | gus-hotelcal Organ 1 1.4 | gus-l1002-02 Organ 3 9.2 |
| synthfx | 13 | 11.41 | 9.36 | arkania-2-intro Atmosphere 4.7 | gus-equinox4 Goblin 15.9 |
| sfx | 13 | 6.82 | 3.52 | gus-l1002-08 Applause 4.4 | gus-calypso2 Bird 13.4 |

By the numbers, gen-plus follows gmplus better than gen-gm follows gm.dls in bass, brass, guitar, piano, pipe, reed,
chromatic, ethnic, organ and lead, and worse in ensemble (choir), drums, percussive, pad, synthfx and sfx (the weak
`env_off` drums/SFX and layered pads of the bulk-fit triage).
