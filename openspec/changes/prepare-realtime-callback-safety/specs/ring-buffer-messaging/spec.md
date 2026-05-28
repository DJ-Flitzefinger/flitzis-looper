## ADDED Requirements

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
