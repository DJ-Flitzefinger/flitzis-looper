## ADDED Requirements

### Requirement: Manual BPM Entry In Selected-Pad Sidebar
When a pad is selected and has audio loaded, the system SHALL provide a manual BPM entry control in the left sidebar.

The manual BPM entry control SHALL:
- Accept a numeric BPM value (float).
- Apply the value to the selected pad as its manual BPM.
- Allow clearing the value to remove the manual BPM override.

#### Scenario: Entering a BPM sets manual BPM
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer enters a BPM value (e.g., 120.0) in the sidebar control
- **THEN** the selected pad’s manual BPM becomes 120.0

#### Scenario: Clearing the BPM removes the manual override
- **GIVEN** a pad is selected and has audio loaded
- **AND** the pad currently has a manual BPM
- **WHEN** the performer clears the BPM value in the sidebar control
- **THEN** the selected pad’s manual BPM becomes unset

### Requirement: Tap BPM Control In Selected-Pad Sidebar
When a pad is selected and has audio loaded, the system SHALL provide a Tap BPM control in the left sidebar.

The Tap BPM control SHALL register a tap on **left mouse button down** (not on button release).

Activating the Tap BPM control repeatedly SHALL compute and set manual BPM for the selected pad (see `pad-manual-bpm`).

#### Scenario: Tap BPM uses mouse down
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer presses the left mouse button down on the Tap BPM control
- **THEN** the system records a Tap BPM event immediately

#### Scenario: Tap BPM sets manual BPM
- **GIVEN** a pad is selected and has audio loaded
- **WHEN** the performer taps the Tap BPM control repeatedly
- **THEN** the selected pad’s manual BPM is updated based on the computed BPM

## MODIFIED Requirements

### Requirement: Display Pad BPM And Key
When a pad has BPM information, the system SHALL display BPM for that pad.

When a pad has detected analysis metadata, the system SHALL display the pad’s key.

The BPM and key SHALL be shown:
- In the pad control, positioned in the top-right corner.
- In the selected-pad sidebar.

When a manual BPM exists for a pad (see `pad-manual-bpm`), the displayed BPM SHALL use that manual BPM value instead of the detected BPM.

#### Scenario: Pad shows BPM and key when available
- **GIVEN** a pad has a loaded sample with detected BPM and key
- **WHEN** the performance view is rendered
- **THEN** the pad renders BPM and key in its top-right corner

#### Scenario: Sidebar shows BPM and key for selected pad
- **GIVEN** the selected pad has a loaded sample with detected BPM and key
- **WHEN** the sidebar is rendered
- **THEN** the sidebar renders BPM and key for the selected pad

#### Scenario: Manual BPM overrides detected BPM in display
- **GIVEN** a pad has a loaded sample with detected BPM and key
- **AND** the pad also has a manual BPM value
- **WHEN** the performance view is rendered
- **THEN** the pad renders the manual BPM value as BPM

#### Scenario: Manual BPM displays even without analysis
- **GIVEN** a pad has a loaded sample
- **AND** the pad has a manual BPM value
- **AND** the pad has no detected analysis metadata
- **WHEN** the performance view is rendered
- **THEN** the pad renders BPM
- **AND** the pad does not render a key value
