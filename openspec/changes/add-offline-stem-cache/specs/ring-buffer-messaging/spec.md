## ADDED Requirements

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
