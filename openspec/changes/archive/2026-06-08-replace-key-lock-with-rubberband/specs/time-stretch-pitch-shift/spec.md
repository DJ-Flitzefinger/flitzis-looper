## MODIFIED Requirements

### Requirement: Real-time Time-stretch And Pitch-shift In Mixer
The system SHALL perform playback-rate and Key Lock processing in real time inside the Rust mixer for each active voice, using bounded per-voice Rust processor state backed by Rubber Band when Key Lock is enabled.

Key Lock disabled SHALL render the active tempo ratio as varispeed playback, so perceived pitch changes with playback speed. Key Lock enabled SHALL keep the same source-frame tempo progression while applying Rubber Band pitch compensation so perceived pitch remains approximately stable.

Rubber Band handle construction, block-size queries, latency queries, runtime dependency discovery, and buffer allocation MUST happen outside callback rendering.

#### Scenario: Mixer produces output using Rubber Band Key Lock
- **GIVEN** a pad is playing a looping sample
- **AND** Key Lock is enabled
- **WHEN** the audio callback renders output
- **THEN** the voice contribution is generated through Rubber Band backed Key Lock processing
- **AND** the callback remains real-time safe

#### Scenario: Key Lock disabled remains varispeed
- **GIVEN** a pad is playing a tonal loop
- **AND** Key Lock is disabled
- **WHEN** the performer increases global Pitch/Speed above `1.00x`
- **THEN** the loop tempo increases
- **AND** the perceived pitch rises with the playback rate

### Requirement: Key Lock Preserves Pitch During Tempo Changes
The system SHALL make Rubber Band the active Key Lock pitch-preservation backend for tempo changes.

This behavior SHALL apply to normal speed changes, BPM-lock tempo ratios, full-mix playback, prepared-stem playback, loop wrapping, active mode toggles, and Multi Loop voice mixing through the same Rust voice timing path.

#### Scenario: Tempo increase with Rubber Band Key Lock keeps pitch stable
- **GIVEN** a pad is playing a tonal loop
- **AND** Key Lock is enabled
- **WHEN** the performer increases global Pitch/Speed above `1.00x`
- **THEN** the loop tempo increases
- **AND** Rubber Band processing keeps the perceived pitch approximately near the original tonal pitch
- **AND** the pad does not need to be stopped or retriggered

#### Scenario: BPM Lock tempo ratio uses the same Rubber Band path
- **GIVEN** BPM Lock is enabled with a valid master BPM
- **AND** a playing pad has valid BPM metadata
- **WHEN** the pad's tempo ratio differs from `1.00x`
- **THEN** Key Lock enabled uses Rubber Band pitch preservation while matching tempo
- **AND** Key Lock disabled renders the same tempo ratio as varispeed repitch

#### Scenario: Prepared stems share Rubber Band Key Lock timing
- **GIVEN** a pad is playing through prepared stems
- **AND** Key Lock is enabled
- **WHEN** the performer changes global Pitch/Speed or BPM Lock changes the pad tempo ratio
- **THEN** every enabled stem component uses the same source-frame playhead and loop wrap as the full mix
- **AND** the combined stem output uses the same Rubber Band Key Lock processing path

### Requirement: Rubber Band Processing Is Bounded In The Callback
The system SHALL use preallocated per-voice Rubber Band state and bounded callback work for Key Lock processing.

The audio callback SHALL NOT allocate heap memory, resize buffers, perform disk I/O, decode audio, log, block, acquire the Python GIL, run neural inference, load plugins, or spin waiting for Rubber Band output. If Rubber Band shifted output is unavailable for part of a callback block, the system SHALL use a deterministic finite fallback and continue rendering without unbounded refill or retrieve loops.

#### Scenario: Callback uses preallocated Rubber Band buffers
- **GIVEN** a voice slot has been constructed before callback rendering
- **AND** Key Lock is enabled
- **WHEN** the performer changes Pitch/Speed during playback
- **THEN** the audio callback reuses preallocated Rubber Band staging and output buffers
- **AND** no heap allocation or buffer resize is required in the callback

#### Scenario: Missing shifted output does not spin
- **GIVEN** a Rubber Band voice has not produced enough shifted frames for the current callback block
- **WHEN** the callback renders the block
- **THEN** the callback fills the missing frames using the documented bounded fallback
- **AND** the callback does not loop unboundedly waiting for Rubber Band output

### Requirement: Rubber Band Latency And Playhead Semantics
The system SHALL document and account for the Rubber Band backend's fixed block size and start delay before declaring the backend ready for user testing.

Playhead telemetry SHALL remain source-frame based. Rubber Band output latency SHALL NOT change loop-region ownership, source-frame wrapping, waveform-editor playhead reporting, trigger quantization, or transport scheduling semantics unless a later focused spec explicitly changes those contracts.

#### Scenario: Latency is documented for the selected backend
- **GIVEN** the Rubber Band backend is initialized for the app output sample rate and channel count
- **WHEN** the backend reports its fixed block size and start delay
- **THEN** the values are recorded in implementation documentation or tests
- **AND** the branch explains whether the first implementation compensates or accepts the output delay

#### Scenario: Playhead remains source-frame based
- **GIVEN** a pad is playing with Key Lock enabled
- **WHEN** the waveform editor receives playhead telemetry
- **THEN** the reported playhead follows the voice's source-frame position
- **AND** Rubber Band output latency does not shift the reported loop position
