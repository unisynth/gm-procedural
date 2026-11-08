# site

GitHub Pages site, deployed by `.github/workflows/pages.yml` on push to `main` (custom domain `CNAME` = `gm-procedural.exceed.hu`, org `unisynth`). Static, no build step, no frameworks; design tokens copied from unisynth.exceed.hu. Audio streams from `gs://gm-procedural` (public, CORS `*`); `data/*.json` comes from `bun tools/site/build.ts`.

| File | Purpose |
|---|---|
| `assets/home.js` | Landing page: hero scope (original vs model trace per family, real Karplus-Strong), attack/loop SVG, family rack from `stats.json` layer counts, tiers/category bars/song medians, reveal-on-scroll. |
| `assets/player.js` | `Deck`: one `<audio>`, four banks `BANKS` (o gm.dls / s gmplus / g gen-gm / p gen-plus), same-position switch, seek canvas with live spectrum (AnalyserNode; falls back to plain playback when CORS fails), keys 1–4/Space/←→/L, bank in `localStorage`. `transportScroll()`. |
| `assets/site.css` | Shared styles: unisynth palette + bank colours `--b-o/s/g/p`, landing sections, listening app (deck, list, stems, sound grid). |
| `data/` | Generated JSON (songs, waves, stats). Do not edit by hand. |
| `index.html` | Landing: problem, model (10 families), fit pipeline, judge (score + hearing), results, listen, papers (cited), provenance. |
| `songs/index.html` | Song selector: search, collection chips, sort by title/tf/length, detail with mix metrics and per-channel solo parts; URL `#<id>[/ch<n>]`; N/P next/previous. |
| `sounds/index.html` | 495-sound grid: category chips, search, sort by score, four versions per wave, flags; URL `#w<n>`. |
