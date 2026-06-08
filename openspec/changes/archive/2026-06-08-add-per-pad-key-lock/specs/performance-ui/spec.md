## MODIFIED Requirements

### Requirement: BPM Lock And Key Lock Controls Affect Playback State
The system SHALL provide BPM Lock and Key Lock controls whose visual state reflects the current mode and whose activation changes the corresponding mode.

The global `KEY LOCK` control SHALL remain available in the right sidebar. Activating the global `KEY LOCK` control SHALL set the per-pad Key Lock value for each currently loaded pad to the same enabled or disabled state and SHALL leave unloaded pads without Key Lock intent. Per-pad Key Lock controls SHALL remain independently editable after a global update and SHALL affect only their loaded target pad.

#### Scenario: Lock buttons reflect current state
- **GIVEN** Key Lock is disabled
- **WHEN** the performance view is rendered
- **THEN** the global Key Lock control is visually indicated as disabled
- **WHEN** the performer enables global Key Lock
- **THEN** the global Key Lock control is visually indicated as enabled
- **AND** every currently loaded pad's per-pad Key Lock value is enabled
- **AND** unloaded pads have no enabled per-pad Key Lock value

#### Scenario: Global Key Lock disables every loaded pad
- **GIVEN** one or more per-pad Key Lock values are enabled
- **WHEN** the performer disables global Key Lock
- **THEN** the global Key Lock control is visually indicated as disabled
- **AND** every currently loaded pad's per-pad Key Lock value is disabled
- **AND** unloaded pads have no enabled per-pad Key Lock value

## ADDED Requirements

### Requirement: Selected-Pad Key Lock Control
The system SHALL provide a per-pad `KEY LOCK` control in the selected-pad left sidebar.

The selected-pad `KEY LOCK` control SHALL be rendered at the bottom of the selected-pad sidepanel under the Stem Mix / stem controls only when the selected pad has loaded audio. The control SHALL NOT be rendered for unloaded or loading selected pads. The control SHALL use the same `mode-on` and `mode-off` visual language as the global Key Lock control. Activating the selected-pad `KEY LOCK` control SHALL toggle only the selected loaded pad's per-pad Key Lock value.

#### Scenario: Selected pad shows per-pad Key Lock control
- **GIVEN** a loaded pad is selected
- **WHEN** the left selected-pad sidebar is rendered
- **THEN** a `KEY LOCK` control is visible under the Stem Mix / stem controls
- **AND** the control visually reflects the selected pad's per-pad Key Lock value

#### Scenario: Empty selected pad hides per-pad Key Lock control
- **GIVEN** an unloaded pad is selected
- **WHEN** the left selected-pad sidebar is rendered
- **THEN** no selected-pad `KEY LOCK` control is rendered
- **AND** the unloaded pad has no enabled per-pad Key Lock value

#### Scenario: Per-pad Key Lock enables only the selected pad
- **GIVEN** global Key Lock is disabled
- **AND** Pad 3 is selected
- **AND** Pad 3 has loaded audio
- **WHEN** the performer activates the selected-pad `KEY LOCK` control
- **THEN** Pad 3's per-pad Key Lock value is enabled
- **AND** other pads' per-pad Key Lock values remain disabled

#### Scenario: Per-pad Key Lock can override a global baseline
- **GIVEN** global Key Lock has been enabled
- **AND** currently loaded pads have enabled per-pad Key Lock values
- **AND** Pad 3 is selected
- **AND** Pad 3 has loaded audio
- **WHEN** the performer activates the selected-pad `KEY LOCK` control
- **THEN** Pad 3's per-pad Key Lock value is disabled
- **AND** other loaded pads' per-pad Key Lock values remain enabled
- **AND** unloaded pads have no enabled per-pad Key Lock value
