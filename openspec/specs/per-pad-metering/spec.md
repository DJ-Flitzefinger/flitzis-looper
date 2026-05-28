# per-pad-metering Specification

## Purpose
Defines per-pad peak metering telemetry from the Rust audio engine and its UI/session projection.

## Requirements

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

### Requirement: Peak update rate is bounded
The audio engine SHALL rate-limit per-pad peak updates to approximately 10 updates per second per
pad on a best-effort basis.

If the audio-to-control message channel is full, peak updates SHALL be dropped without blocking.

#### Scenario: Rate limiting prevents excessive updates
- **GIVEN** pad `id` is playing continuously
- **WHEN** 1 second of playback elapses
- **THEN** the UI receives no more than about 10 peak updates for `id`

#### Scenario: Backpressure drops updates safely
- **GIVEN** the audio-to-control message channel is full
- **WHEN** the audio thread attempts to publish a peak update
- **THEN** the update is dropped
- **AND** the audio thread continues without blocking or panicking

### Requirement: Meter is rendered inside the pad
The UI SHALL render a small level meter inside each pad button, derived from the most recent peak
update for that pad.

The meter SHALL indicate clipping when the measured pad peak reaches `1.0` or higher.

#### Scenario: Meter reflects playing audio
- **GIVEN** pad `id` is playing and producing non-silent output
- **WHEN** the performance view is rendered
- **THEN** pad `id` shows a non-zero meter indication

#### Scenario: Meter indicates clipping
- **GIVEN** pad `id` is playing and its measured peak reaches `1.0`
- **WHEN** the performance view is rendered
- **THEN** pad `id` shows a clipping indication

### Requirement: Metering is performance-friendly in the UI
The UI SHALL use cached peak values from application state and avoid per-frame heavy computation
for per-pad metering.

#### Scenario: Meter rendering uses cached state
- **WHEN** the performance view is rendered
- **THEN** the UI draws meters using peak values already stored in state
- **AND** no blocking operations occur in the UI render loop
