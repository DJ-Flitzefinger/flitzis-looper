## ADDED Requirements

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
