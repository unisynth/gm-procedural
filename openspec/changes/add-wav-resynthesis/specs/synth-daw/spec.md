## MODIFIED Requirements

### Requirement: GSM samples
The library SHALL list the chunk samples stored in the instruments of project songs (once per distinct sample),
each marked GSM or procedural, with length and size. One SHALL play on its own and load into the selected
instrument's sampler slot.

The DAW SHALL import an audio file (resampled to 44.1 kHz mono, at most 30 s) either as a GSM sample (GSM 06.10,
bit-exact with libgsm) or, with "Make procedural…", as a procedural sample entry, and SHALL report the stored
size. It SHALL remove stored samples no slot plays on request.

A procedural entry SHALL be rendered in the browser with the same generator as the player, so it sounds the same
in the DAW, the bounce and the intro.

#### Scenario: Import a WAV
- **WHEN** the user picks Import sample… on the Lead and chooses a 0.5 s stereo 48 kHz WAV
- **THEN** the Lead stores one GSM sample (about 4.5 KB), a sampler slot plays it, and the status reports its size

#### Scenario: Reuse a sample from another song
- **WHEN** the user plays a GSM sample entry in the library and loads it into the selected track
- **THEN** the sample is copied into that instrument (not duplicated if already stored) and a sampler slot plays it

#### Scenario: Make a WAV procedural
- **WHEN** the user picks "Make procedural…" on a sampler slot and chooses a 2 s looped string WAV
- **THEN** the analysis runs with visible progress, the instrument stores one procedural entry of at most about
  2.1 KB, the slot plays it, and the slot shows "procedural", its size, its score and any failing diagnose checks

#### Scenario: Compare with the original
- **WHEN** a procedural entry has just been made from a WAV
- **THEN** the slot offers playing the original and the generated sound one after the other until the dialog is
  closed. The original WAV is not stored in the song

## ADDED Requirements

### Requirement: Procedural analysis route
The local server SHALL offer `/api/procedural`. It takes a WAV upload with optional kind and loop overrides, runs the
resynthesis tool on it as a fixed binary with an argument list (no shell), and returns the procedural entry, the
score and the diagnose summary through a polled job. It SHALL answer 503 when the tool is not built.

#### Scenario: Progress and result
- **WHEN** the DAW posts a WAV and polls the job
- **THEN** it sees each finished step with its score, and finally the entry bytes, the score and the failing checks

### Requirement: Analysis route limits
The analysis route SHALL apply the rules of the other API routes (loopback, token, foreign Origin refused, 16 MB
body), check the WAV before running anything, run at most one job at a time, stop a job past its time limit, and
delete its temporary files in every case.

#### Scenario: Hostile upload
- **WHEN** a client posts a 20 MB body, a file that is not a WAV, or a request from a foreign Origin
- **THEN** the server refuses it (413, 400 or 403), runs nothing, and leaves no temporary file

#### Scenario: Time limit
- **WHEN** an analysis runs past its time limit
- **THEN** the tool is stopped, its temporary files are removed, and the job reports a timeout
