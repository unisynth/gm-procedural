# tools/spessa

Site audio with the site's own engine (`spessasynth_core`). Outputs in git-ignored `build/site-audio/`. Banks: `GM_DLS`, `GMPLUS_SF2` env, `build/gm/gen.dls`, `build/gm-plus/gen.dls`. `skipToFirstNoteOn = false` in both offline and browser so times line up. gm.dls is copyrighted: only its renders are published; other banks play live in the browser.

| File | Purpose |
|---|---|
| `probe.ts` | `bun tools/spessa/probe.ts <bank> <song.mid> <out.wav> [secs]`: one-bank timing/level check (default 20 s). Prints JSON {`load_ms`, `render_ms`, `secs`, `duration`, `peak`, `rms_db`}. |
| `render.ts` | `songs [--only re] [--part i/n] [--max n] [--force]`: per song gm.dls mix + every channel solo (full length, others muted) → `build/site-audio/songs/<id>/{mix,ch<N>}.mp3`, MIDI copy → `midi/<id>.mid`; metrics `tf`/`d_lufs` gen-gm↔gm.dls, `plus` gen-plus↔gmplus, `sf` gmplus↔gm.dls on full mixes + 30 s solo excerpts → `results[.i].json` (resumable). ~1.6 GB/process, ≤ 2 parts. `--max n` exits 10 so a loop restarts (engine leaks ~100–200 MB/song). `waves`: `waveNotes()` (kit key first, else melodic region at unity key) → gm.dls note, `NOTE` {vel 100, hold 1.6 s, total 2.6 s} → `waves/NNN.mp3` + `waves.json`. Exports `RATE`, `Key`, `renderMidi`, `renderNote`, `waveNotes`, `NOTE`. |
