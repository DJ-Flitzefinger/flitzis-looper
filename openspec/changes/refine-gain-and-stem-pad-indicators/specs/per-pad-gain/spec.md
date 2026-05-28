## MODIFIED Requirements

### Requirement: Per-pad gain is editable from the left sidebar
The system SHALL render a selected-pad Gain/Trim control in the left sidebar that edits the
selected pad's dB Gain/Trim.

Left mouse interaction SHALL set Gain/Trim from the pointer's absolute horizontal position on the
Gain/Trim control. Left-clicking a position on the control SHALL immediately jump the selected
pad's Gain/Trim to the dB value represented by that position, and left-dragging SHALL continue to
track the pointer position. The absolute position mapping SHALL preserve the existing asymmetric
normalized Gain/Trim curve: the negative side uses the `0.0..0.5` normalized range, neutral
`0.0 dB` remains at `0.5`, and the positive side uses the `0.5..1.0` normalized range.

Right mouse drag SHALL adjust Gain/Trim at fine speed. A middle mouse button click on the
Gain/Trim control SHALL reset the selected pad to `0.0 dB`. Right-clicking the negative side of
the Gain/Trim axis without dragging SHALL decrease Gain/Trim by a small fine step.
Right-clicking the positive side of the Gain/Trim axis without dragging SHALL increase Gain/Trim
by the same fine step. All UI updates SHALL clamp to the supported `-60.0..=+12.0 dB` range.

#### Scenario: Gain left click jumps to the pointer position
- **GIVEN** pad `id` is selected
- **WHEN** the performer left-clicks the negative side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim is set to the negative dB value represented by that
  position
- **WHEN** the performer left-clicks the positive side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim is set to the positive dB value represented by that
  position

#### Scenario: Gain left drag tracks the pointer position
- **GIVEN** pad `id` is selected
- **WHEN** the performer left-drags across the Gain/Trim control
- **THEN** the selected pad's Gain/Trim follows the pointer's absolute horizontal position
- **AND** the visible handle remains aligned to the pointer position within the bounded control
  range

#### Scenario: Fine and reset gestures update dB trim
- **GIVEN** pad `id` has non-neutral Gain/Trim
- **WHEN** the performer middle-clicks the Gain/Trim control
- **THEN** the selected pad's Gain/Trim resets to `0.0 dB`
- **WHEN** the performer right-clicks the negative side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim decreases by the fine step
- **WHEN** the performer right-clicks the positive side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim increases by the fine step
