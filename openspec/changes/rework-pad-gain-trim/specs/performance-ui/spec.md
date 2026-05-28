## ADDED Requirements

### Requirement: Selected-pad Gain/Trim display layout
The system SHALL render the selected-pad Gain/Trim value directly below the Gain/Trim control in
the left sidebar.

The Gain/Trim value display SHALL use dB formatting with one decimal place and a leading `+` sign
for positive values. The horizontal display and meter SHALL be approximately the same width as the
Gain/Trim control. The UI SHALL render vertical spacing between the Gain/Trim display/meter and
the three Low/Mid/High EQ controls so the controls do not appear cramped. The Gain/Trim value SHALL
NOT be rendered as a percent value inside performance pad buttons. Performance pad buttons SHALL
continue to render loaded-pad BPM/key metadata in the existing top-right pad overlay when that
metadata is available.

#### Scenario: Gain value appears below control
- **GIVEN** pad `id` is selected
- **AND** its Gain/Trim is `+3.5 dB`
- **WHEN** the left sidebar is rendered
- **THEN** the Gain/Trim control is visible
- **AND** a horizontal display directly below it shows `+3.5 dB`
- **AND** the Low, Mid, and High EQ controls appear below that display with visible spacing

#### Scenario: Gain display uses signed dB format
- **WHEN** Gain/Trim is `0.0 dB`
- **THEN** the display shows `0.0 dB`
- **WHEN** Gain/Trim is `+1.5 dB`
- **THEN** the display shows `+1.5 dB`
- **WHEN** Gain/Trim is `-3.0 dB`
- **THEN** the display shows `-3.0 dB`

#### Scenario: Loaded pad keeps BPM and key metadata
- **GIVEN** a loaded pad has BPM and key metadata
- **WHEN** the performance pad grid is rendered
- **THEN** the pad button renders a top-right metadata string such as `94.0 D#`
- **AND** the selected-pad Gain/Trim value is not rendered inside the pad button
