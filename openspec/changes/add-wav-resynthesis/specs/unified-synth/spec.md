## MODIFIED Requirements

### Requirement: Sample section
The sample section SHALL hold up to 255 entries, each with its own size. An entry SHALL be either a WaveSabre GSM
sample chunk, with no bytes after its compressed data, or a procedural sample entry (see "Procedural sample
entries"). Its size SHALL fit the remaining chunk. Sampler slots SHALL refer to entries by 1-based number. A chunk
whose sample section holds only GSM entries SHALL encode and load byte-identically to before procedural entries
existed.

#### Scenario: Bad entries
- **WHEN** an entry has size 0, a size past the end of the chunk, a bad GSM header, bytes after its compressed data,
  or a procedural entry that does not decode
- **THEN** loading returns an error and does not panic

#### Scenario: Index past the section
- **WHEN** a GSM sampler slot refers to sample 3 and the section holds 1 sample
- **THEN** loading returns an error

#### Scenario: Old chunks unchanged
- **WHEN** every song and patch under `project/music/` is loaded and re-encoded
- **THEN** each chunk's bytes are unchanged

### Requirement: Sampler sources
A sampler slot SHALL play one of three sources:
- a sample from the chunk's sample section (a GSM entry, or a procedural entry);
- one wave of the caller's GM.DLS bank;
- one instrument of that bank.

Chunk-sample and DLS-wave sources are chosen by a 1-based index (0 = none; DLS wave: the legacy `(int)v − 1`
truncation). A DLS-instrument source is chosen by `index = 1 + drums·16384 + bank·128 + program`, so it finds the
same instrument in any bank (Windows `gm.dls`, macOS `gs_instruments.dls`). The source parameter's values SHALL keep
their meaning: a procedural entry is played through the chunk-sample source and adds no source value.

#### Scenario: Source values
- **WHEN** the source parameter reads 0.0, 0.5 and 1.0
- **THEN** they are chunk sample (GSM or procedural), DLS instrument and DLS wave (the first and last mean what they
  meant before the instrument source existed)

#### Scenario: No bank
- **WHEN** a sampler slot selects a GM.DLS wave and no bank is given
- **THEN** that slot is silent and the rest of the voice still sounds

## ADDED Requirements

### Requirement: Procedural sample entries
A procedural sample entry SHALL hold a tag no GSM sample chunk can start with, the wave meta (rate, length, loop,
root note, fine tune) and generator parameters within the generator's budget. Entries SHALL need the module feature
`us-sample-gen`; without it, a chunk holding one SHALL fail to load with `UnsupportedModule`.

#### Scenario: Build without the feature
- **WHEN** a player built without `us-sample-gen` loads a chunk holding a procedural entry
- **THEN** loading fails with `UnsupportedModule` naming the generated-sample module

### Requirement: Caller-rendered entries
The instrument SHALL NOT render procedural entries (no heap). The caller SHALL render every entry the song uses with
the deterministic generator before loading and pass the samples in, as it passes the GM bank. The slot SHALL play
them with the entry's loop and the GSM pitch rule. Without samples for an entry, the slot SHALL be silent while the
rest of the voice sounds.

#### Scenario: Same sound everywhere
- **WHEN** a song with a procedural entry renders natively, in the browser and in the compo player
- **THEN** the entry's rendered samples are bit-identical in all three, and so is the song output wherever the song
  output is already bit-identical for GSM samples

#### Scenario: Entry plays like a sample
- **WHEN** a slot plays a looped procedural entry, and a test hook plays the same rendered samples with the same loop
  through the same patch and note
- **THEN** the two outputs are identical

#### Scenario: Samples not supplied
- **WHEN** a song with a procedural entry is loaded without its rendered samples
- **THEN** the slot that plays it is silent and the rest of the voice sounds
