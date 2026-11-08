# gm-procedural

Procedural General MIDI bank: Windows `gm.dls` / gmplus waves rebuilt from fitted synthesis parameters.
Consumed by 64k's unisynth (`dls`, `gmgen` as cargo git dependencies). Overview: `README.md`; design and formats:
`docs/synth/gm-procedural.md`.

Stack: **cargo** (nightly, workspace `crates/*`), **pnpm** owns JS deps, **bun** runs `tools/` scripts and tests.

## Rules

- Never commit or upload `gm.dls`, `gmplus.sf2`, MIDI or renders. Originals come from `GM_DLS` / `GMPLUS_SF2`.
- `gmgen` output is deterministic (`crates/gmgen/clippy.toml` bans f32 transcendentals and `mul_add`); a change that
  alters bank bytes must say so. Check with `gmfit bank` hash and `gmfit report --check`.
- Fit work: follow `.pi/skills/gmfit/SKILL.md` (keep only improvements, re-baseline `scores.json`).
- Spec-driven: changes live in `openspec/changes/<name>/`.
- Keep this file lean; per-file detail goes in the directory `AGENTS.md` files.

## Commands

```bash
pnpm install --frozen-lockfile && cargo fetch     # install
cargo test --workspace --release && bun test      # test
tools/publish.sh                                  # listening pages → gs://gm-procedural
```
