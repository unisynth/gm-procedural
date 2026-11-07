# Spec Delta

## Purpose

A procedural GM bank with the Windows gm.dls layout, fitted to the higher-quality `gmplus.sf2` Sound Canvas
soundfont at 32 kHz, stereo where gmplus pans its layers. It also covers the tooling that compares it with gm.dls, gmplus and
the gm.dls-fitted bank.

## ADDED Requirements

### Requirement: Layout compatible with gm.dls
The plus bank SHALL have the same instruments, banks, programs, regions, articulation and wave indices 0–494 as the
procedural gm.dls bank. A song that plays on one bank SHALL therefore select the same instruments, regions and waves
on the other.

#### Scenario: Same region map
- **WHEN** "Piano 1" plays note 60 on the gm bank and on the plus bank
- **THEN** both play wave index 293 through the same region and articulation

#### Scenario: Extra waves are additional
- **WHEN** the plus bank holds right-channel waves
- **THEN** they sit at indices ≥ 495, and no region references them directly

### Requirement: 32 kHz waves
Every wave of the plus bank SHALL be generated at 32 000 Hz. Each wave's length and loop SHALL be whole numbers of
samples at that rate. Played by its region at the target key, a wave SHALL sound at the pitch gmplus gives that note
within 1 cent. gmplus's own tuning is kept: its stretched piano differs from gm.dls by up to 5 cents, and that
difference stays. The fine tune absorbs any residual from rounding the loop length.

#### Scenario: Pitch kept
- **WHEN** wave 293 plays at key 64 through the plus bank, and gmplus plays key 64 on Piano 1
- **THEN** the measured fundamentals differ by less than 1 cent

### Requirement: Targets from the soundfont
The fit tool SHALL derive one target per wave from the soundfont named by `GMPLUS_SF2`. The target is the sample of
the preset zone that plays the wave's key (the unity key within the using region, or the drum key for a kit wave).
It is DC-removed, filtered and resampled to 32 kHz at the note's pitch, then level-matched to the gm.dls wave.

#### Scenario: Piano target
- **WHEN** the target of wave 293 is built
- **THEN** it comes from gmplus preset (0, 0) at that wave's key, and played at that key it matches gmplus's pitch within 1 cent

#### Scenario: Missing soundfont
- **WHEN** `gmfit --set plus render 293` runs without `GMPLUS_SF2`
- **THEN** it exits non-zero and the message names `GMPLUS_SF2`

### Requirement: Envelope motion and headroom in targets
A target SHALL carry the zone's mod-envelope filter sweep and pitch bend through its attack, up to 1 s of played time.
From there both are frozen for the loop. A target peaking above the generator's headroom SHALL be scaled down, and the
cut SHALL be added to the wave's gain so its loudness in the bank is unchanged.

#### Scenario: Piano filter sweep
- **WHEN** the Piano 1 target is built (filter base 7935 cents, +2200 cents from a 4 s mod-envelope decay)
- **THEN** its attack is brighter than its loop, and the loop holds the cutoff the envelope reached at the freeze point

#### Scenario: Headroom moved to the gain
- **WHEN** a level-matched target peaks at 1.4
- **THEN** its samples peak at 0.85 and the map records the dB added to its gain

### Requirement: Target provenance without audio
For each wave, a committed map SHALL record the target's preset, zone, sample name, resampling ratio, fine tune and
gain, and no PCM. A wave with no matching preset or zone SHALL fall back to the gm.dls wave resampled to 32 kHz, and
the map SHALL mark it as a fallback.

#### Scenario: Map without audio
- **WHEN** the map is regenerated
- **THEN** it lists 495 entries, and the repository holds no PCM taken from the soundfont

#### Scenario: Fallback marked
- **WHEN** a wave's instrument has no gmplus zone at its key
- **THEN** its map entry is marked as a fallback and its target is the gm.dls wave at 32 kHz

### Requirement: Separate parameter set
The plus bank's structure, wave parameters, scores and map SHALL live in a tree separate from the gm.dls set. Fitting
either set SHALL never modify the other. The same families, budget (≤ 4 layers, ≤ 512 numbers per wave), validation
and score SHALL apply to both sets.

#### Scenario: Isolation
- **WHEN** `gmfit --set plus bulk --cat piano` runs
- **THEN** no file under the gm.dls set changes

### Requirement: Stereo where gmplus pans layers
A wave whose gmplus zones are panned apart SHALL get a stereo target: left and right channels summed from the zones
with equal-power pan. The left channel SHALL be the wave's target. The right channel SHALL be the target of a partner
wave appended after the 495 skeleton waves, never referenced by a region.

#### Scenario: Panned layers
- **WHEN** a wave's zones are two detuned layers panned −8 % and +8 %
- **THEN** the map lists the wave as stereo with its partner index, and the two targets differ

#### Scenario: Unpanned
- **WHEN** a wave has one zone, or its zones share one pan
- **THEN** the wave stays mono with no partner

### Requirement: Partner waves are fitted
A partner wave SHALL have its own parameters under the same family set and budget, fitted to the right channel and
coherent with its left wave: where the target's channels correlate, it starts from the left wave's parameters and
render seed and is corrected toward the right channel. It SHALL share rate, length and loop with its left wave, and both loops SHALL stay exactly periodic.
Generation SHALL be deterministic, natively and on wasm32. No stereo is designed where gmplus has none.

#### Scenario: Same loop
- **WHEN** the plus bank is generated
- **THEN** every pair has equal rate, length and loop, and both channels repeat exactly across the loop

### Requirement: Stereo stays mono-compatible
For a stereo wave, the left wave alone SHALL score within 0.05 of the generated pair's mid (L+R)/2 against the mono
target, so readers without the pair chunk still play a fitting sound. A pair whose loops correlate below 0.3 while
the target's channels correlate at 0.3 or more SHALL be flagged.

#### Scenario: Mono fallback
- **WHEN** a reader ignores the stereo-pair chunk
- **THEN** it plays the left wave, whose score against the mono target is within 0.05 of the pair's mid

### Requirement: Set selection
The generator SHALL build either the gm.dls set or the plus set, chosen at compile time. The fit tool SHALL accept
`--set gm|plus` on every command, defaulting to `gm`, so existing invocations keep their meaning. Song render and
check SHALL accept `--bank plus`.

#### Scenario: Default unchanged
- **WHEN** `gmfit render 293` runs without `--set`
- **THEN** it renders against gm.dls and writes under the gm.dls build directory as before

### Requirement: Four-way song comparison
The song comparison tool SHALL render every selected song and stem through four banks: original gm.dls, gmplus.sf2,
the generated gm.dls bank and the generated plus bank in stereo. It SHALL use the same renderer, MIDI, gain and length
for each. The page SHALL offer the four renders side by side with synchronised playback.

#### Scenario: Four players
- **WHEN** the page is built after a render
- **THEN** each song row has four players, labelled gm.dls, gmplus, gen-gm and gen-plus, starting at the same position

#### Scenario: Stereo render
- **WHEN** the plus bank holds stereo waves
- **THEN** the channels of the gen-plus render differ

### Requirement: Comparison metrics per source
The comparison SHALL report ΔLUFS, `tf` and octave LTAS for the generated gm bank against gm.dls, for the mid signal
of the generated plus bank against gmplus, and for gmplus against gm.dls as a reference distance.

#### Scenario: Three pairs
- **WHEN** a render finishes
- **THEN** the results hold the three metric pairs for every song
