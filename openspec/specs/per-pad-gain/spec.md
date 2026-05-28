# per-pad-gain Specification

## Purpose
Defines performer-facing per-pad Gain/Trim controls, persistence, validation, audio-engine
publication, and signal-path placement.

## Requirements

### Requirement: Per-pad gain control
The system SHALL provide a dB-based per-pad Gain/Trim parameter for each pad sample slot id in the
range `0..NUM_SAMPLES`.

The per-pad Gain/Trim MUST be a finite floating-point dB value in the range `-60.0..=+12.0`. The
per-pad Gain/Trim SHALL default to `0.0 dB` for all pads. A normalized UI value of `0.0` SHALL map
to `-60.0 dB`, `0.5` SHALL map to `0.0 dB`, and `1.0` SHALL map to `+12.0 dB`.

The audio engine SHALL convert accepted dB targets to linear gain using
`linear_gain = 10^(gain_db / 20)`. Gain/Trim SHALL be distinct from trigger velocity, performance
volume, Master Volume, and any future explicit output-protection feature.

#### Scenario: Default gain is neutral trim
- **GIVEN** the application has started
- **WHEN** a pad has no explicitly changed Gain/Trim
- **THEN** the pad's effective Gain/Trim is `0.0 dB`
- **AND** the corresponding linear gain is approximately `1.0`

#### Scenario: Normalized UI values map to asymmetric dB trim
- **WHEN** the Gain/Trim UI maps normalized `0.0`
- **THEN** the result is `-60.0 dB`
- **WHEN** the Gain/Trim UI maps normalized `0.5`
- **THEN** the result is `0.0 dB`
- **WHEN** the Gain/Trim UI maps normalized `1.0`
- **THEN** the result is `+12.0 dB`

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
the supported `-60.0..=+12.0 dB` range.

#### Scenario: Gain drag updates asymmetric dB trim
- **GIVEN** a pad is selected
- **WHEN** the performer drags the Gain/Trim control right or up
- **THEN** the selected pad's Gain/Trim increases in dB up to `+12.0 dB`
- **WHEN** the performer drags the Gain/Trim control left or down
- **THEN** the selected pad's Gain/Trim decreases in dB down to `-60.0 dB`

#### Scenario: Gain reset and fine-step gestures update trim
- **GIVEN** pad `id` has non-neutral Gain/Trim
- **WHEN** the performer middle-clicks the Gain/Trim control
- **THEN** the selected pad's Gain/Trim resets to `0.0 dB`
- **WHEN** the performer right-clicks the negative side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim decreases by the fine step
- **WHEN** the performer right-clicks the positive side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim increases by the fine step

### Requirement: Per-pad gain is applied during mixing
The audio engine SHALL apply per-pad Gain/Trim as a bounded dB channel trim while rendering each
pad contribution.

Gain/Trim MUST be applied after source or prepared-stem selection, playback-rate handling, and Key
Lock rendering, and before per-pad EQ/DSP, trigger velocity, pad summing, Master Volume, and master
output metering. Gain/Trim changes for active pads MUST remain click-safe through bounded smoothing
or ramping, and the smoothing and multiplication MUST remain realtime-safe in the audio callback.
Gain/Trim SHALL remain distinct from trigger velocity, performance volume, Master Volume, and any
future explicit output-protection feature.

The system SHALL NOT automatically change per-pad Gain/Trim in response to EQ/isolator changes,
pad clip state, or master clip state.

#### Scenario: Gain feeds EQ and later output stages
- **GIVEN** pad `id` is playing with Gain/Trim set to `g_db`
- **AND** an EQ/isolator band target is changed away from neutral
- **WHEN** the mixer renders audio
- **THEN** the pad source is trimmed by approximately `10^(g_db / 20)` before per-pad EQ/DSP
- **AND** the resulting contribution is subsequently affected by trigger velocity and Master
  Volume

#### Scenario: Gain is not hidden clip compensation
- **GIVEN** pad `id` is playing near full scale
- **WHEN** an EQ/isolator change makes the measured peak reach or exceed `1.0`
- **THEN** the system reports the peak through the relevant meters
- **AND** the pad's Gain/Trim value is not automatically reduced
