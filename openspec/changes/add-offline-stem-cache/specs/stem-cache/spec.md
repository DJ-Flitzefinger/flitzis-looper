## ADDED Requirements

### Requirement: Stem Generation Is Offline And Cached
The system SHALL generate stem sets only outside the real-time audio callback and store the
results as project-local cached artifacts.

Stem generation SHALL be modeled as offline/background work. It MAY use disk I/O,
temporary files, neural inference, and heavy allocation outside the callback, but the audio
callback SHALL NOT run stem separation or access generation internals.

The initial stem set SHALL include vocals, melody, bass, drums, and instrumental stems.

#### Scenario: Stems are generated for a stopped loaded pad
- **GIVEN** a pad has loaded source audio
- **AND** the pad is not currently playing
- **WHEN** the performer requests stem generation
- **THEN** the system schedules offline/background stem generation for that pad
- **AND** generated stem artifacts are associated with that pad's current source version

#### Scenario: Real-time stem separation is not allowed
- **GIVEN** a pad is playing
- **WHEN** the audio callback renders the pad
- **THEN** the callback does not run stem separation, neural inference, disk I/O, logging, blocking waits, heap allocation, or Python/GIL access

### Requirement: Stem Generation And Replacement Require An Inactive Pad
The system SHALL allow stem generation and stem-buffer replacement only when the target pad
is not currently playing.

If a pad starts playing while stem generation is in progress, the completed result SHALL
NOT replace audio-thread stem buffers for that pad until the pad is stopped and the source
version is still current.

#### Scenario: Playing pad blocks generation
- **GIVEN** a pad is currently playing
- **WHEN** the performer requests stem generation for that pad
- **THEN** the system rejects or defers the request
- **AND** no stem generation work is run for that active pad in the audio callback

#### Scenario: Pad starts during generation
- **GIVEN** stem generation is running for a pad that was inactive when the task started
- **WHEN** the pad starts playing before generation completes
- **THEN** the generated buffers are not published as active stem buffers for that playing pad
- **AND** current full-mix playback continues unchanged

### Requirement: Stem Cache Matches The Loaded Source Version
The system SHALL associate cached stems with the exact loaded source version for the pad.

Replacing or unloading the pad source SHALL make previously cached stems unavailable for
playback until a matching valid stem set is generated or restored for the new source
version.

#### Scenario: Replacing a source invalidates old stems
- **GIVEN** a pad has cached stems for source version A
- **WHEN** the pad is loaded with source version B
- **THEN** stems generated for source version A are not eligible for playback on that pad

#### Scenario: Missing cache files degrade safely
- **GIVEN** project state indicates cached stems may exist
- **AND** one or more stem cache files are missing
- **WHEN** the project or pad state is restored
- **THEN** the pad remains usable with full-mix playback
- **AND** missing stem files do not crash load, unload, or playback

### Requirement: Prepared Stem Buffers Are Aligned For Playback
The system SHALL prepare immutable stem buffers that are aligned with the pad's full-mix
buffer before they are eligible for audio-thread publication.

Prepared stem buffers SHALL use the mixer output sample rate and channel layout, share the
same frame origin as the full mix, and provide frame positions compatible with the pad's
existing loop-region and voice playhead math.

#### Scenario: Prepared stems share the full-mix frame origin
- **GIVEN** stem generation completes for a loaded pad
- **WHEN** the generated audio is prepared for Rust publication
- **THEN** every accepted stem buffer uses the same sample-frame origin as the pad's full-mix buffer
- **AND** the buffers are suitable for synchronized loop playback using the existing voice playhead

### Requirement: Stem Availability Degrades To Full-Mix Playback
The system SHALL preserve existing full-mix playback when stems are unavailable, stale,
incomplete, failed, or disabled.

Stem cache or publication failure SHALL NOT stop currently playing pads, evict scheduled
events, corrupt full-mix sample buffers, or require special recovery from the performer.

#### Scenario: Stem generation fails
- **GIVEN** a pad has loaded full-mix audio
- **WHEN** stem generation fails for that pad
- **THEN** the system reports the failure outside the audio callback
- **AND** the pad remains playable using the existing full-mix buffer
