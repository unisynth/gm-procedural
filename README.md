# gm-procedural

A General MIDI sound bank rebuilt from synthesis parameters instead of samples.

Every wave of the Windows `gm.dls` (235 instruments, 495 waves) — and of the gmplus soundfont, a Roland Sound
Canvas–based set — is described by a few hundred numbers for a fixed family of synthesis models (additive, noise,
drum, metal, modal, shaker, FM, formant, Karplus-Strong, subtractive). The generator renders them back into a
`RIFF 'DLS '` bank with the original layout, so any DLS player can use it. The parameters are fitted by measuring and
listening against the original.

**Site:** <https://gm-procedural.exceed.hu/>: the method, the papers, a song selector (70 songs × 4 banks) and all 495 sounds.
Raw listening pages: https://storage.googleapis.com/gm-procedural/index.html

| Path | What |
|---|---|
| `crates/dls` | `no_std` zero-copy DLS1/2 reader; DLS writer behind feature `write` |
| `crates/gmgen` | `no_std + alloc` bank generator: `gmgen::build(None) -> Vec<u8>` (DLS bytes), feature `plus` for the gmplus set |
| `crates/gmfit` | host fitter: `extract`, `analyze`, `fit`, `correct`, `refine`, `bulk`, `render`, `sweep`, `hear`, `report`, `dump`, `bank` |
| `assets/gm`, `assets/gm-plus` | bank structure (no audio) + per-wave parameters + committed scores |
| `tools/gm-songs` | full-song four-way A/B: SMF parser, AUMIDISynth renderer, metrics, page generator, Range server |
| `tools/publish.sh` | uploads pages + renders to the public bucket |
| `tools/site/` | builds `site/data/*.json` from the renders (`bun tools/site/build.ts`) |
| `site/` | GitHub Pages site (`gm-procedural.exceed.hu`): landing page, `songs/`, `sounds/`; audio streams from the bucket |
| `pages/` | hand-made listening pages of the fitting rounds |
| `docs/synth/gm-procedural.md` | formats, render pipeline, families and parameter tables, commands |
| `research/` | instrument research, pilot results |
| `openspec/` | the specs and changes this work is driven by (`add-procedural-gm-bank`, `add-gmplus-bank`) |

## Use as a dependency

```toml
[dependencies]
dls   = { git = "https://github.com/unisynth/gm-procedural", rev = "<sha>" }
gmgen = { git = "https://github.com/unisynth/gm-procedural", rev = "<sha>", default-features = false }
```

```rust
let bank: Vec<u8> = gmgen::build(None);          // whole bank as DLS bytes
let dls = dls::Dls::parse(&bank).unwrap();
```

The bank is deterministic: the same parameters give byte-identical output on every platform. To work on this repo
and a consumer at the same time, override the git source with a local path in the consumer's
`.cargo/config.toml` (`[patch."https://github.com/unisynth/gm-procedural"] gmgen = { path = "../gm-procedural/crates/gmgen" }`, same for `dls`).

## Build and test

```bash
pnpm install --frozen-lockfile && cargo fetch
cargo test --workspace --release && bun test
cargo run --profile fast -p gmfit -- bank -o build/gm/gen.dls
```

Fitting needs the originals on your machine — they are never committed or uploaded:
`GM_DLS=/path/to/gm.dls` (from `C:\Windows\System32\drivers\gm.dls`) and, for the plus set, `GMPLUS_SF2=/path/to/gmplus.sf2`.

## Publish the listening pages

```bash
gcloud auth login
tools/publish.sh --dry-run && tools/publish.sh     # → gs://gm-procedural (public read)
```

## Update the site

```bash
bun tools/gm-songs/run.ts render --force   # songs × 4 banks (after a bank change; see docs)
bun tools/gm-songs/run.ts waves            # per-wave clips
tools/publish.sh                           # audio → gs://gm-procedural
bun tools/site/build.ts                    # site/data/*.json
git commit site/ && git push               # .github/workflows/pages.yml deploys site/
```

## Provenance

`gm.dls` is © Microsoft / Roland; gmplus is a fan soundfont by Distortion Dog. Neither is in this repository. The
committed files hold the bank's structure (names, regions, articulation, loop points) and fitted model parameters,
never sample data — see `assets/gm/NOTICE.md` and `assets/gm-plus/NOTICE.md`. The published renders use public MIDI
sets (credited on every page) and are for comparison only. Code: MIT (`LICENSE`).
