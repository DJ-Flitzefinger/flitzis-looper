# play-samples Specification

## Purpose
To enable real-time-safe triggering, looping playback, and mixing of previously loaded samples by ID (with velocity) from Python, with bounded polyphony to avoid allocations in the audio callback.
## Requirements
### Requirement: Trigger Sample Playback By ID
The system SHALL provide a Python API to trigger playback of a previously loaded sample by integer `id` in the range `0..NUM_SAMPLES`, with a floating-point `velocity` in the range 0.0 to 1.0.

Triggered playback SHALL loop continuously until stopped via `AudioEngine.stop_sample(id)` (or the sample is unloaded; see `load-audio-files`).

If a loop region is configured for `id` (see `loop-region`), playback SHALL start at the loop start and SHALL loop over the configured loop region.

If no loop region is configured, playback SHALL start at the start of the sample buffer and SHALL loop over the full sample buffer.

#### Scenario: Triggered sample contributes to audio output
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **THEN** the sample begins playback in the audio callback
- **AND** the rendered output buffer is not forced to silence

#### Scenario: Triggered sample loops continuously
- **WHEN** a sample is loaded into slot `id`
- **AND** `AudioEngine.play_sample(id, velocity)` is called
- **AND** playback reaches the end of the active loop region (or end of sample if no region)
- **THEN** playback continues from the loop start (or sample start if no region) without requiring a new trigger

#### Scenario: Sample id is out of range
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called with `id >= NUM_SAMPLES`
- **THEN** the call fails with a Python exception
- **AND** no playback is triggered

#### Scenario: Missing sample ID is handled safely
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called for an `id` with no loaded sample
- **THEN** the trigger is ignored (or dropped)
- **AND** the audio callback continues without panic or blocking

### Requirement: Fixed-Capacity Voice Mixing
The system SHALL mix sample playback using a fixed-capacity voice list with `MAX_VOICES = 32` to avoid heap allocations in the real-time audio callback.

#### Scenario: Polyphony limit is enforced
- **WHEN** more than 32 voices are triggered in a short interval
- **THEN** additional triggers are dropped (or replaced) deterministically
- **AND** the audio callback continues without allocating memory

### Requirement: Real-Time Safety During Playback
The system SHALL ensure that triggering playback and mixing audio in the real-time callback performs no blocking operations and no heap allocations.

#### Scenario: Playback processing stays real-time safe
- **WHEN** the audio callback drains `PlaySample` messages and renders audio
- **THEN** no blocking operations are performed
- **AND** no heap allocations occur

### Requirement: Stop Sample Playback By ID
The system SHALL provide a Python API to stop playback of a previously triggered sample by integer `id` in the range `0..NUM_SAMPLES`.

#### Scenario: Stop ends active voices for the sample id
- **WHEN** a sample is playing due to one or more prior `play_sample(id, ...)` calls
- **AND** `AudioEngine.stop_sample(id)` is called
- **THEN** all currently active voices for `id` stop contributing to the audio output

#### Scenario: Stop sample id is out of range
- **WHEN** `AudioEngine.stop_sample(id)` is called with `id >= NUM_SAMPLES`
- **THEN** the call fails with a Python exception

#### Scenario: Stop missing sample id is handled safely
- **WHEN** `AudioEngine.stop_sample(id)` is called for an `id` with no loaded sample
- **THEN** the stop request is ignored (or dropped)
- **AND** the audio callback continues without panic or blocking

### Requirement: Stop All Sample Playback
The system SHALL provide a Python API `AudioEngine.stop_all()` that stops all currently active voices, regardless of sample ID.

#### Scenario: Stop-all ends all active voices
- **WHEN** one or more samples are playing due to prior `play_sample(...)` calls
- **AND** `AudioEngine.stop_all()` is called
- **THEN** all currently active voices stop contributing to the audio output

#### Scenario: Stop-all is safe when nothing is playing
- **WHEN** no samples are currently playing
- **AND** `AudioEngine.stop_all()` is called
- **THEN** the call succeeds

#### Scenario: Stop-all before engine initialization fails
- **WHEN** an `AudioEngine` has not been initialized via `run()`
- **AND** `AudioEngine.stop_all()` is called
- **THEN** the call fails with a Python exception

### Requirement: Set Global Speed Multiplier (Control Plane)
The system SHALL provide a Python API `AudioEngine.set_speed(speed)` to set a global speed multiplier for the audio engine.

The speed multiplier MUST be a finite floating-point value in the range 0.5×..2.0×. The default speed multiplier MUST be 1.0×.

Calling `AudioEngine.set_speed(...)` SHALL enqueue a control message for the audio thread to update the audio thread’s stored global speed value.

#### Scenario: Setting speed enqueues a speed update
- **GIVEN** an `AudioEngine` has been initialized via `run()`
- **WHEN** Python calls `AudioEngine.set_speed(1.25)`
- **THEN** the call succeeds
- **AND** a speed update is enqueued for the audio thread

#### Scenario: Resetting speed sets the stored value back to default
- **GIVEN** an `AudioEngine` has been initialized via `run()`
- **AND** the stored speed multiplier is not 1.0×
- **WHEN** Python calls `AudioEngine.set_speed(1.0)`
- **THEN** the call succeeds
- **AND** the stored speed multiplier becomes 1.0×

#### Scenario: Invalid speed is rejected
- **WHEN** Python attempts to call `AudioEngine.set_speed(...)` with a non-finite value (NaN/Inf)
- **THEN** the call fails with a Python exception
- **WHEN** Python attempts to call `AudioEngine.set_speed(...)` outside 0.5×..2.0×
- **THEN** the call fails with a Python exception

### Requirement: Speed Updates Are Performance-Friendly
The system SHALL treat frequent speed updates as a best-effort control: if the control ring buffer is full, the speed update SHALL be dropped without raising an exception to Python.

#### Scenario: Speed update is dropped when message buffer is full
- **GIVEN** the control ring buffer is full
- **WHEN** Python calls `AudioEngine.set_speed(1.25)`
- **THEN** the call succeeds without blocking
- **AND** the speed update is dropped

### Requirement: Global Speed Multiplier Changes Playback Rate
The system SHALL apply the stored global speed multiplier to playback such that changing `AudioEngine.set_speed(speed)` changes audible playback rate for all active voices.

#### Scenario: Speed change is audible for active pads
- **GIVEN** a pad is playing
- **WHEN** Python calls `AudioEngine.set_speed(0.8)`
- **THEN** the pad’s playback tempo slows down audibly

### Requirement: Lock Mode Changes Affect Playback
The system SHALL propagate BPM lock and Key lock state changes from Python to the audio engine so that these modes affect playback.

#### Scenario: Enabling key lock affects audio processing mode
- **GIVEN** a pad is playing
- **WHEN** the performer enables Key lock
- **THEN** the audio engine updates its processing mode without stopping playback

### Requirement: Per-pad BPM Metadata Is Available To The Audio Engine
The system SHALL publish per-pad effective BPM metadata to the audio engine so it can tempo-match pads when BPM lock is enabled.

#### Scenario: Audio engine receives BPM updates
- **GIVEN** a pad has updated effective BPM metadata
- **WHEN** the controller publishes metadata to the audio engine
- **THEN** subsequent playback processing uses that BPM when BPM lock is enabled


<!-- Added from add-offline-stem-cache -->
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


<!-- Added from add-offline-stem-cache -->
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


<!-- Added from add-offline-stem-cache -->
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


<!-- Added from add-phase-aware-playback-sync -->
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


<!-- Added from add-phase-aware-playback-sync -->
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


<!-- Added from add-rust-transport-timeline -->
### Requirement: Immediate Trigger Compatibility Through Transport Scheduler
The system SHALL preserve existing immediate sample trigger behavior when trigger
quantization is disabled.

The implementation MAY internally route immediate triggers through the transport scheduler
with a target equal to the current output frame, or it MAY keep an immediate fast path. In
both cases, `AudioEngine.play_sample(id, velocity)` SHALL retain the current observable
behavior unless the user has explicitly enabled quantized triggering through future controls.

#### Scenario: Existing play_sample remains immediate by default
- **GIVEN** trigger quantization is disabled
- **AND** a sample is loaded into slot `id`
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called
- **THEN** playback starts or restarts using the current immediate trigger behavior
- **AND** no beat or bar boundary wait is introduced

#### Scenario: Missing sample remains safe under the transport path
- **GIVEN** no sample is loaded into slot `id`
- **WHEN** `AudioEngine.play_sample(id, velocity)` is called with quantization disabled
- **THEN** the trigger is ignored or dropped safely
- **AND** the audio callback continues without panic or blocking


<!-- Added from add-rust-transport-timeline -->
### Requirement: Quantized Triggers Preserve Source Loop Start
The system SHALL start every newly triggered quantized pad from that pad's effective loop
start source frame.

Quantized trigger scheduling SHALL only change the absolute output frame where the pad becomes
audible. It SHALL NOT change the initial source frame, seek into the middle of the loop, seek near
the loop end, or apply late-click catch-up inside the source loop.

If a loop region is configured for the pad, the effective loop start SHALL be the configured loop
start. If no loop region is configured, the effective loop start SHALL be sample frame zero.

#### Scenario: Quantized play starts from configured loop start
- **GIVEN** trigger quantization is enabled
- **AND** a loaded pad has a configured loop region starting at source frame 9,600
- **WHEN** the pad trigger is scheduled on the next transport grid boundary
- **THEN** the pad becomes audible at that output frame
- **AND** playback starts from source frame 9,600

#### Scenario: Late click does not seek into the loop
- **GIVEN** trigger quantization is enabled
- **AND** the human trigger arrives after the nearest previous grid boundary
- **WHEN** Rust schedules the trigger
- **THEN** Rust targets the next future grid boundary
- **AND** the initial source frame remains the effective loop start


<!-- Added from add-rust-transport-timeline -->
### Requirement: Quantized Trigger Failure Does Not Partially Change Playback
The system SHALL reject a quantized trigger request without applying partial playback
changes when the fixed-capacity scheduler cannot accept the request.

For transitions that would stop one or more currently playing pads and start another pad at
a quantized boundary, scheduler rejection SHALL leave currently playing pads unchanged.

#### Scenario: Scheduler-full quantized start leaves playback unchanged
- **GIVEN** trigger quantization is enabled
- **AND** the scheduler is full
- **AND** pad 1 is currently playing
- **WHEN** pad 2 is triggered
- **THEN** the pad 2 start is rejected
- **AND** pad 1 remains playing
- **AND** no partial stop/start transition is applied


<!-- Added from add-rust-transport-timeline -->
### Requirement: Exclusive Sample Trigger API
The system SHALL provide a Python API `AudioEngine.play_sample_exclusive(id, velocity)` that
requests one audio-thread command to stop all active voices and start the requested loaded
sample.

When trigger quantization is disabled, `AudioEngine.play_sample_exclusive(id, velocity)`
SHALL preserve the existing one-at-a-time behavior of stopping currently active voices and
starting the requested sample promptly.

When trigger quantization is enabled, Rust SHALL schedule the stop-all operation and the
requested sample start as one fixed-size scheduled command at one absolute output frame.

#### Scenario: Exclusive trigger is one fixed-size request
- **WHEN** Python/control code requests exclusive playback for a loaded pad
- **THEN** the request is sent to the audio thread as one fixed-size control message
- **AND** the audio thread represents the stop-all-then-play transition as one scheduled command

#### Scenario: Exclusive trigger does not stop pads when the target cannot play
- **GIVEN** pad 1 is currently playing
- **AND** pad 2 has no loaded sample
- **WHEN** exclusive playback is requested for pad 2
- **THEN** pad 1 remains playing
- **AND** no partial stop/start transition is applied


<!-- Added from add-stem-performance-controls -->
### Requirement: Stem Mix Controls Preserve Pad Voice Timing
The system SHALL apply stem mix mode and future per-stem mask changes without changing pad
voice timing.

Prepared-stem playback SHALL continue to share the same voice playhead, loop region,
transport-scheduled start frame, BPM-lock behavior, key-lock processing, EQ/gain, metering,
and playhead update path as full-mix playback.

#### Scenario: Switching to all-stems keeps the loop position
- **GIVEN** a pad is playing with a valid prepared stem set
- **WHEN** the performer switches the pad from full-mix mode to all-stems mode
- **THEN** playback continues from the same voice playhead position
- **AND** loop-region, BPM-lock, key-lock, EQ/gain, metering, and playhead reporting continue on the same timing path

#### Scenario: Per-stem toggle keeps synchronized playback
- **GIVEN** a pad is playing in all-stems mode
- **WHEN** the performer toggles a selected-pad per-stem control
- **THEN** enabled stems are read from the same loop-relative sample position
- **AND** the pad is not retriggered or time-slipped by the toggle itself


<!-- Added from add-stem-performance-controls -->
### Requirement: Full-Mix Revert Preserves Playback
The system SHALL allow a pad to revert from prepared-stem playback to full-mix playback
without stopping current playback.

If the full-mix revert request reaches the audio callback, the callback SHALL update bounded
mix state only. It SHALL NOT delete cache artifacts, unload prepared handles, read files,
decode audio, run neural inference, log, block, allocate stem buffers, or acquire the Python
GIL.

#### Scenario: Reverting to full mix is immediate and safe
- **GIVEN** a pad is playing in all-stems mode
- **WHEN** the performer selects full-mix mode
- **THEN** the pad continues playback using the loaded full-mix buffer
- **AND** playback does not require stopping, retriggering, or deleting the prepared stem cache

#### Scenario: Full-mix revert is safe with missing stems
- **GIVEN** a pad has no current prepared stem set
- **WHEN** the performer selects full-mix mode
- **THEN** playback remains on the loaded full-mix buffer
- **AND** the audio callback performs no stem cache file I/O


<!-- Added from prepare-realtime-callback-safety -->
### Requirement: Oversized Output Blocks Are Rendered In Bounded Chunks
The system SHALL render audio callback output buffers in chunks that fit the preallocated
real-time processing buffers.

If the audio backend delivers more output frames than the requested callback block size, the Rust
audio path SHALL split the render work into bounded sub-blocks rather than indexing past internal
buffers or resizing them in the callback. The chunking SHALL preserve immediate playback,
scheduled event offsets, loop playback, BPM-lock tempo ratio, Key Lock mode selection, stem
rendering fallback, gain/EQ application, and per-pad metering semantics.

#### Scenario: Larger-than-requested callback block remains safe
- **GIVEN** the audio backend delivers an output block larger than the requested fixed block size
- **WHEN** the callback renders the block
- **THEN** Rust renders it as bounded sub-blocks that fit preallocated processing buffers
- **AND** playback continues without panic, heap allocation, blocking, logging, disk I/O, Python
  GIL access, neural inference, or plugin loading

#### Scenario: Scheduled event offset survives chunking
- **GIVEN** a scheduled pad start targets an output frame inside an oversized callback block
- **WHEN** the callback renders the block using bounded sub-blocks
- **THEN** frames before the scheduled target are rendered before the pad starts
- **AND** the pad starts contributing at the scheduled target frame


<!-- Added from update-waveform-pause-toggle -->
### Requirement: Pause and Resume Sample Playback By ID
The system SHALL provide Python APIs to pause and resume playback of a sample by integer `id` without resetting the playhead.

`AudioEngine.pause_sample(id)` SHALL pause playback of the sample associated with `id`, if it is currently playing. Pausing SHALL stop the sample's voice from contributing to the audio output while retaining its current playback position within the loop region (or full sample if no loop region).

`AudioEngine.resume_sample(id)` SHALL resume playback of the sample associated with `id` from its paused position. If the sample was not previously paused, the call SHALL have no effect.

Both functions SHALL be no-ops for `id` with no loaded sample or no active playback voice. They SHALL NOT affect other sample IDs.

#### Scenario: Pause stops mixing but preserves position
- **GIVEN** a sample is loaded into slot `id` and is currently playing
- **AND** playback has progressed to time `t` within the loop region
- **WHEN** `AudioEngine.pause_sample(id)` is called
- **THEN** the sample's voice stops contributing to the audio output immediately
- **AND** the stored playback position remains at time `t`
- **AND** subsequent calls to `resume_sample(id)` will continue from `t`

#### Scenario: Resume continues from paused position
- **GIVEN** a sample in slot `id` is paused with playback position at time `t`
- **WHEN** `AudioEngine.resume_sample(id)` is called
- **THEN** the sample resumes playback from time `t`
- **AND** the voice continues mixing from that point onward

#### Scenario: Pause has no effect if not playing
- **GIVEN** a sample is loaded into slot `id` but is not currently playing (or already paused)
- **WHEN** `AudioEngine.pause_sample(id)` is called
- **THEN** the call succeeds with no effect on playback state

#### Scenario: Resume has no effect if not paused
- **GIVEN** a sample is loaded into slot `id` and is currently playing (not paused)
- **WHEN** `AudioEngine.resume_sample(id)` is called
- **THEN** the call succeeds with no effect on playback state

#### Scenario: Pause/resume are safe for missing sample
- **WHEN** `pause_sample(id)` or `resume_sample(id)` is called for an `id` with no loaded sample
- **THEN** the call is ignored (no exception) and the audio callback continues without panic or blocking

