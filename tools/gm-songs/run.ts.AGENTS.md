# run.ts

Full-song A/B driver for the procedural GM bank vs Windows gm.dls (change add-procedural-gm-bank; four-way extension: change add-gmplus-bank). CLI: `bun tools/gm-songs/run.ts select|prepare|render|waves|page|serve`. Docs: `docs/synth/gm-procedural.md` § Full-song A/B. Everything it reads or writes is under git-ignored `build/` (never committed).

## Banks (add-gmplus-bank)

- Four banks, key → file suffix / label (`BANK`): `o` orig / gm.dls (`build/gm/orig.dls`, `gmfit bank --orig`), `s` sf / gmplus (`GMPLUS_SF2`, copied to `banks/gmplus.sf2`), `g` gen / gen-gm (`build/gm/gen.dls`, `gmfit bank`), `p` plus / gen-plus (`build/gm-plus/gen.dls`, `gmfit --set plus bank -o build/gm-plus/gen.dls`).
- Same renderer for all four: Apple AUMIDISynth via `build/gm/songs/gmrender` (`swiftc -O tools/gm-songs/render.swift`).
- Inputs: `build/gm/midi/gus` (archive.org BESTMID6 "The Very Best of GUS MIDI"), `build/gm/midi/novak` (archive.org DOS-Gaming-General-MIDI-Comparison `midi-files.zip`). Git-ignored.

## Commands

- `select [--gus N]` → `build/gm/songs/selection.json`. Novak DOS set (all, minus test-tone) + N GUS songs (default 24). GUS picked by greedy coverage: new melodic program counts 1, new drum key 0.25; 45 s – 8 min, ≥ 3 channels. Dedup by md5. Titles from GUS `DESCRIPT.ION`.
- `prepare` → `build/gm/songs/banks/{orig,gen,plus}.dls` (named via `nameWaves`), `gmplus.sf2`, `plus-R.dls` (`rightPass`). Self-checks: every DLS copy plays Organ 1 (prog 16, C4) and goes silent with wave 150 zeroed (`PROBE_WAVE`; renderer plays the wave the region links to); gmplus.sf2 probe note sounds; determinism (same bank rendered twice is bit-identical, needed for exact two-pass stereo). Needs `GMPLUS_SF2`.
- `render [--only re] [--shard i/n] [--force]`: per song, mix + per-channel solo renders through all four banks; mp3 `<name>-{orig,sf,gen,plus}.mp3` with one gain per row (peak 0.89 over four banks); solo mp3 = 30 s excerpt from the channel's first note − 0.3 s (`EXCERPT`); 2 s tail (`TAIL`). Writes `results.json` (`results.<i>.json` with `--shard`; `page` merges `results*.json`). Skips songs that already hold four-way results unless `--force` (crashed run resumes). `JOBS` env = parallel songs, default `min(3, cpus-1)`: each job ≈ 1 GB, never shard ×8 (ran ~120 renders, exhausted RAM). Temp float WAVs removed per stem.
- Metrics per row (`metrics()`): `lufs_orig`, `lufs_gen`, `d_lufs`, `tf`, `gain`, `octaves`, `bands`. Top level = g↔o; `plus` = p↔s; `sf` = s↔o. Stems add `label`, `waves` (notes per wave via structure regions × played keys × programs), `bank_score` (notes-weighted `assets/gm/scores.json`).
- Stereo gen-plus (add-gmplus-bank D7): gen-plus audio = left of `plus.dls` render + right of `plus-R.dls` render (AUMIDISynth plays mono waves).
- `waves`: encodes `gmfit dump` output (`build/gm/songs/waves/gm/`, `.../plus/`, 495 waves) → `waves/NNN-{orig,gen,sf,plus}.mp3` (lame -V4); o/g from gm set, s/p from plus set. Stereo waves (`assets/gm-plus/map.json` `stereo.partner`) merged left+partner into one stereo WAV before encoding.
- `page` → `index.html` (songs; per-stem tf/ΔLUFS for gen-gm↔gm.dls, gen-plus↔gmplus, gmplus↔gm.dls; 'plus fitted' coverage = share of notes on `assets/gm-plus/waves` non-placeholder fits), `report.md` (instruments ranked worst-first by tf, wave ids, gmfit score, corr(bank score, tf) over stems within 20 dB of mix), and `waves.html` (495 sounds, category filter + only-fitted filter) when `waves/gm/index.json` exists. Shared `PLAYER_JS`: keys 1–4 switch bank at same position, seek bar, space, ←/→ ±5 s, L loop.
- `serve [port]` → `serve()` from `serve.ts` over `build/gm/songs`, default 60941 (Range 206 needed for seeking and same-position bank switching).
- `SOURCES` (exported): MIDI + bank credits HTML block on both pages; renders are ours, inputs are public downloads never re-hosted.
