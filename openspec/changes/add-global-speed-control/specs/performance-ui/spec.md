## ADDED Requirements

### Requirement: Global Speed Controls
The system SHALL provide global speed controls in the performance view consisting of:
- A speed control that allows selecting a speed multiplier in the range 0.5×..2.0×.
- A speed increase action ("+") that increases the speed multiplier by 0.05×.
- A speed decrease action ("-") that decreases the speed multiplier by 0.05×.
- A reset action that restores the speed multiplier to 1.0×.

The speed control MUST default to 1.0× on startup.

The speed control SHALL be rendered vertically and positioned to the right of the performance pad grid.
The speed increase/decrease actions and reset action SHALL be positioned adjacent to the speed control and vertically aligned.

#### Scenario: Speed controls are visible in the performance view
- **WHEN** the UI is started
- **THEN** a global speed control is visible
- **AND** a global speed increase action is visible
- **AND** a global speed reset action is visible
- **AND** a global speed decrease action is visible

#### Scenario: Adjusting speed sends a speed update to the audio engine
- **GIVEN** the audio engine is running
- **WHEN** the performer changes the global speed control from 1.0× to 1.25×
- **THEN** the application calls `AudioEngine.set_speed(1.25)`

#### Scenario: Increment and decrement adjust speed in fixed steps
- **GIVEN** the global speed multiplier is 1.0×
- **WHEN** the performer activates the speed increase action once
- **THEN** the global speed multiplier becomes 1.05×
- **WHEN** the performer activates the speed decrease action once
- **THEN** the global speed multiplier becomes 1.0×

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
- **AND** the speed increase action exists with tag `speed_plus_btn`
- **AND** the speed reset action exists with tag `speed_reset_btn`
- **AND** the speed decrease action exists with tag `speed_minus_btn`
