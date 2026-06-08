## ADDED Requirements

### Requirement: Prepared Stems Share Pad Playback Timing
The system SHALL mix prepared stem buffers using the same pad voice timing as full-mix
sample playback.

When a pad voice uses prepared stems, stem reads SHALL share the voice playhead, loop
region, trigger timing, transport phase, speed multiplier, BPM-lock behavior, and key-lock
processing path that would apply to the full-mix buffer.

#### Scenario: Stem playback stays synchronized with the loop
- **GIVEN** a pad has prepared stems and a configured loop region
- **WHEN** the pad voice plays through the loop region
- **THEN** all enabled stem buffers are read from the same loop-relative sample position
- **AND** stems remain synchronized with the pad's full-mix timing

### Requirement: Stem Mixing Falls Back To Full Mix
The system SHALL preserve full-mix playback when prepared stems are missing, stale,
incomplete, failed, rejected, or disabled.

The audio callback SHALL NOT stop full-mix playback as a side effect of missing stem data.

#### Scenario: Missing stem set uses full mix
- **GIVEN** a pad has loaded full-mix audio
- **AND** no valid prepared stem set is available
- **WHEN** the pad is triggered
- **THEN** playback uses the existing full-mix buffer
- **AND** the trigger follows the existing immediate or quantized playback behavior

### Requirement: Stem Mix State Is Bounded Audio-Thread State
The system SHALL represent future stem mute, solo, toggle, or revert-to-full-mix state as
bounded audio-thread-owned state updated through fixed-size control messages.

The audio callback SHALL NOT generate stems, read stem cache files, decode stem files,
allocate stem buffers, run neural inference, log, block, or acquire the Python GIL in
response to stem mix state changes.

#### Scenario: Future stem toggle does not generate stems in callback
- **GIVEN** a pad has prepared stem buffers already published to Rust
- **WHEN** a future stem toggle request reaches the audio callback
- **THEN** the callback updates bounded mix state only
- **AND** it does not run generation, decoding, disk I/O, allocation, logging, blocking, neural inference, or Python/GIL access
