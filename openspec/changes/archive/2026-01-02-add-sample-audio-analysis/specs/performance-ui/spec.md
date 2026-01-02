## ADDED Requirements
### Requirement: Display Pad BPM And Key
When a pad has detected analysis metadata, the system SHALL display the pad’s BPM and key.

The BPM and key SHALL be shown:
- In the pad control, positioned in the top-right corner.
- In the selected-pad sidebar.

#### Scenario: Pad shows BPM and key when available
- **GIVEN** a pad has a loaded sample with detected BPM and key
- **WHEN** the performance view is rendered
- **THEN** the pad renders BPM and key in its top-right corner

#### Scenario: Sidebar shows BPM and key for selected pad
- **GIVEN** the selected pad has a loaded sample with detected BPM and key
- **WHEN** the sidebar is rendered
- **THEN** the sidebar renders BPM and key for the selected pad
