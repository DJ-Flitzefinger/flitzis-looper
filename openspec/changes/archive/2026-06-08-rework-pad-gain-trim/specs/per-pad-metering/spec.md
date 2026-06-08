## MODIFIED Requirements

### Requirement: Per-pad peak metering is available to the UI
The audio engine SHALL compute per-pad peak levels for pads that are currently contributing to the
output.

Peak values MUST be computed from actual rendered pad audio after per-pad Gain/Trim and per-pad
EQ/DSP. Peak values MUST be mono, computed as the maximum absolute sample value across channels for
the pad during the measurement interval. Peak values MUST be clamped to the range `0.0..=1.0` when
published to the UI. If the unclamped peak reaches or exceeds `1.0`, the reported peak value SHALL
be `1.0` so the UI can indicate clipping. The audio engine SHALL publish peak updates from the
audio thread to the control/UI thread without blocking.

#### Scenario: Peak updates are emitted for active pads
- **GIVEN** pad `id` is playing
- **WHEN** the audio callback processes audio for that pad
- **THEN** the audio engine publishes periodic peak updates for `id`

#### Scenario: Peak update indicates clipping threshold
- **GIVEN** pad `id` is playing
- **AND** its post-Gain/Trim/post-EQ rendered pad peak reaches or exceeds `1.0`
- **WHEN** the audio engine publishes a peak update
- **THEN** the reported peak value for `id` is `1.0`

### Requirement: Meter is rendered in the gain area, not inside pads
The UI SHALL render a horizontal Gain-area meter for the selected pad derived from the most recent
peak update for that pad.

The Gain-area meter SHALL render a two-zone scale where the first 80% is green and the remaining
20% is yellow. The Gain-area meter SHALL NOT reserve a small red overload zone because clipping is
shown by a dedicated clip indicator. The dedicated clip indicator SHALL activate when the pad peak
reaches `1.0`, SHALL use a clearly perceptible bright red active state, and SHALL remain visibly
held or afterglowing for about one second. Performance pad buttons SHALL NOT render the previous
vertical right-edge pad level meter. Meter rendering SHALL use cached state and SHALL NOT derive
the meter from the Gain/Trim knob position.

#### Scenario: Gain-area meter reflects playing audio
- **GIVEN** pad `id` is selected, playing, and producing non-silent output
- **WHEN** the selected-pad sidebar is rendered
- **THEN** the Gain-area meter shows a non-zero level derived from the cached pad peak

#### Scenario: Performance pad omits vertical level meter
- **GIVEN** pad `id` is loaded and has a recent peak update
- **WHEN** the performance pad grid is rendered
- **THEN** the pad button does not render the previous vertical right-edge level meter
- **AND** the selected-pad Gain-area meter remains the pad level display

#### Scenario: Gain-area clip indicator holds briefly
- **GIVEN** pad `id` produces a peak that reaches `1.0`
- **WHEN** the selected-pad sidebar is rendered immediately after the peak update
- **THEN** the Gain-area clip indicator is visibly active
- **WHEN** the hold time has elapsed without another clipping peak
- **THEN** the Gain-area clip indicator is no longer active
