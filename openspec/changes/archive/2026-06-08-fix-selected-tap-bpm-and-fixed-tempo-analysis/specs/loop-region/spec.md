## MODIFIED Requirements

### Requirement: Default loop region uses analysis downbeat onset
The system SHALL derive the default loop start from the analysis beat grid when analysis metadata exists for a pad.

The system SHALL use `downbeats[0]` as the default onset when available; otherwise it SHALL use `beats[0]`; otherwise it SHALL use `0.0` seconds.

The system SHALL snap a first downbeat or first beat very close to the file start to `0.0` seconds before deriving the default onset, so analyzer hop latency at the beginning of a track does not shift the source-domain musical grid.

#### Scenario: Default onset uses first downbeat
- **GIVEN** a pad has analysis with `downbeats[0] = D`
- **AND** D is not very close to the file start
- **WHEN** the default loop region is computed
- **THEN** the loop start is approximately D

#### Scenario: Near-start downbeat snaps to track start
- **GIVEN** a pad has analysis with `downbeats[0]` very close to `0.0` seconds
- **WHEN** the default loop region is computed
- **THEN** the loop start is `0.0` seconds

### Requirement: Beat snapping rules
The system SHALL snap loop start and loop end adjustments to the nearest musical 1/64-note grid point derived from the effective BPM when auto-loop is enabled.

The effective BPM used for snapping SHALL be the same effective BPM used for auto-loop duration computation (manual BPM override first, else analysis BPM).

If no effective BPM is available, the system SHALL NOT perform musical grid snapping.

A 1/64-note grid interval MUST be defined as 1/16 of a beat.
In seconds, the interval SHALL be computed as:
- `beat_sec = 60 / effective_bpm`
- `grid_step_sec = beat_sec / 16`

The grid MUST be anchored at `grid_anchor_sec`, defined as:
- `grid_anchor_sec = default_onset_sec + grid_offset_sec`

The default onset time (`default_onset_sec`) MUST follow "Default loop region uses analysis downbeat onset", including snapping first beat/downbeat values very close to file start to `0.0` seconds.

A future grid offset MUST be supported as an additive shift applied to the anchor (`grid_offset_sec`).
Until such a setting exists, `grid_offset_sec = 0`.

After snapping to the musical grid, the stored marker position MUST be quantized to an integer sample index at the loaded sample rate, and the stored marker time MUST be exactly `sample_index / sample_rate_hz` (i.e., the system MUST NOT store fractional-sample timestamps).

When auto-loop is disabled, loop start and loop end adjustments SHALL not snap and SHALL be free to any time value.

#### Scenario: Auto-loop enabled loop start is exactly sample-accurate on 1/64 grid
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** the grid anchor is 10.0 seconds
- **AND** the loaded sample rate is 48000 Hz
- **WHEN** the performer sets loop start near 10.031 seconds
- **THEN** the stored loop start is exactly 10.03125 seconds
- **AND** it corresponds to sample index 481500 at 48000 Hz

#### Scenario: Near-start analysis anchor does not shift snapping grid
- **GIVEN** auto-loop is enabled
- **AND** analysis reports a first downbeat at 1536 samples after track start
- **WHEN** the performer adjusts loop markers with a zero grid offset
- **THEN** snapping uses a grid anchor at track start
