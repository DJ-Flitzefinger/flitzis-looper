## ADDED Requirements

### Requirement: Transport Phase Supports Explicit Target-Frame Calculations
The system SHALL allow Rust to derive musical transport phase for an arbitrary scheduled
output frame using the Rust-owned transport timeline.

When master BPM is valid, Rust SHALL compute the target frame's bar phase in 4/4 as beats
relative to the transport downbeat anchor. This phase SHALL be available for explicit transport
sync features and diagnostics, but normal newly triggered pad playback SHALL still start from the
pad's effective loop start unless a future OpenSpec change explicitly re-enables source-frame
phase alignment.

If master BPM is missing or invalid, phase-aware target-frame calculation SHALL be
unavailable and quantized playback SHALL fall back to the documented immediate or loop-start
behavior.

#### Scenario: Target frame has a bar phase
- **GIVEN** the output sample rate is 48,000 Hz
- **AND** master BPM is 120
- **AND** the transport downbeat frame is 0
- **WHEN** Rust evaluates target output frame 48,000
- **THEN** the target bar phase is 2 beats into the current bar

#### Scenario: Missing master BPM disables target-frame phase calculation
- **GIVEN** no valid master BPM is stored in the transport
- **WHEN** Rust evaluates a scheduled playback target frame
- **THEN** no transport target-frame phase is produced
- **AND** the audio callback continues without panic, blocking, allocation, disk I/O, logging, or GIL access

### Requirement: Explicit Sync Can Anchor Transport Downbeat From A Playing Pad
The system SHALL support anchoring the Rust transport downbeat to a selected active pad's current
musical phase only through an explicit sync request.

The audio thread SHALL use only bounded, audio-thread-owned state:

- selected anchor pad id,
- current transport output frame,
- transport master BPM,
- anchor pad BPM,
- anchor pad timing anchor,
- anchor pad current playhead frame,
- output sample rate.

If the selected pad is not active or required metadata is unavailable, Rust SHALL leave the
transport downbeat unchanged and SHALL preserve existing BPM-lock tempo matching behavior. Starting,
stopping, pausing, retriggering, unloading, or metadata-updating a pad SHALL NOT invoke this sync
implicitly.

#### Scenario: Explicit request from playing anchor pad sets transport phase
- **GIVEN** the permanent Rust transport has a valid master BPM
- **AND** the selected anchor pad is playing
- **AND** the anchor pad has valid BPM and timing-anchor metadata
- **WHEN** Rust handles a fixed-size request to anchor transport phase from that pad
- **THEN** Rust updates the transport downbeat frame so the transport bar phase matches the pad's current bar phase

#### Scenario: Inactive anchor pad leaves transport phase unchanged
- **GIVEN** the permanent Rust transport has an existing downbeat anchor
- **AND** the selected anchor pad is not currently playing
- **WHEN** Rust handles a request to anchor transport phase from that pad
- **THEN** Rust leaves the transport downbeat frame unchanged
- **AND** BPM lock continues to tempo-match pads by BPM ratio when metadata is available

#### Scenario: Pad stop does not implicitly anchor transport phase
- **GIVEN** pad 1 and pad 2 are playing
- **WHEN** pad 1 is stopped without an explicit transport sync request
- **THEN** Rust leaves the transport downbeat frame unchanged
- **AND** later quantized triggers use the same global transport grid

### Requirement: Phase Calculations Remain Real-Time Safe
Transport phase anchoring and target-frame phase calculation SHALL be real-time safe.

The audio callback SHALL NOT perform disk I/O, Python/GIL access, blocking locks or waits,
logging, heap allocation, neural network inference, real-time stem separation, or
long-running work while computing phase.

#### Scenario: Phase anchoring uses bounded scalar state
- **WHEN** the audio callback handles a transport phase-anchor request
- **THEN** it reads only fixed-size audio-thread-owned state
- **AND** it performs only bounded scalar arithmetic
- **AND** it does not allocate, block, log, touch disk, acquire the GIL, or run analysis
