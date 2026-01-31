## MODIFIED Requirements

### Requirement: Beat snapping rules
When auto-loop is enabled, loop start and loop end adjustments SHALL snap to the nearest musical 1/64-note grid point derived from the effective BPM.

The effective BPM used for snapping SHALL be the same effective BPM used for auto-loop duration computation (manual BPM override first, else analysis BPM).

If no effective BPM is available, the system SHALL NOT perform musical grid snapping.

A 1/64-note grid interval MUST be defined as 1/16 of a beat.
In seconds, the interval SHALL be computed as:
- `beat_sec = 60 / effective_bpm`
- `grid_step_sec = beat_sec / 16`

The grid MUST be anchored at `grid_anchor_sec`, defined as:
- `grid_anchor_sec = default_onset_sec + grid_offset_sec`

The default onset time (`default_onset_sec`) MUST follow "Default loop region uses analysis downbeat onset".

A future grid offset MUST be supported as an additive shift applied to the anchor (`grid_offset_sec`).
Until such a setting exists, `grid_offset_sec = 0`.

After snapping to the musical grid, the stored marker position MUST be quantized to an integer sample index at the cached WAV sample rate, and the stored marker time MUST be exactly `sample_index / sample_rate_hz` (i.e., the system MUST NOT store fractional-sample timestamps).

When auto-loop is disabled, loop start and loop end adjustments SHALL not snap and SHALL be free to any time value.

#### Scenario: Auto-loop enabled loop start is exactly sample-accurate on 1/64 grid
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** the grid anchor is 10.0 seconds
- **AND** the cached WAV sample rate is 48000 Hz
- **WHEN** the performer sets loop start near 10.031 seconds
- **THEN** the stored loop start is exactly 10.03125 seconds
- **AND** it corresponds to sample index 481500 at 48000 Hz

#### Scenario: Auto-loop enabled loop end is exactly sample-accurate on 1/64 grid
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** the grid anchor is 10.0 seconds
- **AND** the cached WAV sample rate is 48000 Hz
- **WHEN** the performer sets loop end near 10.062 seconds
- **THEN** the stored loop end is exactly 10.0625 seconds
- **AND** it corresponds to sample index 483000 at 48000 Hz

#### Scenario: Auto-loop disabled does not snap
- **GIVEN** auto-loop is disabled
- **WHEN** the performer sets loop start to approximately T
- **THEN** the stored loop start is approximately T and is not modified by musical grid snapping
