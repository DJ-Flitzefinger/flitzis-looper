## MODIFIED Requirements

### Requirement: Per-pad peak metering is available to the UI
The audio engine SHALL compute per-pad peak levels for pads that are currently contributing to the
output.

Peak values MUST be computed from actual rendered pad contribution after source or prepared-stem
selection, playback-rate and Key Lock rendering, per-pad Gain/Trim, per-pad EQ/DSP, and trigger
velocity. Per-pad peak values MUST be measured before pad summing, Master Volume, momentary output
mute, and final master output metering. Peak values MUST be mono, computed as the maximum absolute
sample value across channels for the pad during the measurement interval.

The audio engine SHALL publish per-pad peak updates from the audio thread to the control/UI thread
without blocking. The per-pad meter projection MAY clamp the stored display level to `0.0..=1.0`,
but pad clip state MUST be derived from the measured peak reaching or exceeding `1.0`.

#### Scenario: Peak updates are emitted for active pads
- **GIVEN** pad `id` is playing
- **WHEN** the audio callback processes audio for that pad
- **THEN** the audio engine publishes periodic peak updates for `id`

#### Scenario: Pad peak is pre-master
- **GIVEN** pad `id` is playing with a steady post-Gain/Trim/post-EQ contribution
- **WHEN** the performer lowers Master Volume
- **THEN** the per-pad peak for `id` does not decrease solely because Master Volume changed
- **AND** the master output peak reflects the lower final output level

#### Scenario: Pad clip state is derived from measured pad peak
- **GIVEN** pad `id` is playing
- **AND** its post-Gain/Trim/post-EQ rendered pad peak reaches or exceeds `1.0`
- **WHEN** the control/UI layer receives or projects the peak
- **THEN** the pad clip indication can activate even if the display level is clamped to `1.0`

