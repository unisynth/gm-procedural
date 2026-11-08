## ADDED Requirements

### Requirement: WAV targets
The resynthesis tool SHALL accept a WAV file (PCM 8/16/24-bit or float, any rate, mono or stereo) as a target.
Stereo SHALL be mixed to mono, and leading silence below −60 dB re the peak SHALL be trimmed. The target keeps
its own sample rate unless the caller asks for a rate, as the DAW does with 44.1 kHz. A GM wave index SHALL stay a
valid target, and every result for a GM wave (analysis, fit, correction, score, report) SHALL be byte-identical to
the results before WAV targets existed.

#### Scenario: GM waves unchanged
- **WHEN** `gmfit report --check` runs after WAV targets are added
- **THEN** it reports no wave whose score differs from the committed baseline

#### Scenario: Unreadable input
- **WHEN** the input is not a RIFF/WAVE file, is compressed (not PCM or float), is empty, or is longer than 30 s
- **THEN** the tool exits with an error naming the reason and writes nothing

### Requirement: Loop
A target's loop SHALL come from the WAV `smpl` forward loop when it lies inside the data, or from `--loop
start,length`. Otherwise the tool SHALL detect one in the sustain: whole periods (and whole vibrato cycles) for
pitched sounds, a spectrally typical stretch for unpitched ones. A detected loop SHALL be accepted only when the
original, played attack-then-loop, scores at least 0.9 against itself.

#### Scenario: Loop from the file
- **WHEN** a WAV with a `smpl` forward loop 251…9961 is analyzed
- **THEN** the target loop is start 251, length 9710

#### Scenario: Detected loop on a vibrato tone
- **WHEN** a synthetic 220 Hz tone with 5 Hz vibrato and no `smpl` chunk is analyzed
- **THEN** the detected loop length is within 1 sample of a whole number of both periods and vibrato cycles, and the
  loop renders without a `loop_click` flag

### Requirement: One-shot targets
A target with no accepted loop, or one that decays below −40 dB re its peak within its length, SHALL be a one-shot
generated as a whole-wave region.

#### Scenario: Drum hit
- **WHEN** a kick drum that decays to −60 dB within 0.4 s is analyzed
- **THEN** it is a one-shot target with no loop

### Requirement: Automatic classification
The tool SHALL choose the target's prior (kind and model families) from measured pitch strength, harmonicity
(harmonic, stretched or inharmonic), sustain or decay, transient and onset glide, and SHALL print the measurements
and the choice. `--kind <name>` (the GM prior names) SHALL override it. GM waves SHALL keep their GM prior.

#### Scenario: Classifier on known sounds
- **WHEN** the classifier runs on the six GM pilot waves (kick, snare, closed hat, Fingered Bass, Strings, Piano) as
  plain WAVs
- **THEN** it chooses a kind with the same family list as their GM prior: drum for kick and snare, metal for the
  hat, harmonic sustain for bass and strings, stiff-string for the piano

#### Scenario: Override
- **WHEN** `--kind organ` is given for a sound the classifier calls strings
- **THEN** the organ prior is used and the output says the classification was overridden

### Requirement: Parameter files
The pipeline SHALL write a parameter file holding the target's wave meta (rate, length, loop, root note, fine
tune) and gmgen parameters within the same budget as GM waves (4 layers, 512 numbers). The file SHALL have a JSON
form for editing and a binary entry form for chunks, and the two SHALL convert into each other without loss.
Rendering a parameter file SHALL be deterministic and identical on native and wasm builds.

#### Scenario: Round trip
- **WHEN** a parameter file is converted JSON → entry → JSON
- **THEN** the result equals the original file, and both render the same samples bit for bit

#### Scenario: Over budget
- **WHEN** a parameter file holds 513 numbers
- **THEN** conversion and rendering refuse it with an error naming the count

### Requirement: Make a WAV procedural
`gmfit wav make <in.wav>` SHALL run loop, classification, analysis, fit, correction and refine. It SHALL write:
- the parameter file;
- the rendered WAV;
- the original/generated sheet;
- a diagnose report.

It SHALL also print the score and the failing checks. The separate steps (`analyze`, `fit`, `correct`, `refine`,
`render`) SHALL be available on the same files. Each step SHALL keep a result only if it scores at least as well
as its input, as for GM waves.

#### Scenario: Strings sample
- **WHEN** GM wave 405 is exported as a WAV with its loop and `gmfit wav make` runs on it
- **THEN** the score is within 0.02 of the committed GM result for wave 405

#### Scenario: Progress for a caller
- **WHEN** `make` runs with `--progress`
- **THEN** it prints one JSON line per finished step, holding the step name, the score so far and the elapsed
  seconds

### Requirement: Diagnose checks
`gmfit diagnose` SHALL take a GM wave index or two WAVs (original, generated) and report, as JSON and Markdown,
the checks the score averages away: harmonic width, pitch movement, loudness wobble, buzz, the floor between
harmonics, harmonic region levels, rumble, the top octave, the attack time × band grid, and loop groups. Each
check SHALL give the original and generated value, ok or off by a stated tolerance, and a hint naming the analysis
step or parameter that addresses it.

#### Scenario: Known faults are found
- **WHEN** diagnose compares a synthetic tone with (a) in-phase upper harmonics, (b) no floor between harmonics,
  (c) added rumble below f0 and (d) vibrato removed, each against the clean tone
- **THEN** each case flags its own check (buzz, gap floor, rumble, pitch movement) and no other check is off

#### Scenario: Hint
- **WHEN** the buzz check is off
- **THEN** its hint names upper-harmonic phases and `phase_seed`

#### Scenario: Unpitched pair
- **WHEN** the original is a hi-hat
- **THEN** the pitched checks report "n/a" and the band, rumble, top-octave and attack checks run

### Requirement: Difference map and bank view
Diagnose SHALL write a difference-map image: generated − original in dB over time × frequency, masked below the
original's −60 dB floor. `gmfit report` SHALL list each GM wave's failing checks.

#### Scenario: Map of a known fault
- **WHEN** the generated tone lacks the floor between harmonics
- **THEN** the map shows the gaps between the harmonic lines below the original, and the harmonic lines near 0 dB

#### Scenario: Report column
- **WHEN** `gmfit report` runs
- **THEN** `build/gm/report.md` has a column with each wave's failing diagnose checks
