# site

GitHub Pages site, deployed by `.github/workflows/pages.yml` on push to `main` (custom domain `CNAME` = `gm-procedural.exceed.hu`, org `unisynth`). Static, no build step, no frameworks; design tokens copied from unisynth.exceed.hu. gm.dls audio (copyrighted) streams as mp3 from `gs://gm-procedural/site/` (public, CORS `*`); gmplus, gen-gm and gen-plus play live from the MIDI (SpessaSynth worklet), the procedural banks built in the page by `gmgen.wasm`. Pages serves only HTML/CSS/JS; every binary (mp3, MIDI, gmplus.sf2, fallback DLS, `gmgen.wasm`) is fetched from the bucket. `data/*.json`, `assets/live.js`, `assets/spessasynth_processor.min.js` come from `bun tools/site/build.ts`.

| File | Purpose |
|---|---|
| `assets/home.js` | Landing page: hero scope (original vs model trace per family, real Karplus-Strong), attack/loop SVG, family rack from `stats.json` layer counts, tiers/category bars/song medians, reveal-on-scroll. |
| `assets/gmgen-worker.js` | Web Worker: fetches `gmgen.wasm` from the bucket (URL in the message, `?v=<sha1[:12]>`), instantiates it, `begin/step/finish`, posts `{progress, wave, waves}` then `{done: ArrayBuffer, ms}`. |
| `assets/player.js` | `Deck`: bank `o` = gm.dls `<audio>` (mp3), `s/g/p` = `Live` (MIDI or one note); same-position switch, seek canvas with live spectrum + bank build progress, keys 1–4/Space/←→/L, items `{kind: "song", mp3, midi, seconds, solo}` / `{kind: "note", mp3, note}`; `NOTE` hold 1.6 s / 2.6 s; bank in `localStorage` `gmp.bank2`; `window.deck` on pages for tests. |
| `assets/site.css` | Shared styles: unisynth palette + bank colours `--b-o/s/g/p`, landing sections, listening app (deck, list, stems, sound grid). |
| `assets/live.js`, `assets/spessasynth_processor.min.js` | Generated (tools/site); live engine bundle (Apache-2.0 SpessaSynth inside, `GMGEN_WASM_HASH` defined at bundle time), AudioWorklet processor. |
| `data/` | Generated JSON (songs, waves, stats). Do not edit by hand. |
| `index.html` | Landing: problem, model (10 families), fit pipeline, judge (score + hearing), results, listen, papers (cited), provenance. |
| `songs/index.html` | Song selector: search, collection chips, sort by title/tf/length, detail with mix metrics and per-channel solo parts; URL `#<id>[/ch<n>]`; N/P next/previous. |
| `sounds/index.html` | 495-sound grid: category chips, search, sort by score, four versions per wave, flags; URL `#w<n>`. |
