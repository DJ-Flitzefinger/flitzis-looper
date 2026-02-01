# loop-region Specification

## Purpose
To define per-pad loop region settings (start/end times, auto-loop on/off, bar count) and default behaviors (beat snapping, reset, sample-accurate markers) for precise loop editing in the waveform editor.
## Requirements
### Requirement: Per-pad loop region settings
The system SHALL maintain per-pad loop region settings consisting of:
- `loop_start_sec` (loop start time in seconds)
- `loop_end_sec` (loop end time in seconds)
- `auto_loop_enabled` (boolean; default enabled)
- `auto_loop_bars` (integer bar count; default 4)

#### Scenario: Defaults exist for a newly loaded pad
- **GIVEN** a pad has loaded audio
- **WHEN** loop settings are first evaluated for that pad
- **THEN** auto-loop is enabled
- **AND** the bar count is 4

### Requirement: Default loop region uses analysis downbeat onset
When analysis metadata exists for a pad, the default loop start SHALL be derived from the analysis beat grid.

The system SHALL use `downbeats[0]` as the default onset when available; otherwise it SHALL use `beats[0]`; otherwise it SHALL use `0.0` seconds.

#### Scenario: Default onset uses first downbeat
- **GIVEN** a pad has analysis with `downbeats[0] = D`
- **WHEN** the default loop region is computed
- **THEN** the loop start is approximately D

### Requirement: Reset restores a 4-bar auto-loop
The system SHALL provide a reset action that restores the loop region to:
- loop start = default onset (see above)
- auto-loop enabled
- bar count = 4
- loop end = start + 4 bars (see auto-loop requirements below)

#### Scenario: Reset restores defaults
- **GIVEN** a pad has a custom loop region
- **WHEN** the performer activates Reset in the waveform editor
- **THEN** the loop region matches the default configuration described above

### Requirement: Auto-loop defines loop end by bars in 4/4
When auto-loop is enabled, the system SHALL determine loop end from loop start and the bar count.

A bar MUST be defined as 4 beats (4/4).

The loop duration in seconds SHALL be computed from the effective BPM (manual BPM override first, else analysis BPM). If no BPM is available, the system SHALL not attempt musical-duration computation.

#### Scenario: Auto-loop computes loop end from BPM
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** loop start is 10.0 seconds
- **WHEN** the bar count is 4
- **THEN** loop end is approximately 18.0 seconds

#### Scenario: Auto-loop is unavailable without BPM
- **GIVEN** auto-loop is enabled
- **AND** no effective BPM is available
- **WHEN** the waveform editor is rendered
- **THEN** bar-count controls are disabled or visually indicated as unavailable

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

### Requirement: Sample-accurate loop markers
The system SHALL support placing loop markers with a granularity of one sample at the cached WAV sample rate.

When the waveform editor is zoomed such that individual samples are visible, setting loop start or loop end SHALL result in a marker time corresponding to an integer sample index at the cached WAV sample rate (i.e., `n / sample_rate_hz`).

#### Scenario: Extreme zoom marker placement is sample-accurate
- **GIVEN** a pad has loaded audio
- **AND** the waveform editor is zoomed such that individual samples are visible
- **WHEN** the performer sets loop start or loop end
- **THEN** the stored loop marker time corresponds to an integer sample index at the cached WAV sample rate

### Requirement: Loop region updates apply immediately during playback
The system SHALL apply loop region changes immediately; it SHALL NOT require an explicit apply/save action.

The waveform editor SHALL support changing loop start/end while the pad is actively playing.

#### Scenario: Live loop updates
- **GIVEN** a pad is playing
- **WHEN** the performer changes loop start or loop end
- **THEN** subsequent playback loops follow the updated region without stopping the pad

