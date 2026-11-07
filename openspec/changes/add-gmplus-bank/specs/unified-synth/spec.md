# Spec Delta

## ADDED Requirements

### Requirement: Stereo DLS waves
With the stereo-sampler feature, a sampler slot playing a DLS wave that has a stereo partner SHALL play the left wave
on the left output and the partner on the right. Both channels share one position, loop and interpolation, and the
slot's left and right pan gains apply to them as they do to a mono wave.

#### Scenario: Paired wave
- **WHEN** a DLS-instrument slot plays a region whose wave has a partner, with pan centred
- **THEN** the left output follows the left wave and the right output follows the partner, sample-aligned

### Requirement: Mono playback without pairs or the feature
An unpaired wave, or any wave in a build without the stereo-sampler feature, SHALL play mono exactly as before. A
build without the feature SHALL not contain the stereo-pair code.

#### Scenario: Golden unchanged
- **WHEN** the existing goldens play on a bank without pairs
- **THEN** their hashes are unchanged, with the feature on or off
