# Spec Delta

## ADDED Requirements

### Requirement: Stereo wave pairs
A bank MAY hold a stereo-pair chunk that names pairs of left and right wave indices. The reader SHALL report the
right-channel partner of a paired wave and nothing for an unpaired one. The writer SHALL emit the chunk when given
pairs, and read → write → read SHALL keep them. A bank without the chunk SHALL read exactly as before.

#### Scenario: Round trip
- **WHEN** a bank with pairs (293, 495) and (150, 496) is written and read back
- **THEN** the partner of 293 is 495, the partner of 150 is 496, and wave 289 has none

### Requirement: Malformed pairs are ignored
A pair naming an index out of range, or partners that differ in rate, length or loop, SHALL be ignored and never cause
a panic.

#### Scenario: Bad pair ignored
- **WHEN** a pair names a right wave whose loop differs from its left wave
- **THEN** that pair is dropped and the bank still parses

### Requirement: Paired banks stay valid for other readers
A bank with pairs SHALL remain a valid DLS for readers that skip unknown chunks. Its regions SHALL play the left
channel.

#### Scenario: Foreign reader
- **WHEN** the generated plus bank is loaded by AUMIDISynth
- **THEN** it loads and plays the left channel of every paired wave
