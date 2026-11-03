# Spec Delta

## ADDED Requirements

### Requirement: GM bank source
The DAW SHALL offer the GM bank sources "Procedural" (generated in the browser, the default) and "OS bank" (bytes from `/api/bank` or the static `data/bank.dls`, offered only when available). The choice SHALL persist per browser, apply to playback, bounce and the library, and SHALL NOT be saved into the song.

#### Scenario: Default is procedural
- **WHEN** the DAW opens on a machine whose server serves an OS bank
- **THEN** GM tracks play from the generated bank and the source reads "Procedural"

#### Scenario: A/B against the OS bank
- **WHEN** the source is switched to "OS bank" during playback
- **THEN** GM tracks continue from the OS bank without reloading the song, and switching back restores the generated bank

#### Scenario: No OS bank
- **WHEN** `/api/bank` answers 404
- **THEN** "OS bank" is not offered and GM tracks play from the generated bank

#### Scenario: Bounce follows the source
- **WHEN** a song is bounced with "Procedural" selected
- **THEN** the bounce uses the generated bank

## MODIFIED Requirements

### Requirement: Read-only library and bank routes
The server SHALL serve read-only `/api/library` (the catalog of library sources) and `/api/bank` (GM bank bytes from
`GM_DLS` or the OS bank path, 404 when absent) under the same token and Origin rules.

#### Scenario: No bank
- **WHEN** no GM bank exists on the machine
- **THEN** `/api/bank` answers 404 and the DAW keeps GM entries available through the generated bank

### Requirement: GM sampler editor
A sampler slot SHALL offer the sources "GSM sample (in chunk)", "GM.DLS wave" and "GM.DLS instrument", a GM picker that
opens the library on the GM bank, and a key map of the regions that will play (one region over all keys for a single
wave). The DAW SHALL say when no bank is available (the generated bank failed and no OS bank is served) and SHALL say
that Windows production builds play the OS `gm.dls` while other platforms play the generated bank.

#### Scenario: Wave vs instrument
- **WHEN** the source of a slot holding "Piano 1" changes from "GM.DLS instrument" to "GM.DLS wave"
- **THEN** the key map changes from 10 regions to one wave over keys 0–127

#### Scenario: Platform note
- **WHEN** a slot uses a GM.DLS instrument
- **THEN** the inspector states that Windows builds use the OS bank and other platforms the generated bank
