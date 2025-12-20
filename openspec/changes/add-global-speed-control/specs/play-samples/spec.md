## ADDED Requirements

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
