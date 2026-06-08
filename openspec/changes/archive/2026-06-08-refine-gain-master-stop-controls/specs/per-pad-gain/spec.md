## MODIFIED Requirements

### Requirement: Per-pad gain control
The system SHALL provide a dB-based per-pad Gain/Trim parameter for each pad sample slot id in the
range `0..NUM_SAMPLES`.

The per-pad Gain/Trim MUST be a finite floating-point dB value in the range `-60.0..=+12.0`.
The per-pad Gain/Trim SHALL default to `0.0 dB` for all pads. A normalized UI value of `0.0`
SHALL map to `-60.0 dB`, `0.5` SHALL map to `0.0 dB`, and `1.0` SHALL map to `+12.0 dB`.
The negative side of the control SHALL use only the `0.0..0.5` normalized range and the positive
side SHALL use only the `0.5..1.0` normalized range, preserving the neutral 12 o'clock center.
The audio engine SHALL convert accepted dB targets to linear gain using
`linear_gain = 10^(gain_db / 20)`. Gain/Trim SHALL be distinct from pad trigger velocity,
performance volume, and master volume.

#### Scenario: Default gain is neutral trim
- **GIVEN** the application has started
- **WHEN** a pad has no explicitly changed Gain/Trim
- **THEN** the pad's effective Gain/Trim is `0.0 dB`
- **AND** the corresponding linear gain is approximately `1.0`

#### Scenario: Normalized UI values map to asymmetric dB trim
- **WHEN** the Gain/Trim UI maps normalized `0.0`
- **THEN** the target is `-60.0 dB`
- **WHEN** the Gain/Trim UI maps normalized `0.5`
- **THEN** the target is `0.0 dB`
- **WHEN** the Gain/Trim UI maps normalized `1.0`
- **THEN** the target is `+12.0 dB`

#### Scenario: dB values convert to expected linear gain
- **WHEN** Gain/Trim is `-60.0 dB`
- **THEN** the linear gain is approximately `0.001`
- **WHEN** Gain/Trim is `+6.0 dB`
- **THEN** the linear gain is approximately `1.995`
- **WHEN** Gain/Trim is `+12.0 dB`
- **THEN** the linear gain is approximately `3.981`

### Requirement: Per-pad gain is editable from the left sidebar
The system SHALL render a selected-pad Gain/Trim control in the left sidebar that edits the
selected pad's dB Gain/Trim.

Left mouse drag SHALL adjust Gain/Trim at normal speed. Right mouse drag SHALL adjust Gain/Trim at
fine speed. Dragging left or down SHALL decrease Gain/Trim, and dragging right or up SHALL
increase Gain/Trim. A middle mouse button click on the Gain/Trim control SHALL reset the selected
pad to `0.0 dB`. Right-clicking the negative side of the Gain/Trim axis without dragging SHALL
decrease Gain/Trim by a small fine step. Right-clicking the positive side of the Gain/Trim axis
without dragging SHALL increase Gain/Trim by the same fine step. All UI updates SHALL clamp to
`-60.0..=+12.0 dB`.

#### Scenario: Gain drag updates asymmetric dB trim
- **GIVEN** pad `id` is selected
- **WHEN** the performer drags the Gain/Trim control right or up
- **THEN** the selected pad's Gain/Trim increases in dB up to `+12.0 dB`
- **WHEN** the performer drags the Gain/Trim control left or down
- **THEN** the selected pad's Gain/Trim decreases in dB down to `-60.0 dB`

#### Scenario: Fine and reset gestures update dB trim
- **GIVEN** pad `id` has non-neutral Gain/Trim
- **WHEN** the performer middle-clicks the Gain/Trim control
- **THEN** the selected pad's Gain/Trim resets to `0.0 dB`
- **WHEN** the performer right-clicks the negative side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim decreases by the fine step
- **WHEN** the performer right-clicks the positive side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim increases by the fine step
