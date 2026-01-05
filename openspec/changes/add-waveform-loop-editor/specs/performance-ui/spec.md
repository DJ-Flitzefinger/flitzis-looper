## ADDED Requirements

### Requirement: Selected-pad sidebar offers loop editing
When a pad is selected and has loaded audio, the system SHALL provide an action labeled "Adjust Loop" in the selected-pad sidebar.

Activating "Adjust Loop" SHALL open the waveform editor for the selected pad (see `waveform-editor`).

#### Scenario: Adjust Loop action is available for loaded pad
- **GIVEN** a pad is selected
- **AND** the pad has loaded audio
- **WHEN** the left sidebar is rendered
- **THEN** the sidebar contains an action labeled "Adjust Loop"

#### Scenario: Adjust Loop opens waveform editor
- **GIVEN** a pad is selected
- **AND** the pad has loaded audio
- **WHEN** the performer activates "Adjust Loop"
- **THEN** the waveform editor window opens for that pad
