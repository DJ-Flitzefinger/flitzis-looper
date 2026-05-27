## MODIFIED Requirements

### Requirement: Real-time Safety
The system SHALL ensure no heap allocations, no Python GIL acquisition, and no blocking operations during audio thread message processing.

Rubber Band integration SHALL keep library handles, DLL paths, file paths, build-tool paths, dynamically sized buffers, and error-reporting payloads out of callback-side control messages. The audio callback MUST NOT perform Rubber Band library discovery, runtime DLL probing, handle construction, dependency installation, logging, disk I/O, plugin work, neural inference, UI work, or unbounded message draining while processing messages.

#### Scenario: Rubber Band messages remain bounded
- **WHEN** Python enables Key Lock or changes a related mode
- **THEN** the control path sends only bounded scalar mode or parameter values to the audio thread
- **AND** it does not send Rubber Band handles, file paths, DLL paths, or heap-owned backend objects through the ring buffer

#### Scenario: Audio thread message processing remains callback-safe
- **WHEN** a Key Lock or BPM Lock message is received in the audio thread
- **THEN** no heap allocations occur due to message processing
- **AND** the Python GIL is not acquired
- **AND** no blocking operations are performed
