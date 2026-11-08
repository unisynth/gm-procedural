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

## Layout

| Path | What |
|---|---|
| `Cargo.toml` / `Cargo.lock` | Cargo workspace (`crates/*`); lockfile committed |
| `rust-toolchain.toml` | Pins nightly + `rust-src` |
| `package.json` / `pnpm-lock.yaml` | tools deps (spessasynth, typescript); `test` = `bun test` |
| `.gitignore` | `target/ node_modules/ build/`, originals + renders (`*.dls/sf2/mid/wav/mp3`), wasm, `.pi` kb db |
| `LICENSE` | MIT |
| `.pi/` | pi config: worktree hook, prompts, skills (gmfit) → `.pi/AGENTS.md` |
| `assets/` | Fitted bank params, scores, structure (no audio) → `assets/gm*/AGENTS.md` |
| `crates/` | Rust workspace: dls, gmgen, gmgen-wasm, gmfit → `crates/AGENTS.md` |
| `docs/` | Design and format docs (`docs/synth/gm-procedural.md`) |
| `openspec/` | Spec-driven changes and specs → `openspec/AGENTS.md` |
| `pages/` | Hand-made listening pages, published by `tools/publish.sh` |
| `research/` | Research notes and write-ups |
| `site/` | GitHub Pages site → `site/AGENTS.md` |
| `tools/` | bun scripts: song A/B, site builder, spessa renders, publish → `tools/AGENTS.md` |
| `build/` | Git-ignored scratch: renders, reports, site audio |

## Code Instructions

Behavioral guidelines to reduce common coding mistakes. Bias toward caution over
speed. For trivial tasks, use judgment.

### 1. Think Before Coding

- State assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them — don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- Never speculate about code you have not opened. Consult the doc tree
  (`kb agents <path>` / `kb_search`) first, then read the specific file.
- Before any major change, confirm the plan.

### 2. Simplicity First

- Minimum code that solves the problem. Nothing speculative.
- No features beyond what was asked; no abstractions for single-use code.
- No error handling for impossible scenarios.
- DRY: extract a shared helper when a pattern repeats; don't pre-extract for one call site.

### 3. Surgical Changes

- Touch only what you must. Match existing style.
- Don't refactor things that aren't broken.
- Remove imports/variables your changes made unused; leave pre-existing dead code (mention it).
- Every changed line traces directly to the request.

### 4. Goal-Driven Execution (TDD)

- Turn tasks into verifiable goals with explicit success criteria.
- Write or update tests first, verify they fail, then write the minimal implementation to pass.
- For multi-step tasks, state a brief plan with a verify step each.

### 5. Communication

- Give a high-level explanation of what changed — don't dump diffs without summary.

## OpenSpec

This project uses OpenSpec for spec-driven changes. Place change artifacts at
`openspec/changes/<name>/`. Prefer `openspec change new <name>` to scaffold.

When authoring a proposal, add a `## Discipline Skills` line to `proposal.md`
naming the `eng-disciplines` skills its tasks will trigger (mapped via the
checkpoint table below); omit only when none apply. This needs no edit to any
openspec skill — the implement loop reads the proposal artifact unchanged, so
the named skills enter its context and get invoked.

## Discipline Skills

During implementation, invoke the matching `eng-disciplines` skill when a task
signal appears. Skills auto-trigger on natural language, but the implement loop
may never utter the phrase — this table makes the mapping explicit (signals are
observable in the diff / `tasks.md`, not vague intent):

| Task signal (in diff / tasks.md) | Skill |
|---|---|
| touches auth, untrusted input, secrets, webhooks, PII | `security-hardening` |
| spec has a latency/throughput budget, or a large-data / high-traffic path | `performance-optimization` |
| new endpoint, job, external call, or "can't tell what happened in prod" | `observability-instrumentation` |
| non-trivial/irreversible step (migration, public API, cross-boundary) BEFORE it stands | `doubt-driven-review` |
| a bug surfaces mid-implementation | `systematic-debugging` |
| runtime state opaque, `console.log` insufficient | `node-inspect-debugger` |
| feature works + tests pass but the implementation feels heavy | `code-simplification` |

The end gates (`code-review`, `code-quality`) remain unchanged and run at completion before commit.

<!-- dox-doctrine -->

## DOX doctrine

Per-turn DOX doctrine — the kb-first READ discipline and the directory `AGENTS.md`
WRITE discipline — is injected by the `pi-dashboard-kb-extension`. Tune it in
`.pi/dashboard/knowledge_base.json` under the `doctrine` key (`inject`, `write`).
