## ADDED Requirements

### Requirement: Global Speed Controls
The system SHALL provide global speed controls in the performance view consisting of:
- A speed control that allows selecting a speed multiplier in the range 0.5×..2.0×.
- A reset action that restores the speed multiplier to 1.0×.

The speed control MUST default to 1.0× on startup.

#### Scenario: Speed controls are visible in the performance view
- **WHEN** the UI is started
- **THEN** a global speed control is visible
- **AND** a global speed reset action is visible

#### Scenario: Adjusting speed sends a speed update to the audio engine
- **GIVEN** the audio engine is running
- **WHEN** the performer changes the global speed control from 1.0× to 1.25×
- **THEN** the application calls `AudioEngine.set_speed(1.25)`

#### Scenario: Reset restores default speed
- **GIVEN** the global speed multiplier is not 1.0×
- **WHEN** the performer activates the reset action
- **THEN** the global speed multiplier becomes 1.0×
- **AND** the UI control reflects 1.0×

### Requirement: Stable Speed Control Identifiers
The system SHALL assign deterministic item tags to the global speed controls to enable programmatic updates and event binding.

#### Scenario: Speed control tags are stable
- **WHEN** the UI is started
- **THEN** the speed control exists with tag `speed_slider`
- **AND** the speed reset action exists with tag `speed_reset_btn`
