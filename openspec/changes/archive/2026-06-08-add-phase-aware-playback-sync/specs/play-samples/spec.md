## ADDED Requirements

### Requirement: Quantized Starts Preserve Effective Loop Start
The system SHALL preserve the effective loop-start source frame for all newly triggered
quantized starts.

When trigger quantization is enabled and a loaded pad is scheduled to start or restart at a
transport grid boundary, Rust SHALL use the selected output frame only to decide when the pad
becomes audible. Rust SHALL NOT use transport phase, pad BPM, pad timing metadata, or late-click
catch-up to choose a different initial source frame for normal pad triggers.

If valid pad timing metadata is available, it SHALL remain available for loop-editor grid
anchoring and explicit future sync operations, but it SHALL NOT make a newly triggered pad start
from the middle or end of its loop.

#### Scenario: Quantized one-sixteenth start begins at loop start
- **GIVEN** trigger quantization is enabled with grid step `1/16`
- **AND** a loaded pad has valid BPM and timing-anchor metadata
- **AND** the pad has a configured loop start
- **WHEN** the scheduled event executes at the selected transport grid boundary
- **THEN** Rust starts or restarts the pad at the configured loop-start source frame
- **AND** Rust does not start at a phase-derived source frame inside the loop

#### Scenario: Quantized late subdivision start waits instead of catching up
- **GIVEN** trigger quantization is enabled with grid step `1/16`
- **AND** the human trigger arrives after the nearest previous grid boundary
- **WHEN** Rust schedules the pad start
- **THEN** Rust targets the next future selected-grid boundary
- **AND** the pad starts from the effective loop start at that future output frame

#### Scenario: Missing pad metadata still starts at loop start
- **GIVEN** trigger quantization is enabled
- **AND** a loaded pad lacks valid effective BPM or timing-anchor metadata
- **WHEN** the scheduled event executes
- **THEN** Rust starts or restarts the pad at the existing effective loop-start frame

### Requirement: Immediate Starts Remain Unchanged
The system SHALL preserve existing immediate loop-start behavior when trigger quantization
is disabled.

`AudioEngine.play_sample(id, velocity)` and
`AudioEngine.play_sample_exclusive(id, velocity)` SHALL preserve the existing immediate
loop-start behavior.

Phase-aware source-frame calculation SHALL NOT be applied to immediate triggers unless a
future OpenSpec change explicitly requests that behavior.

#### Scenario: Immediate trigger starts at loop start
- **GIVEN** trigger quantization is disabled
- **AND** a loaded pad has valid timing-anchor metadata
- **WHEN** Python/control code requests playback
- **THEN** Rust starts or restarts playback promptly at the existing effective loop-start frame
- **AND** no transport beat/bar boundary wait is introduced
