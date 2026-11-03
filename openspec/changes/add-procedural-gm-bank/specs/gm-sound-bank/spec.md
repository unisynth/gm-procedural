# Spec Delta

## ADDED Requirements

### Requirement: DLS writer
Behind an opt-in `write` feature (requires `alloc`), the crate SHALL serialise a bank description (instruments with drum flag, bank, program, name, articulation; regions with key/velocity range, key group, wave index, optional `wsmp` and articulation; waves as mono 16-bit PCM with rate and `wsmp`) into a `RIFF 'DLS '` buffer with `colh`, `lins`, `ptbl` and `wvpl`. The default build SHALL stay allocation-free.

#### Scenario: Round trip of the real bank
- **WHEN** the Windows `gm.dls` (from `GM_DLS`) is read into a bank description and written back
- **THEN** the written buffer parses, its fingerprint dump equals the original's, and every wave's samples are equal

#### Scenario: Default build unchanged
- **WHEN** the crate is checked for `i686-pc-windows-gnu` without the `write` feature
- **THEN** it compiles with only `core`

## MODIFIED Requirements

### Requirement: The licensed bank is never shipped or committed
The Roland/Microsoft bank file SHALL only be read from the OS at runtime or from `GM_DLS` in tools and tests. No copy of the file or its PCM SHALL be committed to the repository or embedded in the intro. Bounded synthesis-model parameters fitted against it (no sample data) and its structure table (names, regions, articulation, wave rate/length/loop) MAY be committed and shipped, as recorded in `NOTICE.md`. Tests against `gm.dls` SHALL read it from the `GM_DLS` environment variable and skip when it is unset. The macOS bank test SHALL skip when the file is absent.

#### Scenario: CI without the bank
- **WHEN** `cargo test -p dls` runs without `GM_DLS` on a non-macOS host
- **THEN** the real-bank tests skip and the synthetic tests still run

#### Scenario: No PCM committed
- **WHEN** the repository's committed GM assets are inspected
- **THEN** they contain structure and model parameters only, and no file is a DLS or holds the original wave samples
