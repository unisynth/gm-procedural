# openspec

OpenSpec spec-driven workflow: proposals/designs/tasks under `changes/`, accepted capability specs under `specs/`. Scaffold new changes with `openspec change new <name>`; every `proposal.md` carries a `## Discipline Skills` line (see root `AGENTS.md`).

| File | Purpose |
|---|---|
| `changes/` | Active changes, one dir each: `proposal.md`, `design.md`, `tasks.md`, `specs/<cap>/spec.md` deltas, `.openspec.yaml`. Now: `add-gmplus-bank`, `add-procedural-gm-bank`, `add-wav-resynthesis`. `changes/archive/` holds archived ones (empty). |
| `config.yaml` | OpenSpec config: `schema: spec-driven`; the context / per-artifact `rules` / `operations` blocks are commented examples only (nothing active). |
| `specs/` | Accepted capability specs. Now: `gm-sound-bank/spec.md`. |
