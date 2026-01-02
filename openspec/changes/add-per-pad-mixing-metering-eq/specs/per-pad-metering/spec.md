## ADDED Requirements

### Requirement: Per-pad peak metering is available to the UI
The audio engine SHALL compute per-pad peak levels for pads that are currently contributing to the output.

Peak values MUST be computed post-processing (post per-pad gain and post per-pad EQ).

Peak values MUST be mono, computed as the maximum absolute sample value across channels for the pad during the measurement interval.

Peak values MUST be clamped to the range `0.0..=1.0`.

If the unclamped peak reaches or exceeds `1.0`, the reported peak value SHALL be `1.0` to indicate clipping.

The audio engine SHALL publish peak updates from the audio thread to the control/UI thread.

#### Scenario: Peak updates are emitted for active pads
- **GIVEN** pad `id` is playing
- **WHEN** the audio callback processes audio for that pad
- **THEN** the audio engine publishes periodic peak updates for `id`

### Requirement: Peak update rate is bounded
Peak updates MUST be rate-limited to approximately 10 updates per second per pad (best-effort).

If the audio→control message channel is full, peak updates SHALL be dropped without blocking.

#### Scenario: Rate limiting prevents excessive updates
- **GIVEN** pad `id` is playing continuously
- **WHEN** 1 second of playback elapses
- **THEN** the UI receives no more than ~10 peak updates for `id`

#### Scenario: Backpressure drops updates safely
- **GIVEN** the audio→control message channel is full
- **WHEN** the audio thread attempts to publish a peak update
- **THEN** the update is dropped
- **AND** the audio thread continues without blocking or panicking

### Requirement: Meter is rendered inside the pad
The UI SHALL render a small level meter inside each pad button, derived from the most recent peak update for that pad.

The meter SHALL indicate clipping when the reported peak value reaches `1.0` (render the top of the meter in red).

#### Scenario: Meter reflects playing audio
- **GIVEN** pad `id` is playing and producing non-silent output
- **WHEN** the performance view is rendered
- **THEN** pad `id` shows a non-zero meter indication

#### Scenario: Meter indicates clipping
- **GIVEN** pad `id` is playing and its reported peak reaches `1.0`
- **WHEN** the performance view is rendered
- **THEN** pad `id` shows a clipping indication (red at the top of the meter)

### Requirement: Metering is performance-friendly in the UI
The UI SHOULD avoid per-frame heavy computation for metering; it SHALL use cached peak values from application state.

#### Scenario: Meter rendering uses cached state
- **WHEN** the performance view is rendered
- **THEN** the UI draws meters using peak values already stored in state
- **AND** no blocking operations occur in the UI render loop
