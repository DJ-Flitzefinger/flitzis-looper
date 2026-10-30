## MODIFIED Requirements

### Requirement: BPM Lock And Key Lock Controls Affect Playback State
The system SHALL provide BPM lock and Key Lock controls. The BPM lock control SHALL reflect its
current mode and change that mode on activation. The main global Key Lock control SHALL
show the stored global default using enabled/disabled colors and SHALL change that default on
activation. Its color SHALL NOT imply that explicitly overridden pads or unconfirmed Native
requests use the same live mode. Global changes SHALL affect only inheriting loaded pads.

#### Scenario: Lock buttons reflect current state
- **GIVEN** the global Key Lock default is disabled
- **WHEN** the performance view is rendered
- **THEN** the main global Key Lock control is visually indicated as disabled
- **WHEN** the performer enables the global default
- **THEN** the main global Key Lock control is visually indicated as enabled
- **AND** explicit pad overrides retain their requested modes

### Requirement: Selected-Pad Key Lock Control
The system SHALL provide adjacent `KEY LOCK PAD` and `GLOBAL` controls at the bottom of the
selected-pad left sidebar under Stem Mix / stem controls only for a selected loaded pad. Neither
control SHALL be rendered for unloaded or loading selected pads.

Left-clicking PAD SHALL set explicit ON and right-clicking PAD SHALL set explicit OFF for only the
selected loaded pad. PAD SHALL be green for explicit ON, red for explicit OFF and neutral for
inheritance, based on requested override origin rather than observed Native processing.

The adjacent GLOBAL control SHALL show the default in green for ON or red for OFF only while the
pad inherits; it SHALL be neutral while an explicit override exists. Left-clicking this adjacent
GLOBAL control SHALL clear only the selected pad's override to inheritance. It SHALL NOT change
the global default or other pads' origins. Repeated ON, OFF or inherit gestures SHALL retain their
explicit meaning rather than toggle.

#### Scenario: Selected pad shows per-pad Key Lock control
- **GIVEN** a loaded pad is selected
- **WHEN** the left selected-pad sidebar is rendered
- **THEN** adjacent `KEY LOCK PAD` and `GLOBAL` controls are visible under the stem controls
- **AND** PAD shows the requested override origin
- **AND** GLOBAL shows the default's color only if the pad inherits

#### Scenario: Empty selected pad hides per-pad Key Lock control
- **GIVEN** an unloaded pad is selected
- **WHEN** the left selected-pad sidebar is rendered
- **THEN** neither selected-pad Key Lock control is rendered
- **AND** the unloaded pad has a `null` override

#### Scenario: Per-pad Key Lock enables only the selected pad
- **GIVEN** global Key Lock is disabled
- **AND** Pad 3 is selected
- **AND** Pad 3 has loaded audio
- **WHEN** the performer left-clicks PAD
- **THEN** Pad 3 has explicit ON and PAD is green
- **AND** its adjacent GLOBAL control is neutral
- **AND** other pads' origins remain unchanged

#### Scenario: Right-click selects explicit OFF despite global ON
- **GIVEN** global Key Lock is enabled and selected loaded Pad 3 inherits
- **WHEN** the performer right-clicks PAD
- **THEN** Pad 3 has explicit OFF, PAD is red and its adjacent GLOBAL control is neutral
- **AND** the global default and other pads' origins remain unchanged

#### Scenario: Adjacent GLOBAL restores inheritance
- **GIVEN** global Key Lock is enabled and selected loaded Pad 3 has explicit OFF
- **WHEN** the performer left-clicks its adjacent GLOBAL control
- **THEN** Pad 3's override is `null`, PAD is neutral and GLOBAL is green
- **AND** the global default and other pads' overrides remain unchanged

#### Scenario: Repeated PAD gestures retain explicit meaning
- **GIVEN** selected loaded Pad 3 has explicit ON
- **WHEN** the performer left-clicks PAD again
- **THEN** Pad 3 remains explicitly ON
- **AND** a repeated right-click when explicitly OFF likewise remains OFF

### Requirement: Key Lock Native Status Is Separate From Intent
The system SHALL show Native confirmed/unknown mode, pending preparation and error separately
from intent colors, using matching Native feedback. Queue admission SHALL NOT confirm audio.
Failure and observed audio SHALL NOT rewrite override origin. Transient status SHALL NOT persist
as audio truth or claim musical/device or source-aligned acceptance.

#### Scenario: Failed explicit ON remains green while Native stays OFF
- **GIVEN** selected Pad 3 has confirmed Native OFF
- **WHEN** explicit ON publication or preparation fails
- **THEN** PAD remains green for explicit ON
- **AND** error and confirmed Native OFF are shown separately
- **AND** the override is not rewritten to OFF or inheritance

#### Scenario: Inheritance is visible while native preparation is pending
- **GIVEN** selected Pad 3 inherits an enabled global default
- **WHEN** matching native preparation or confirmation is pending
- **THEN** PAD is neutral and adjacent GLOBAL is green
- **AND** pending/preparing and the previous confirmed or unknown Native state are shown separately
- **AND** the UI does not claim the requested mode is already confirmed
