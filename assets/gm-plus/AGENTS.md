# assets/gm-plus

Procedural GM bank fitted to the gmplus soundfont (`GMPLUS_SF2`, never committed) on the gm.dls skeleton at 32 kHz. Change add-gmplus-bank; docs `docs/synth/gm-procedural.md` § The gmplus set.

| File | Purpose |
|---|---|
| `NOTICE.md` | Provenance: gm.dls structure + parameters fitted to gmplus (Sound Canvas) samples; no audio; neither bank committed. See change: add-gmplus-bank. |
| `map.json` | `gm-plus-map/1` (`gmfit plus-map`, `crates/gmfit/src/plus.rs`): `summary` + one row per wave — preset/key/zone/sample, `step`, `loop`, `len`, `dev_cents`, `fine_delta`, `gain_db`, `wsmp_gain_db`, `wsmp_gain` (DLS units, 1/655360 dB), `filter` [Hz, env cents, Q], `pitch_env`, `freeze_s`, `peak`, `fallback`; layered waves add `layers` [{zone, sample, cents, att_cb (×0.4 E-mu), pan, looped}], `locked_dev_cents`, `xfade_zones`; summary `max_locked_dev_cents`, `xfaded`, `loud_wsmp`. All zones summed; unlockable loops crossfaded (design D2, 3.3 review). Stereo (D4): 60 rows with `stereo: {partner}`; partner rows 495–554 `{wave, left, len, loop}`; `structure.json` has 555 waves + `pairs`; `waves/495–554.json` fit the right channel. No PCM (`tests/plus.rs` `map_holds_no_audio`). See change: add-gmplus-bank 1.4. |
| `scores.json` | Regression baseline, 555 waves (495–554 = stereo right channels). Same schema as `assets/gm/scores.json`, one per line. Written by `gmfit report --baseline` / `gmfit fit` for the plus set (`GMPLUS_SF2`). See change: add-gmplus-bank. |
