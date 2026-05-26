## MODIFIED Requirements

### Requirement: Selected-pad sidebar offers loop editing
The system SHALL provide an `Adjust Loop` action in the selected-pad sidebar when a loaded pad is
selected.

Activating `Adjust Loop` SHALL open the waveform editor for the selected pad when no waveform
editor is open. Activating `Adjust Loop` for the same selected pad while its waveform editor is
already open SHALL close the waveform editor. Activating `Adjust Loop` for a different loaded pad
while the waveform editor is open SHALL switch the editor to that pad.

#### Scenario: Adjust Loop action is available for loaded pad
- **GIVEN** a pad is selected
- **AND** the pad has loaded audio
- **WHEN** the left sidebar is rendered
- **THEN** the sidebar contains an action labeled "Adjust Loop"

#### Scenario: Adjust Loop opens waveform editor
- **GIVEN** a pad is selected
- **AND** the pad has loaded audio
- **AND** no waveform editor is open
- **WHEN** the performer activates "Adjust Loop"
- **THEN** the waveform editor window opens for that pad

#### Scenario: Adjust Loop closes same-pad editor
- **GIVEN** Pad A is selected
- **AND** Pad A has loaded audio
- **AND** the waveform editor is already open for Pad A
- **WHEN** the performer activates "Adjust Loop"
- **THEN** the waveform editor closes

#### Scenario: Adjust Loop switches to a different selected pad
- **GIVEN** Pad A has loaded audio
- **AND** Pad B is selected and has loaded audio
- **AND** the waveform editor is already open for Pad A
- **WHEN** the performer activates "Adjust Loop"
- **THEN** the waveform editor remains open
- **AND** it targets Pad B

