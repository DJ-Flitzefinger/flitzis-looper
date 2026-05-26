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
