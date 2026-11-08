# tools

bun/shell tools. Each `*.test.ts` runs under `bun test`.

| File | Purpose |
|---|---|
| `gm-songs/` | Full-song four-bank A/B (gm.dls, gmplus.sf2, gen-gm, gen-plus) via AUMIDISynth: SMF parse, tf distance, DLS wave naming, `run.ts` driver, pages, Range server → see `gm-songs/AGENTS.md` |
| `publish.sh` | Uploads site audio to public `gs://gm-procedural` (`GM_BUCKET`): `build/site-audio/{songs,waves,midi}` → `site/…`, `GMPLUS_SF2`, `build/site-audio/banks/gen-gm.dls` (wasm output, loaded by default), `build/gm-plus/gen.dls` (fallback) → `site/banks/{gmplus.sf2,gen-gm.dls,gen-plus.dls}`, `build/site-audio/gmgen.wasm` → `site/gmgen.wasm` (application/wasm), `build/site-audio/icon/` → `site/icon/`, hand-made `pages/<n>` + `build/gm/<n>/` clips → `gm/pages/<n>/`, `pages/index.html` → root. Never uploads gm.dls, logs, json. `--dry-run`. |
| `site/` | Site builder: `data/*.json` (songs, waves, stats), bundles SpessaSynth live engine `live.js`, song titles → see `site/AGENTS.md` |
| `spessa/` | Site audio + metrics with spessasynth_core: gm.dls mix/solo mp3s, per-wave notes, four-bank tf/ΔLUFS results, one-bank probe → see `spessa/AGENTS.md` |
