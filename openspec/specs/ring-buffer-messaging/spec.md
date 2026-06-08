# Ring Buffer Messaging Specification

## Purpose
To enable efficient bounded communication between audio processing, control, loader, and input threads while keeping the CPAL audio callback realtime-safe.
## Requirements
### Requirement: Fixed-capacity Ring Buffer Messaging
The system SHALL use fixed-capacity ring buffers for inter-thread audio-engine messaging.

Control-to-audio message buffers SHALL have a capacity of 1024 messages and SHALL carry fixed-size Rust message values or lightweight shared handles. The audio-to-control buffer SHALL also be fixed capacity and SHALL carry small telemetry messages.

#### Scenario: Single message transfer
- **WHEN** a message is written by the producer
- **THEN** it is readable by the consumer
- **AND** the buffer head and tail pointers are correctly updated

### Requirement: Separate Ordered Commands And Fast Parameters
The system SHALL separate ordered command messages from high-rate scalar parameter messages.

Ordered commands SHALL preserve event order for playback, publication, loop, stem, mode, and transport state changes. Fast parameter messages SHALL carry frequently updated scalar targets and SHALL be coalesced by identity in the audio callback before application.

#### Scenario: Parameter burst does not consume command capacity
- **GIVEN** a performer rapidly changes a scalar control
- **WHEN** parameter updates fill or pressure the parameter ring
- **THEN** ordered trigger and stop commands still use the separate command ring

#### Scenario: Latest drained parameter wins
- **GIVEN** multiple drained parameter messages target the same parameter identity
- **WHEN** the callback applies the drained parameter batch
- **THEN** only the latest drained value for that identity is applied

### Requirement: Real-time Safety
The system SHALL ensure no heap allocations, no Python GIL acquisition, and no blocking operations during audio thread message processing.

The audio callback MUST NOT perform disk I/O, JSON access, plugin scanning/loading, neural inference, UI work, logging, blocking waits, or unbounded message draining while processing messages.

#### Scenario: Audio thread message processing
- **WHEN** a message is received in the audio thread
- **THEN** no heap allocations occur due to message processing
- **AND** the Python GIL is not acquired
- **AND** no blocking operations are performed

### Requirement: Bounded Callback Drain
The system SHALL bound callback-side message drain work per invocation.

The callback SHALL drain no more than the configured ordered command budget and no more than the configured parameter budget in one invocation. Additional messages SHALL remain queued for later callbacks.

#### Scenario: Control-message burst is bounded
- **GIVEN** more ordered control messages are queued than the per-callback budget
- **WHEN** the callback drains messages
- **THEN** it processes only the configured budget
- **AND** leaves remaining messages queued without blocking

### Requirement: Error Handling
The system SHALL handle ring buffer full and empty conditions gracefully.

#### Scenario: Full buffer handling
- **WHEN** a ring buffer is full and a producer attempts to send a message
- **THEN** the push fails without blocking
- **AND** no panic occurs in the audio thread

#### Scenario: Empty buffer handling
- **WHEN** the audio thread attempts to read from an empty ring buffer
- **THEN** no message is returned
- **AND** the thread continues processing normally
- **AND** no blocking occurs

## Constraints
- Ring buffers are fixed capacity.
- Message values are bounded Rust structs or lightweight handles.
- The audio callback does not acquire the Python GIL.
- The audio callback does not block.
- The audio callback does not perform disk I/O, JSON access, plugin work, neural inference, logging, or UI work.


<!-- Added from add-low-jitter-input-mapping -->
### Requirement: Rust Input Modules Stay Outside Audio Callback
The system SHALL allow Rust input/control modules outside the audio callback while keeping the
audio callback protected.

Rust modules outside the callback MAY own MIDI ports, timestamping, normalization, in-memory
mapping snapshots, bounded queues, dispatcher threads, and command bridging. The audio callback
SHALL NOT perform MIDI port handling, keyboard polling, JSON access, Python/GIL access, blocking
locks, logging, neural inference, unbounded allocation, or long-running work.

#### Scenario: MIDI callback queues control work outside audio callback
- **WHEN** the MIDI backend callback receives a supported mapped input
- **THEN** Rust normalizes and queues the event outside the audio callback
- **AND** the audio callback only observes any resulting bounded control message later

#### Scenario: Audio callback remains free of Python and JSON
- **WHEN** mapped input playback is active
- **THEN** the audio callback does not call Python
- **AND** it does not read or write mapping JSON
- **AND** it does not own MIDI port connections


<!-- Added from add-low-jitter-input-mapping -->
### Requirement: MIDI Never Targets Audio Callback Directly
The system SHALL forbid direct MIDI-to-audio-callback execution paths.

MIDI input SHALL pass through the Rust input layer, mapping resolver, and existing bounded
control-command bridge before it can affect audio playback. MIDI input SHALL NOT call audio
callback functions directly and SHALL NOT bypass the established command queue.

#### Scenario: Mapped MIDI trigger uses command bridge
- **GIVEN** a MIDI input is mapped to pad trigger
- **WHEN** the performer sends that MIDI input
- **THEN** the Rust input layer resolves the mapping outside the callback
- **AND** it requests playback through the bounded control-command bridge
- **AND** the audio callback performs only its normal queued-message processing


<!-- Added from add-offline-stem-cache -->
### Requirement: Stem Publication Uses Fixed-Size Control Messages
The system SHALL publish prepared stem buffers to the audio thread using the existing
fixed-capacity control-to-audio ring buffer.

Stem publication messages SHALL contain bounded scalar metadata and shared immutable buffer
handles only, such as pad id, source generation/version token, available stem mask, and
per-stem audio handles. They SHALL NOT contain file paths, Python objects, unbounded vectors
of metadata, or full audio data copied through the message.

#### Scenario: Prepared stem set is published by handle
- **GIVEN** a background stem task has prepared validated immutable stem buffers
- **WHEN** the control layer publishes the stem set to Rust
- **THEN** the control message contains fixed-size descriptors and shared buffer handles
- **AND** the audio thread does not read cache files or copy full stem audio through the ring buffer


<!-- Added from add-offline-stem-cache -->
### Requirement: Stem Message Handling Remains Real-Time Safe
The system SHALL handle stem publication messages in the audio callback without disk I/O,
Python/GIL access, blocking, logging, heap allocation, neural inference, or long-running
work.

If the ring buffer is full before a stem publication request reaches the audio thread, the
producer-side request SHALL fail or be deferred without affecting the audio callback.

#### Scenario: Ring buffer full preserves playback
- **GIVEN** a prepared stem publication request is ready
- **AND** the control-to-audio ring buffer is full
- **WHEN** the producer attempts to enqueue the request
- **THEN** the request is rejected or deferred outside the audio callback
- **AND** currently playing full-mix audio continues unchanged

#### Scenario: Audio callback accepts prepared handles only
- **GIVEN** a stem publication message reaches the audio callback
- **WHEN** the callback handles the message
- **THEN** it stores or rejects the bounded prepared handles using audio-thread-owned state
- **AND** it does not touch disk, allocate stem audio, run inference, log, block, or acquire the GIL


<!-- Added from add-phase-aware-playback-sync -->
### Requirement: Phase Sync Uses Fixed-Size Control Messages
The system SHALL use the existing fixed-capacity control-to-audio ring buffer for
phase-aware playback sync requests.

Any new request for BPM-lock transport phase anchoring SHALL be represented as a fixed-size
control message. The message SHALL contain bounded identifiers or scalar values only, such
as a selected pad id, and SHALL NOT contain file paths, heap-owned beat-grid vectors, or
Python objects.

#### Scenario: BPM-lock phase-anchor request is fixed-size
- **WHEN** Python/control code requests transport phase anchoring from the selected BPM-lock pad
- **THEN** the request is enqueued as a fixed-size control message
- **AND** the audio thread uses audio-thread-owned mixer and transport state to process it
- **AND** Python does not directly access transport, scheduler, or mixer storage


<!-- Added from add-phase-aware-playback-sync -->
### Requirement: Phase Sync Message Failures Are Non-Blocking
Phase sync message failures SHALL be handled without blocking the audio callback.

If the control ring buffer is full before a request reaches the audio thread, the
Python-facing producer-side error/drop behavior SHALL apply. If phase anchoring cannot be
computed after a request reaches the audio thread, the request SHALL be ignored safely and
existing playback SHALL continue.

#### Scenario: Ring buffer full does not affect audio callback
- **GIVEN** the control-to-audio ring buffer is full
- **WHEN** Python/control code attempts to enqueue a phase-anchor request
- **THEN** the request is rejected or dropped according to the Python API contract
- **AND** the audio callback continues unaffected

#### Scenario: Phase anchor request cannot be computed
- **GIVEN** a phase-anchor request reaches the audio callback
- **AND** the selected pad is inactive or missing required metadata
- **WHEN** the callback handles the request
- **THEN** the callback ignores the request safely
- **AND** it does not block, allocate, log, touch disk, acquire the GIL, or panic


<!-- Added from add-rust-transport-timeline -->
### Requirement: Transport Scheduling Uses Existing Ring Buffers
The system SHALL keep the existing fixed-capacity SPSC ring-buffer architecture for
transport and scheduler control messages.

Python/control code SHALL send fixed-size control messages through the existing
control-to-audio path. The audio callback SHALL own the consumer side, the transport
timeline, and the fixed-capacity scheduler.

#### Scenario: Quantized trigger request uses fixed-size messaging
- **WHEN** Python/control code requests a quantized pad trigger
- **THEN** the request is represented as a fixed-size control message
- **AND** the audio thread converts it to an absolute output-frame scheduler event
- **AND** Python does not directly access audio-thread scheduler storage


<!-- Added from add-rust-transport-timeline -->
### Requirement: Transport Message Failures Are Non-Blocking
Transport and scheduler message failures SHALL be handled without blocking the audio
callback.

If the control ring buffer is full before a request reaches the audio thread, the existing
Python-facing error/drop behavior SHALL apply. If the audio-thread scheduler is full after a
request reaches the audio thread, scheduler-full behavior SHALL apply.

#### Scenario: Control ring buffer full remains producer-side failure
- **GIVEN** the control-to-audio ring buffer is full
- **WHEN** Python/control code attempts to enqueue a transport or quantized trigger message
- **THEN** the message is rejected or dropped according to the Python API contract
- **AND** the audio callback continues unaffected

#### Scenario: Audio-thread scheduler full remains callback-local failure
- **GIVEN** a quantized trigger message reaches the audio callback
- **AND** the fixed-capacity scheduler is full
- **WHEN** the callback handles the message
- **THEN** the callback rejects the scheduled request without blocking
- **AND** it does not acquire the Python GIL, perform disk I/O, allocate heap memory, log, or panic


<!-- Added from add-stem-performance-controls -->
### Requirement: Stem Mix Control Messages Are Fixed Size
The system SHALL update audio-thread stem mix state using fixed-size bounded control
messages through the existing control-to-audio ring buffer.

Stem mix control messages SHALL contain bounded scalar fields such as pad id, source-version
hash or token, mix mode, enabled stem mask, mute mask, and solo mask. They SHALL NOT contain
file paths, Python objects, unbounded metadata vectors, or copied stem audio payloads.

#### Scenario: All-stems mode sends bounded control state
- **GIVEN** the performer selects all-stems mode for a pad with current prepared stems
- **WHEN** the controller publishes the update to Rust
- **THEN** the control-to-audio message contains only bounded stem mix metadata
- **AND** the audio thread does not receive file paths or full stem audio payloads

#### Scenario: Future per-stem mask update is bounded
- **GIVEN** a per-stem mask control changes for a pad
- **WHEN** the control layer publishes the update to Rust
- **THEN** the message represents the state as bounded masks over known stem kinds
- **AND** the update does not allocate, block, log, touch disk, run inference, or acquire the Python GIL in the audio callback

#### Scenario: Bottom-bar preset update is bounded
- **GIVEN** the performer selects the selected-pad `I` or `A` preset
- **WHEN** the control layer publishes the update to Rust
- **THEN** the control-to-audio message carries pad id, source-version hash, and an enabled-stem mask only
- **AND** the message does not contain file paths, Python objects, unbounded metadata, or copied stem audio payloads


<!-- Added from add-stem-performance-controls -->
### Requirement: Stem Mix Message Failure Preserves Playback
The system SHALL preserve current playback when a stem mix control message cannot be
enqueued or is rejected by the audio thread.

Producer-side ring-buffer-full failure SHALL be reported or deferred outside the audio
callback. Audio-thread stale-source or unavailable-stem rejection SHALL leave existing
full-mix or stem playback state unchanged.

#### Scenario: Ring buffer full leaves current mix unchanged
- **GIVEN** a stem mix update is ready
- **AND** the control-to-audio ring buffer is full
- **WHEN** the producer tries to enqueue the update
- **THEN** the request is rejected or deferred outside the audio callback
- **AND** current playback continues with the previous mix state

#### Scenario: Stale stem mix update is rejected safely
- **GIVEN** a stem mix update targets source version A
- **AND** the audio thread currently has source version B loaded for that pad
- **WHEN** the update reaches the audio callback
- **THEN** the callback rejects the stale update
- **AND** current playback continues with the previous mix state


<!-- Added from clarify-state-ownership-boundary -->
### Requirement: Audio Telemetry Dispatch Is Controller-Owned
The system SHALL route audio-to-control telemetry through controller-owned dispatch before it
mutates Python session projections.

The UI layer MAY request runtime polling during rendering, but the controller SHALL own the
message-type dispatch for `SampleStarted`, `SampleStopped`, `PadPeak`, and `PadPlayhead` telemetry.

#### Scenario: Audio message updates session through controller dispatch
- **GIVEN** the audio-to-control queue contains pad peak, playhead, started, or stopped telemetry
- **WHEN** runtime events are polled
- **THEN** the controller dispatches each recognized message to the appropriate controller handler
- **AND** the resulting `SessionState` changes happen through controller-owned code


<!-- Added from clarify-state-ownership-boundary -->
### Requirement: Python Session Playback State Is A Projection Of Rust Audio State
The system SHALL treat Rust audio-thread state as the live authority for active voices, source
playheads, pause/render state, transport, scheduler, loaded buffers, prepared stems, and future
smoothed DSP parameter state.

Python `SessionState` playback fields SHALL be transient projections updated by controller-owned
telemetry handling and explicit controller actions such as unload, pause, and resume. Audio
telemetry remains best-effort; a dropped telemetry message MUST NOT make `ProjectState` durable
intent change silently.

#### Scenario: Dropped telemetry does not persist false live state
- **GIVEN** audio-to-control telemetry is delayed or dropped
- **WHEN** the project is saved or restored
- **THEN** `ProjectState` still persists only durable performer intent
- **AND** live playback indicators remain a transient `SessionState` projection


<!-- Added from harden-gen3-runtime-control-paths -->
### Requirement: Must-apply control publications report enqueue failure
The system SHALL report failed enqueue attempts for must-apply command and parameter publications as caller-visible errors.

Must-apply publications SHALL include startup restore, explicit unload/reset neutralization, and one-shot ordered state changes where Python must know whether Rust accepted the requested live-audio state. A caller MAY deliberately classify high-rate updates as best-effort only when that choice is explicit and test-covered.

#### Scenario: Must-apply parameter update reports full parameter queue
- **GIVEN** a must-apply startup or reset path publishes a global volume, speed, master BPM, per-pad BPM, per-pad gain, or per-pad EQ target
- **AND** the parameter queue cannot accept the message
- **WHEN** the Rust-facing setter is called
- **THEN** the setter returns a caller-visible failure
- **AND** Python does not mark the must-apply publication as accepted

#### Scenario: Must-apply ordered command reports full command queue
- **GIVEN** a must-apply path publishes an unload, loop-region, timing metadata, Key Lock, trigger quantization, or other ordered live-audio state command
- **AND** the ordered command queue cannot accept the message
- **WHEN** the Rust-facing setter is called
- **THEN** the setter returns a caller-visible failure
- **AND** the failure is not hidden behind a successful Python API return

#### Scenario: Best-effort classification is explicit
- **GIVEN** a high-rate control path intentionally treats a superseded update as best-effort
- **WHEN** its queue publication cannot be accepted
- **THEN** the code path documents or exposes that best-effort classification
- **AND** must-apply startup, restore, unload, and reset paths do not reuse the silent best-effort behavior


<!-- Added from prepare-realtime-callback-safety -->
### Requirement: Audio Callback Control Drain Is Budgeted
The system SHALL process no more than a fixed bounded number of control-to-audio messages during
one audio callback invocation.

When more control messages are pending than the per-callback budget allows, the audio callback
SHALL leave the remaining messages in the bounded control ring for later callbacks. The callback
SHALL continue to render the current output buffer after the budgeted drain and SHALL NOT spin
until the producer-side burst is exhausted.

#### Scenario: Control burst is partially drained
- **GIVEN** more control messages are queued than the callback budget allows
- **WHEN** one audio callback invocation processes control messages
- **THEN** it handles at most the configured budget
- **AND** it leaves the remaining messages queued for later callbacks
- **AND** it proceeds to audio rendering without blocking, allocating, logging, touching disk,
  acquiring the Python GIL, or polling MIDI ports


<!-- Added from prepare-realtime-callback-safety -->
### Requirement: Retired Audio Buffers Are Dropped Off The Callback Thread
The system SHALL defer sample-buffer and prepared-stem-buffer handle retirement from the audio
callback to non-audio-thread cleanup.

When the audio callback replaces, unloads, rejects, or stops using shared immutable audio buffer
handles, it SHALL move those handles into bounded preallocated retirement state or a bounded
non-audio cleanup queue. The callback SHALL NOT perform large final `Arc` deallocations for sample
or stem audio payloads.

#### Scenario: Rejected prepared stems are retired outside realtime rendering
- **GIVEN** a prepared-stem publication message reaches the audio callback
- **AND** the callback rejects the publication because the pad is active, stale, unloaded, or
  shape-incompatible
- **WHEN** the callback releases the rejected prepared handles
- **THEN** the handles are moved to non-audio cleanup
- **AND** the callback does not deallocate the large stem audio payloads directly

#### Scenario: Unload retires loaded handles outside realtime rendering
- **GIVEN** a pad has a loaded sample buffer and prepared stems
- **WHEN** an unload request reaches the audio callback
- **THEN** the callback stops using the handles and schedules them for non-audio cleanup
- **AND** it does not perform disk I/O, blocking waits, logging, Python/GIL access, neural
  inference, plugin loading, or large audio-payload deallocation


<!-- Added from prepare-command-parameter-path -->
### Requirement: Commands And Parameters Use Separate Bounded Paths
The system SHALL route discrete control commands and continuous parameter updates through separate
bounded control-to-audio paths.

The ordered command path SHALL carry playback triggers, stop commands, sample publication,
transport/mode changes, stem state changes, loop-region state changes, pause/resume, unload, and
other order-sensitive operations. The parameter path SHALL carry fast scalar updates such as
volume, speed, master BPM, per-pad BPM, per-pad gain, per-pad EQ, and future DSP parameters.

#### Scenario: Parameter burst does not fill command queue
- **GIVEN** the continuous parameter path is full of pending parameter updates
- **WHEN** a trigger or stop command is sent through the ordered command path
- **THEN** command acceptance depends on command path capacity
- **AND** the parameter backlog does not occupy command path slots
- **AND** the audio callback remains bounded and nonblocking


<!-- Added from prepare-command-parameter-path -->
### Requirement: Parameter Updates Are Coalesced Before Audio-State Application
The system SHALL coalesce continuous parameter updates by parameter identity before applying them
to audio-thread state during one callback invocation.

When multiple drained parameter messages target the same parameter identity, the callback SHALL
apply the latest drained value for that identity and SHALL NOT repeatedly apply superseded values
from the same callback drain. Future DSP parameters SHALL use this parameter path and SHALL apply
audio-side smoothing before sample processing.

#### Scenario: Repeated EQ updates use latest drained value
- **GIVEN** multiple per-pad EQ parameter messages for the same pad are pending
- **WHEN** one audio callback invocation drains those parameter messages
- **THEN** only the latest drained EQ value for that pad is applied to mixer state
- **AND** no parameter processing blocks, logs, touches disk, acquires the Python GIL, polls MIDI
  ports, loads plugins, or allocates unbounded audio-thread state


<!-- Added from prepare-command-parameter-path -->
### Requirement: Direct Input Multi-Message Dispatch Is Atomic
The system SHALL enqueue direct Rust input-dispatch command sequences all-or-nothing when one
input action requires more than one ordered command message.

If the ordered command path lacks capacity for the complete sequence, the dispatcher SHALL reject
the direct sequence without enqueueing a partial loop-region update, partial trigger, or partial
stop sequence.

#### Scenario: Trigger dispatch rejects partial loop-and-play sequence
- **GIVEN** a mapped direct MIDI trigger needs to send a loop-region update followed by a play
  command
- **AND** the ordered command queue has capacity for only one message
- **WHEN** the dispatcher handles the trigger
- **THEN** it sends no command messages
- **AND** it reports the direct dispatch as not dispatched
- **AND** the audio callback later observes no partial trigger transaction

