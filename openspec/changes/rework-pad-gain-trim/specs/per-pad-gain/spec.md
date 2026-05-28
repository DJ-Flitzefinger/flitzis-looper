## MODIFIED Requirements

### Requirement: Per-pad gain control
The system SHALL provide a dB-based per-pad Gain/Trim parameter for each pad sample slot id in the
range `0..NUM_SAMPLES`.

The per-pad Gain/Trim MUST be a finite floating-point dB value in the range `-12.0..=+12.0`.
The per-pad Gain/Trim SHALL default to `0.0 dB` for all pads. A normalized UI value of `0.0`
SHALL map to `-12.0 dB`, `0.5` SHALL map to `0.0 dB`, and `1.0` SHALL map to `+12.0 dB`.
The audio engine SHALL convert accepted dB targets to linear gain using
`linear_gain = 10^(gain_db / 20)`. Gain/Trim SHALL be distinct from pad trigger velocity,
performance volume, and master volume.

#### Scenario: Default gain is neutral trim
- **GIVEN** the application has started
- **WHEN** a pad has no explicitly changed Gain/Trim
- **THEN** the pad's effective Gain/Trim is `0.0 dB`
- **AND** the corresponding linear gain is approximately `1.0`

#### Scenario: Normalized UI values map to dB trim
- **WHEN** the Gain/Trim UI maps normalized `0.0`
- **THEN** the target is `-12.0 dB`
- **WHEN** the Gain/Trim UI maps normalized `0.5`
- **THEN** the target is `0.0 dB`
- **WHEN** the Gain/Trim UI maps normalized `1.0`
- **THEN** the target is `+12.0 dB`

#### Scenario: dB values convert to expected linear gain
- **WHEN** Gain/Trim is `-6.0 dB`
- **THEN** the linear gain is approximately `0.501`
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
`-12.0..=+12.0 dB`.

#### Scenario: Gain drag updates dB trim
- **GIVEN** pad `id` is selected
- **WHEN** the performer drags the Gain/Trim control right or up
- **THEN** the selected pad's Gain/Trim increases in dB
- **WHEN** the performer drags the Gain/Trim control left or down
- **THEN** the selected pad's Gain/Trim decreases in dB

#### Scenario: Fine and reset gestures update dB trim
- **GIVEN** pad `id` has non-neutral Gain/Trim
- **WHEN** the performer middle-clicks the Gain/Trim control
- **THEN** the selected pad's Gain/Trim resets to `0.0 dB`
- **WHEN** the performer right-clicks the negative side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim decreases by the fine step
- **WHEN** the performer right-clicks the positive side of the Gain/Trim control
- **THEN** the selected pad's Gain/Trim increases by the fine step

### Requirement: Per-pad gain is applied during mixing
The audio engine SHALL apply the per-pad Gain/Trim when mixing voices for a pad by converting the
accepted dB target to a smoothed linear multiplier.

Gain/Trim SHALL be applied after source selection, prepared-stem mixing, playback-rate handling,
and Key Lock rendering, and before per-pad EQ/DSP, trigger velocity, master volume, and master
mixing. Gain changes for active pads SHALL be smoothed or ramped over a short bounded time to avoid
clicks. The smoothing and multiplication MUST remain realtime-safe in the audio callback.

#### Scenario: Gain composes with EQ, trigger velocity, and master volume
- **GIVEN** pad `id` is playing with trigger velocity `v`
- **AND** master volume is `m`
- **AND** pad Gain/Trim is `g_db`
- **WHEN** the mixer renders audio
- **THEN** the pad source is trimmed by approximately `10^(g_db / 20)` before per-pad EQ/DSP
- **AND** the post-trim/post-EQ contribution is subsequently scaled by `v * m`

#### Scenario: Active gain changes are click-safe
- **GIVEN** pad `id` is playing
- **WHEN** the performer changes Gain/Trim from `0.0 dB` to `+12.0 dB`
- **THEN** the audio engine ramps the applied linear multiplier instead of jumping abruptly
- **AND** the audio callback does not allocate, block, call Python, read files, log, or perform
  unbounded work due to the gain change
