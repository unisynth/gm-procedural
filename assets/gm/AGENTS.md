# assets/gm

| File | Purpose |
|---|---|
| `NOTICE.md` | Provenance: structure of Windows `gm.dls` + fitted synthesis parameters, no audio; original never committed. See change: add-procedural-gm-bank. |
| `scores.json` | Regression baseline: wave → `{score, d_spec, d_env, d_loop, flags, f0_orig, f0_gen}`, one per line. Written by `gmfit report --baseline` / `gmfit fit`; `gmfit report --check` fails on a drop > 0.005. See change: add-procedural-gm-bank. |
| `structure.json` | `gm-structure/1` (`gmgen::structure`): 235 instruments (drums, bank, program, name, `art` connections `[src,ctl,dst,trn,scale]`, regions `{key,vel,group,wave,wsmp,art}`), 495 waves `{rate,len,wsmp:[unity,fine,gain(,loopStart,loopLen)]}`. One item per line. Written by `gmfit extract` from `GM_DLS`; `crates/gmfit/tests/structure.rs` checks no-PCM + staleness. See change: add-procedural-gm-bank. |
| `waves/NNN.json` | Per-wave model parameters (`gmgen::params_json::WaveFile`): `wave`, optional `gain_db`/`pitch`, `layers[{family, region, p, t}]`, `correction`, `placeholder`, `note`. ≤ 4 layers / 256 numbers. Packed by `crates/gmgen/build.rs`. See change: add-procedural-gm-bank. |
