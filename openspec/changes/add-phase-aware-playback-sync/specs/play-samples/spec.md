## ADDED Requirements

### Requirement: Quantized Starts Use Pad Phase Metadata When Available
When trigger quantization is enabled and a loaded pad is scheduled to start or restart at a
transport beat/bar boundary, Rust SHALL use the scheduled target output frame plus bounded
per-pad timing metadata to choose the pad's initial sample frame when valid metadata is
available.

Rust SHALL compute the phase-aware initial sample frame from:

- scheduled target output frame,
- transport master BPM and bar phase,
- pad effective BPM,
- pad phase anchor,
- active loop region or full sample region.

If valid phase data is unavailable, Rust SHALL start or restart at the existing effective
loop-start frame.

#### Scenario: Quantized next-bar start begins at the pad phase anchor
- **GIVEN** trigger quantization is set to next bar
- **AND** a loaded pad has valid BPM and phase-anchor metadata
- **AND** the scheduled target output frame is a transport bar boundary
- **WHEN** the scheduled event executes
- **THEN** Rust starts or restarts the pad at the frame corresponding to the pad's bar-phase anchor within the active loop region

#### Scenario: Quantized next-beat start uses matching beat phase
- **GIVEN** trigger quantization is set to next beat
- **AND** the scheduled target output frame is beat 2 within the transport bar
- **AND** a loaded pad has valid BPM and phase-anchor metadata
- **WHEN** the scheduled event executes
- **THEN** Rust starts or restarts the pad at the frame corresponding to beat 2 within the pad's bar phase

#### Scenario: Missing pad metadata falls back to loop start
- **GIVEN** trigger quantization is enabled
- **AND** a loaded pad lacks valid effective BPM or phase-anchor metadata
- **WHEN** the scheduled event executes
- **THEN** Rust starts or restarts the pad at the existing effective loop-start frame

### Requirement: Immediate Starts Remain Unchanged
When trigger quantization is disabled, `AudioEngine.play_sample(id, velocity)` and
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
