## ADDED Requirements

### Requirement: Quantized Starts Use Pad Phase Metadata When Available
The system SHALL use bounded pad phase metadata to choose the initial sample frame for
quantized starts when valid metadata is available.

When trigger quantization is enabled and a loaded pad is scheduled to start or restart at a
transport grid boundary, Rust SHALL use the targeted grid output frame plus bounded per-pad
timing metadata to choose the pad's initial sample frame.

Rust SHALL compute the phase-aware initial sample frame from:

- scheduled target output frame,
- transport master BPM and bar phase,
- pad effective BPM,
- pad phase anchor,
- active loop region or full sample region.

If the nearest targeted grid output frame is before the actual execution frame, Rust SHALL advance
the phase-aware initial sample frame by the late output-frame offset, scaled by the pad's current
playback tempo ratio, before wrapping it into the active loop region.

If valid phase data is unavailable, Rust SHALL start or restart at the existing effective
loop-start frame.

#### Scenario: Quantized one-sixteenth start begins at the pad phase anchor
- **GIVEN** trigger quantization is enabled with grid step `1/16`
- **AND** a loaded pad has valid BPM and phase-anchor metadata
- **AND** the targeted grid output frame is a transport bar boundary
- **WHEN** the scheduled event executes
- **THEN** Rust starts or restarts the pad at the frame corresponding to the pad's bar-phase anchor within the active loop region

#### Scenario: Quantized late subdivision start catches up
- **GIVEN** trigger quantization is enabled with grid step `1/16`
- **AND** the targeted grid output frame is 240 frames before the actual execution frame
- **AND** a loaded pad has valid BPM and phase-anchor metadata
- **WHEN** the scheduled event executes
- **THEN** Rust starts or restarts the pad at the phase-aware initial sample frame plus 240 output
  frames scaled by the pad's current playback tempo ratio

#### Scenario: Missing pad metadata falls back to loop start
- **GIVEN** trigger quantization is enabled
- **AND** a loaded pad lacks valid effective BPM or phase-anchor metadata
- **WHEN** the scheduled event executes
- **THEN** Rust starts or restarts the pad at the existing effective loop-start frame

### Requirement: Immediate Starts Remain Unchanged
The system SHALL preserve existing immediate loop-start behavior when trigger quantization
is disabled.

`AudioEngine.play_sample(id, velocity)` and
`AudioEngine.play_sample_exclusive(id, velocity)` SHALL preserve the existing immediate
loop-start behavior.

Phase-aware start-frame calculation SHALL NOT be applied to immediate triggers unless a
future OpenSpec change explicitly requests that behavior.

#### Scenario: Immediate trigger starts at loop start
- **GIVEN** trigger quantization is disabled
- **AND** a loaded pad has valid phase-anchor metadata
- **WHEN** Python/control code requests playback
- **THEN** Rust starts or restarts playback promptly at the existing effective loop-start frame
- **AND** no transport beat/bar boundary wait is introduced
