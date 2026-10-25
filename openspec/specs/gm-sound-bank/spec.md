# gm-sound-bank Specification

## Purpose

Lets the synth play General MIDI samples from the operating system's own DLS bank at runtime. Windows provides `%SystemRoot%\System32\drivers\gm.dls` and macOS provides `CoreAudio.component/Contents/Resources/gs_instruments.dls`. The intro therefore ships no sample data and never redistributes the licensed Roland bank. Implemented by `crates/dls`.

## Requirements

### Requirement: Zero-copy no_std reader
The bank reader SHALL be `no_std`, SHALL NOT allocate, and SHALL return views (slices) into the caller's file buffer. `Dls::parse(&[u8])` SHALL accept a `RIFF 'DLS '` file (DLS level 1 or 2) and locate `colh`, `lins`, `ptbl` and `wvpl`.

#### Scenario: Valid bank
- **WHEN** a well-formed DLS file is parsed
- **THEN** `instrument_count()` equals the `colh` count and `wave_count()` equals the `ptbl` cue count

#### Scenario: Not a DLS file
- **WHEN** the buffer is empty or is a RIFF of another form type (e.g. `WAVE`)
- **THEN** parsing returns `Error::NotDls`

#### Scenario: Missing section
- **WHEN** one of `colh`, `lins`, `ptbl`, `wvpl` is absent
- **THEN** parsing returns `Error::Missing` naming that chunk

#### Scenario: Builds for the compo target
- **WHEN** the crate is checked for `i686-pc-windows-gnu` with only `core`
- **THEN** it compiles

### Requirement: Never panic on malformed data
Every read SHALL be bounds-checked. Malformed or truncated data SHALL produce `Err` or `None` and SHALL NOT panic or read out of bounds.

#### Scenario: Truncated file
- **WHEN** the RIFF size runs past the end of the buffer
- **THEN** parsing returns `Error::Truncated`

#### Scenario: Bad pool table offset
- **WHEN** a `ptbl` cue points outside the wave pool
- **THEN** `wave(i)` returns `None` for that wave and the other waves still resolve

### Requirement: Offsets come from the file structure
Waves SHALL be located through the `ptbl` cue offsets relative to the `wvpl` data. Fixed file offsets SHALL NOT be used (unlike WaveSabre's hard-coded `0x44602`), so one reader serves both the Windows and the macOS bank.

#### Scenario: Same code reads both OS banks
- **WHEN** the Windows `gm.dls` and the macOS `gs_instruments.dls` are parsed
- **THEN** both yield 235 instruments and 495 readable waves

### Requirement: Instrument lookup
`instruments()` SHALL iterate `ins ` lists and expose program (0..=127), bank MSB/LSB, the drum flag (bank bit 31) and the `INFO/INAM` name with the NUL terminator trimmed. `find(drums, bank, program)` SHALL return the first matching instrument. GS drum kits are addressed as drum flag + bank 0 + program = kit number.

#### Scenario: GM lookup
- **WHEN** `find(false, 0, 0)` is called on an OS bank
- **THEN** Acoustic Grand Piano ("Piano 1") is returned

#### Scenario: Drum kit lookup
- **WHEN** `find(true, 0, 0)` is called on an OS bank
- **THEN** the Standard kit is returned and `region_for(36, 100)` (kick) resolves

#### Scenario: Unknown program
- **WHEN** no instrument matches
- **THEN** `find` returns `None`

### Requirement: Regions and sample parameters
`Instrument::regions()` SHALL iterate `rgn ` and `rgn2` lists and expose the key range, velocity range, key group and pool wave index. `region_for(key, vel)` SHALL return the first region covering both values. `Dls::sampler(&Region)` SHALL return the wave together with the effective `Wsmp` (unity note, fine tune in cents, gain, first loop as `(start, length)`). The region's `wsmp` SHALL override the wave's.

#### Scenario: Region without wsmp
- **WHEN** a region has no `wsmp`
- **THEN** `sampler` returns the wave's own `wsmp`

#### Scenario: Region with wsmp
- **WHEN** a region carries its own `wsmp`
- **THEN** `sampler` returns the region's unity note, tuning and loop

### Requirement: Articulation access
Instruments and regions SHALL expose their `lart`/`lar2` connection blocks (`art1`/`art2`) as `Connection { source, control, destination, transform, scale }`. These are raw DLS units, uninterpreted.

#### Scenario: Instrument and region articulation
- **WHEN** an instrument has a global `lart` and one region has its own
- **THEN** `Instrument::articulation()` yields the global blocks and that region's `articulation()` yields its own blocks, while regions without `lart` yield none

### Requirement: PCM wave decode
`wave(i)` SHALL accept mono PCM at 8-bit unsigned or 16-bit signed and reject other formats with `None`. `Wave::sample(i)` SHALL return a value in `[-1, 1)` (8-bit: `(s-128)/128`, 16-bit: `s/32768`) and 0 when `i` is out of range.

#### Scenario: 8-bit and 16-bit samples
- **WHEN** 8-bit bytes `0x80, 0xFF, 0x00` and 16-bit values `0, -32768, 16384` are read
- **THEN** samples are `0, 127/128, -1` and `0, -1, 0.5`

### Requirement: Cross-OS bank equivalence is wave-level
Songs that must sound the same on Windows and macOS SHALL reference samples by pool wave index together with their own tuning and envelope. Measured: both banks contain the same 495 recordings (per-wave correlation ≥ 0.9994; Windows 16-bit, macOS 8-bit) with identical lengths and loops. Region maps (36 of 235 instruments) and `wsmp` tuning (65 waves) differ between the banks.

#### Scenario: Wave index portability
- **WHEN** wave N is read from each OS bank
- **THEN** both have the same rate, length and loop, and their waveforms correlate at ≥ 0.999

### Requirement: The licensed bank is never shipped or committed
The Roland/Microsoft bank SHALL only be read from the OS at runtime. No copy SHALL be committed to the repository or embedded in the intro. Tests against `gm.dls` SHALL read it from the `GM_DLS` environment variable and skip when it is unset. The macOS bank test SHALL skip when the file is absent.

#### Scenario: CI without the bank
- **WHEN** `cargo test -p dls` runs without `GM_DLS` on a non-macOS host
- **THEN** the real-bank tests skip and the synthetic tests still run

### Requirement: Bank fingerprint tool
`cargo run -p dls --example dump -- <bank>` SHALL print a TSV with one `ins` line (drum flag, bank, program, name, region count, articulation count, key-range→wave map) and one `wave` line (rate, bits, length, unity note, fine tune, loop) per entry. Unreadable waves SHALL be printed as `UNREADABLE`. Two banks can then be diffed independently of bit depth.

#### Scenario: Diff two banks
- **WHEN** dump is run on the Windows and the macOS bank
- **THEN** the outputs list the same instrument keys and can be diffed line by line
