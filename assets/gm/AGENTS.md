# assets/gm

| File | Purpose |
|---|---|
| `NOTICE.md` | Provenance: structure of Windows `gm.dls` + fitted synthesis parameters, no audio; original never committed. See change: add-procedural-gm-bank. |
| `structure.json` | `gm-structure/1` (`gmgen::structure`): 235 instruments (drums, bank, program, name, `art` connections `[src,ctl,dst,trn,scale]`, regions `{key,vel,group,wave,wsmp,art}`), 495 waves `{rate,len,wsmp:[unity,fine,gain(,loopStart,loopLen)]}`. One item per line. Written by `gmfit extract` from `GM_DLS`; `crates/gmfit/tests/structure.rs` checks no-PCM + staleness. See change: add-procedural-gm-bank. |
