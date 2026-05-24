## ADDED Requirements

### Requirement: Rust-Owned Global Transport Timeline
The system SHALL maintain a global transport timeline inside the Rust audio engine's
audio-thread-owned state.

The transport timeline SHALL be advanced from the CPAL output callback by the number of
rendered output sample frames. Python/control code MAY request transport state changes, but
SHALL NOT own or directly mutate the audio-thread sample-frame clock.

The transport timeline SHALL store:

- an absolute output sample-frame clock,
- the output sample rate,
- optional validated master BPM,
- a 4/4 bar model with four beats per bar,
- an absolute output-frame downbeat anchor for musical phase.

#### Scenario: Timeline advances by rendered output frames
- **GIVEN** the Rust audio callback begins with `output_frame = F`
- **AND** the callback renders `N` output frames
- **WHEN** rendering completes
- **THEN** the Rust transport timeline's output-frame clock is `F + N`

#### Scenario: Python does not own sample-frame time
- **WHEN** Python sends playback or transport control requests
- **THEN** Rust receives those requests through fixed-size control messages
- **AND** Rust computes audio-thread target frames from its own transport timeline

### Requirement: Sample-Frame Clock Supports Beat And Bar Phase
When master BPM is available, the system SHALL derive beat phase and bar phase from the
Rust-owned output sample-frame clock.

Beat duration SHALL be computed from the output sample rate and master BPM. Bar duration
SHALL be four beats. Beat and bar phase SHALL be anchored to the transport downbeat frame.

If no valid master BPM is available, the system SHALL continue to support immediate
playback, but SHALL NOT claim musical quantization is available.

#### Scenario: Beat phase advances from the sample-frame clock
- **GIVEN** the output sample rate is 48,000 Hz
- **AND** master BPM is 120
- **AND** the transport downbeat frame is 0
- **WHEN** the transport output-frame clock is 24,000
- **THEN** the transport is at beat 1 after the downbeat
- **AND** the bar phase is one beat into a 4-beat bar

#### Scenario: Missing master BPM disables musical quantization
- **GIVEN** no valid master BPM is set
- **WHEN** a quantized trigger is requested
- **THEN** the system rejects quantized scheduling or falls back to documented immediate behavior
- **AND** the audio callback continues without panic, blocking, allocation, disk I/O, or GIL access

### Requirement: Master BPM Is Owned And Validated In Rust
The system SHALL store master BPM in Rust audio-thread-owned transport state.

Master BPM updates SHALL reject non-finite and non-positive values. Invalid master BPM
updates SHALL NOT corrupt existing transport state.

#### Scenario: Valid master BPM updates transport state
- **WHEN** Python/control code sends a valid finite positive master BPM update
- **THEN** the Rust audio thread stores the new master BPM
- **AND** subsequent beat and bar phase calculations use that BPM

#### Scenario: Invalid master BPM is ignored safely
- **WHEN** Python/control code sends NaN, infinity, zero, or a negative master BPM
- **THEN** Rust ignores the invalid value
- **AND** the previous valid transport state remains available

### Requirement: Absolute Output-Frame Scheduler
The system SHALL provide an audio-thread-owned scheduler for playback events targeted to
absolute output sample-frame positions.

Scheduled event execution SHALL be based on Rust's output-frame clock, not wall-clock time
and not Python callback time.

Events due before or at the current callback's first output frame SHALL execute before the
first frame of that callback is rendered. Events targeted inside the current output buffer
SHALL execute at their target frame offset within that buffer.

#### Scenario: Event scheduled inside a callback buffer executes at its frame offset
- **GIVEN** a callback starts at output frame 10,000
- **AND** the callback will render 512 frames
- **AND** a pad-start event is scheduled for output frame 10,128
- **WHEN** the callback renders the buffer
- **THEN** frames 10,000 through 10,127 are rendered before the pad starts
- **AND** the pad starts contributing at frame 10,128

#### Scenario: Late event executes at the current callback start
- **GIVEN** a callback starts at output frame 10,000
- **AND** an event is due at output frame 9,990
- **WHEN** the callback processes scheduled events
- **THEN** the event executes at output frame 10,000
- **AND** the callback does not block, rewind, or panic

### Requirement: Fixed-Capacity Scheduler Behavior
The scheduler SHALL use fixed-capacity storage and SHALL NOT allocate heap memory in the
audio callback.

When multiple events target the same output frame, the scheduler SHALL execute them in a
deterministic stable order based on insertion sequence.

When the scheduler is full, the new scheduled request SHALL be rejected. Existing scheduled
events SHALL NOT be evicted. Currently playing voices SHALL NOT be stopped as a side effect
of rejecting the new request.

#### Scenario: Same-frame events execute in insertion order
- **GIVEN** two events are accepted for the same target output frame
- **WHEN** that frame is reached
- **THEN** the event accepted first executes first
- **AND** the event accepted second executes second

#### Scenario: Scheduler-full rejection preserves current playback
- **GIVEN** the scheduler is at fixed capacity
- **AND** one or more pads are currently playing
- **WHEN** a new quantized trigger request arrives
- **THEN** the new request is rejected
- **AND** existing scheduled events remain scheduled
- **AND** currently playing pads remain playing
- **AND** the audio callback performs no blocking operation, heap allocation, logging, disk I/O, neural inference, or GIL access

### Requirement: Quantized Pad Triggers Use The Rust Transport
The system SHALL support opt-in quantized pad triggers that use the Rust transport timeline and a
fixed musical grid step to choose a nearest-grid launch phase.

The supported trigger quantization states SHALL include disabled/immediate plus fixed grid
subdivisions `1/16`, `1/32`, and `1/64`. These grid steps SHALL use the same 1/64-note unit basis
as the loop editor musical grid, where `1/64` is one sixteenth of a beat in 4/4.

When quantization is disabled, existing immediate trigger behavior SHALL be preserved. When
quantization is enabled, Rust SHALL compute the nearest selected-grid boundary from the current
transport state, the downbeat anchor, and the selected grid step. If the boundary is in the future,
Rust SHALL schedule the trigger at that output frame. If the boundary is in the past, Rust SHALL
execute the trigger at the current callback position and advance the pad's initial sample frame by
the late output-frame offset so the audible loop phase matches that nearest grid boundary.

The default trigger quantization enabled state SHALL be disabled. The default persisted grid step
SHALL be `1/16` and SHALL only affect scheduling after quantization is enabled. Control code MAY
update the effective audio-thread mode through a fixed-size control message. UI/controller
controls MAY be added separately, but the audio thread SHALL own the effective mode used for
scheduling.

The Rust transport SHALL establish or refresh its masterclock from active pad timing metadata when
needed for quantized triggering. The first active pad with valid BPM and timing metadata SHALL be
allowed to establish the masterclock; later quantized triggers SHALL align to that Rust-owned
masterclock rather than to Python UI timing.

#### Scenario: Quantization disabled preserves immediate start
- **GIVEN** trigger quantization is disabled
- **AND** a sample is loaded into a pad
- **WHEN** Python/control code requests pad playback
- **THEN** Rust starts or restarts the pad at the current callback position using existing immediate semantics

#### Scenario: One-sixteenth trigger starts at the future selected subdivision boundary
- **GIVEN** master BPM and downbeat anchor are available
- **AND** trigger quantization is enabled with grid step `1/16`
- **WHEN** a loaded pad is triggered closer to the next 1/16-note boundary than the previous one
- **THEN** Rust schedules the pad start for the next 1/16-note boundary's absolute output frame

#### Scenario: Late human trigger catches up to the nearest grid phase
- **GIVEN** master BPM and downbeat anchor are available
- **AND** trigger quantization is enabled with grid step `1/16`
- **WHEN** a loaded pad is triggered closer to the previous 1/16-note boundary than the next one
- **THEN** Rust starts the pad at the current callback position
- **AND** the pad's initial sample frame is advanced by the output-frame distance from that
  previous 1/16-note boundary to the current callback position

#### Scenario: Minimum grid matches the loop editor grid basis
- **GIVEN** master BPM and downbeat anchor are available
- **AND** trigger quantization is enabled with grid step `1/64`
- **WHEN** a loaded pad is triggered between 1/64-note boundaries
- **THEN** Rust targets the nearest 1/64-note boundary's phase
- **AND** that subdivision uses the same one-sixteenth-of-a-beat basis as the loop editor's
  finest musical grid and snapping step

### Requirement: Beatgrid And Downbeat Metadata Integrates With Transport
The system SHALL integrate pad beatgrid and downbeat metadata with Rust playback timing by
publishing bounded, precomputed timing metadata to the audio thread.

The metadata preparation SHALL happen outside the audio callback. The audio callback SHALL
NOT run beat detection, allocate beatgrid vectors, perform disk I/O, or acquire the Python
GIL.

For per-pad onset and phase anchoring, the system SHALL use the same fallback order as
loop-region defaults:

1. first downbeat when available,
2. otherwise first beat when available,
3. otherwise zero seconds.

Only finite non-negative anchors SHALL be published to the audio thread. Invalid, missing, or
negative metadata SHALL fall back before publication or be clamped to zero by Rust-side validation.

#### Scenario: Downbeat metadata anchors pad phase
- **GIVEN** a pad has analysis metadata with at least one downbeat
- **WHEN** bounded timing metadata is published to Rust
- **THEN** Rust uses the first downbeat as the pad's musical phase anchor for quantized playback

#### Scenario: Missing downbeat falls back to beat then zero
- **GIVEN** a pad has no downbeat metadata
- **WHEN** bounded timing metadata is prepared
- **THEN** the first beat is used when available
- **AND** zero seconds is used when neither beats nor downbeats are available

#### Scenario: Full beat-grid vectors are not published to the callback
- **GIVEN** a pad has a large beat-grid vector
- **WHEN** playback timing metadata is published to Rust
- **THEN** the control message contains bounded timing fields only
- **AND** the audio callback does not allocate or iterate the beat-grid vector

### Requirement: Audio Callback Real-Time Safety Is Preserved
The transport timeline, scheduler, quantized triggering, and beatgrid integration SHALL
preserve audio callback real-time safety.

The audio callback SHALL NOT perform:

- disk I/O,
- Python/GIL access,
- blocking locks or waits,
- logging,
- heavy allocations,
- neural network inference,
- real-time stem separation,
- long-running work.

Heavy work, including analysis and future stem preparation, SHALL happen in background or
control-plane work before immutable audio data or bounded metadata is published to the audio
thread.

#### Scenario: Quantized triggering remains real-time safe
- **WHEN** the audio callback handles transport updates, scheduled events, and quantized pad triggers
- **THEN** it uses only bounded audio-thread-owned data structures
- **AND** it performs no disk I/O, Python/GIL access, blocking operation, logging, heap allocation, neural inference, or real-time stem separation

#### Scenario: Future stems are offline and cached
- **WHEN** a later Gen3 phase adds stem support
- **THEN** stem generation occurs offline or in background workers
- **AND** stem generation is only allowed for pads that are not currently playing
- **AND** the audio callback only mixes already prepared audio buffers
