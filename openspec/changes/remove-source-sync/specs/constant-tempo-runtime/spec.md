## ADDED Requirements

### Requirement: Constant Tempo Without Source SYNC
The application SHALL expose Multi Loop and BPM Lock without source SYNC,
REALTIME BPM, source-map editor controls or equivalent MIDI/keyboard actions.
It SHALL remove map preparation, selection, storage, runtime adoption and beat
phase correction from the current playback architecture. Neural BPM inference
and its verified CPU/CUDA worker protocol SHALL remain available.

#### Scenario: Accepted constant loops remain available
- **GIVEN** two sources with accurate constant BPM and independently set starts
- **WHEN** the user selects 8 and 64 bars and enables Multi Loop and BPM Lock
- **THEN** ordinary scalar rate and precise musical loop-period behavior remain
- **AND** no source-map job or phase-correction request is created

### Requirement: Current Schemas Without Retired Feature Compatibility
The application SHALL use current project, input and sealed Copy/Pair/Bank schemas
without source-SYNC or map fields. It SHALL NOT add compatibility, migration or
tombstone paths for those retired features. Development SHALL NOT delete Human
samples or settings; the Human prepares a fresh project himself.

#### Scenario: A fresh project has only current timing controls
- **GIVEN** a fresh project
- **WHEN** a source is loaded, analyzed or copied
- **THEN** ordinary scalar BPM, loop and pitch state is available
- **AND** no retired map or policy data is generated

### Requirement: Sample Aligned Grid Display
Scalar grid lines SHALL use the same loaded-frame rounding as physical loop
markers. This display projection SHALL NOT change the stored grid origin, BPM,
analytical beat coordinate or playback period.

#### Scenario: Zoomed grid agrees with a snapped loop marker
- **GIVEN** an analytical grid position between loaded sample frames
- **WHEN** the editor displays that line and sets a marker at that position
- **THEN** both use the same integer loaded frame
- **AND** each later position is evaluated independently from the original grid

### Requirement: Unqualified Phase Control Is Retired
The application SHALL NOT expose the unqualified pad-derived phase-anchor or bootstrap
AudioEngine APIs, corresponding control messages, callback dispatch or feature-only helpers.
It SHALL retain the source-qualified internal BPMLOCK session-bootstrap chain and ordinary
TransportTimeline, sample-frame masterclock, quantization and launch-birth invariants.

#### Scenario: Constant-BPM Native rejects retired phase control
- **WHEN** the reflect-only constant-BPM runtime preflight inspects an extension
- **THEN** unqualified phase-anchor and bootstrap APIs are absent
- **AND** no engine, worker or audio device is created

#### Scenario: Ordinary qualified bootstrap remains
- **GIVEN** a current source-qualified BPMLOCK owner or acknowledged accepted-timing refresh
- **WHEN** its internal pending reference becomes active and valid
- **THEN** the existing one-time bootstrap can complete
- **AND** output-frame time, source playheads and launch ownership are preserved
