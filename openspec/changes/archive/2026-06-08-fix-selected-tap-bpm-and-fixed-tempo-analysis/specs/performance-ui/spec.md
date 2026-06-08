## MODIFIED Requirements

### Requirement: Display Pad BPM And Key
The system SHALL display BPM and key metadata for loaded pads while separating readable pad-overlay
rounding from precise BPM values used by editable displays, timing, and grid workflows.

The pad control's top-right BPM overlay SHALL display the effective BPM rounded to the nearest
whole BPM for quick scanning. The selected-pad sidebar BPM input and the global BPM display above
the pitch/speed control SHALL display effective BPM values rounded to two decimal places. Manual
BPM entry and automatic analysis metadata SHALL preserve their underlying floating-point BPM values
for playback, snapping, and Loop Editor grid calculations.

When a manual BPM exists for a pad (see `pad-manual-bpm`), the displayed BPM SHALL use that manual
BPM value instead of the detected BPM before applying the display-specific rounding rules.

#### Scenario: Pad overlay shows rounded readable BPM
- **GIVEN** a pad has an effective BPM of 89.99 and detected key metadata
- **WHEN** the performance view renders the pad control
- **THEN** the pad control's top-right overlay displays BPM rounded to the nearest whole BPM
- **AND** the key remains visible next to that rounded BPM

#### Scenario: Editable BPM displays preserve decimal precision
- **GIVEN** a pad or global BPM display has an effective BPM of 89.99
- **WHEN** the selected-pad BPM input or global BPM display is rendered
- **THEN** the rendered BPM text shows the value rounded to two decimal places
- **AND** playback, snapping, and Loop Editor grid calculations continue to use the underlying
  floating-point BPM value
