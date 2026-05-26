## ADDED Requirements

### Requirement: Loaded pad loop defaults use track start and 8 bars
The system SHALL initialize loop intent for a newly loaded track with auto-loop enabled, an
8.0-bar auto-loop count, and loop start at `0.0` seconds.

The default loaded-track loop start SHALL NOT be replaced by analysis downbeat or beat onset.

If no effective BPM is available, the system SHALL still store auto-loop enabled and `8.0` bars,
but SHALL treat the effective loop end as unavailable until a BPM exists.

#### Scenario: Newly loaded pad defaults to 8-bar auto-loop at track start
- **GIVEN** a pad has just loaded audio
- **AND** the pad has an effective BPM of 120
- **WHEN** loop settings are first evaluated for that pad
- **THEN** auto-loop is enabled
- **AND** the bar count is 8.0
- **AND** loop start is 0.0 seconds
- **AND** loop end is approximately 16.0 seconds

#### Scenario: Analysis onset does not move loaded loop default
- **GIVEN** a newly loaded pad has analysis with `downbeats[0] = 2.0`
- **WHEN** loop settings are first evaluated for that pad
- **THEN** loop start is 0.0 seconds
- **AND** the analysis onset remains available only for grid anchoring and snapping

#### Scenario: Loaded default remains auto-looped without BPM
- **GIVEN** a pad has just loaded audio
- **AND** no effective BPM is available
- **WHEN** loop settings are first evaluated for that pad
- **THEN** auto-loop is enabled
- **AND** the bar count is 8.0
- **AND** loop start is 0.0 seconds
- **AND** no musical loop end is computed

### Requirement: ALL sets explicit full-track loop region
The system SHALL provide an `ALL` action that sets the selected pad loop region to the full loaded
track duration.

`ALL` SHALL store loop start `0.0`, loop end equal to the loaded track duration, and
`auto_loop_enabled = false` so the full-track end is explicit rather than derived from bar count.

`ALL` SHALL apply the effective full-track region to the audio engine immediately when the selected
pad is loaded.

#### Scenario: ALL stores full-track manual loop region
- **GIVEN** a pad is loaded with duration 42.0 seconds
- **AND** the pad currently has an auto-loop region
- **WHEN** the performer activates `ALL`
- **THEN** loop start is 0.0 seconds
- **AND** loop end is 42.0 seconds
- **AND** auto-loop is disabled
- **AND** the audio engine receives the full-track loop region

#### Scenario: ALL is unavailable without loaded duration
- **GIVEN** a pad has no loaded track duration
- **WHEN** the performer activates `ALL`
- **THEN** loop settings remain unchanged
- **AND** no loop-region update is sent to the audio engine

### Requirement: Loop bar count supports half-bar numeric values
The system SHALL accept finite numeric auto-loop bar counts in 0.5-bar granularity with a minimum
of 0.5 bars.

Existing project files that contain integer bar counts SHALL load as equivalent numeric bar counts.

#### Scenario: Half-bar value is valid
- **GIVEN** a pad has a loaded track, effective BPM, and enough remaining duration
- **WHEN** the bar count is set to 0.5
- **THEN** the stored bar count is 0.5
- **AND** the auto-loop duration is half of one 4/4 bar

#### Scenario: Legacy integer bar count remains valid
- **GIVEN** a project file stores `pad_loop_bars` for a pad as integer 4
- **WHEN** the project is loaded
- **THEN** the pad bar count is treated as 4.0
- **AND** no migration changes the musical duration of that saved loop

## MODIFIED Requirements

### Requirement: Per-pad loop region settings
The system SHALL maintain per-pad loop region settings consisting of loop start time, loop end time,
auto-loop enabled state, and numeric auto-loop bar count.

The per-pad settings SHALL include:

- `loop_start_sec` as loop start time in seconds, default `0.0`;
- `loop_end_sec` as loop end time in seconds, or unavailable when derived/unknown;
- `auto_loop_enabled` as a boolean, default enabled for newly loaded tracks;
- `auto_loop_bars` as a finite numeric bar count, default `8.0`, minimum `0.5`.

#### Scenario: Defaults exist for a newly loaded pad
- **GIVEN** a pad has loaded audio
- **WHEN** loop settings are first evaluated for that pad
- **THEN** auto-loop is enabled
- **AND** the bar count is 8.0
- **AND** the loop start is 0.0 seconds

### Requirement: Auto-loop defines loop end by bars in 4/4
The system SHALL determine auto-loop end from loop start, numeric bar count, and effective BPM when
auto-loop is enabled.

A bar MUST be defined as 4 beats in 4/4.

The loop duration in seconds SHALL be computed from the effective BPM, using manual BPM override
first and analysis BPM second. If no BPM is available, the system SHALL not attempt
musical-duration computation.

The system SHALL reject requested bar counts that cannot fit between the current loop start and
loaded track duration at the effective BPM.

#### Scenario: Auto-loop computes 8 bars from BPM
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** loop start is 10.0 seconds
- **WHEN** the bar count is 8.0
- **THEN** loop end is approximately 26.0 seconds

#### Scenario: Auto-loop computes a half-bar duration
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** loop start is 10.0 seconds
- **WHEN** the bar count is 0.5
- **THEN** loop end is approximately 11.0 seconds

#### Scenario: Auto-loop is unavailable without BPM
- **GIVEN** auto-loop is enabled
- **AND** no effective BPM is available
- **WHEN** the waveform editor is rendered
- **THEN** bar-count controls are disabled or visually indicated as unavailable

#### Scenario: Bar count cannot exceed remaining duration
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** loop start is 20.0 seconds
- **AND** loaded track duration is 30.0 seconds
- **AND** the current bar count is 4.0
- **WHEN** the performer requests 8.0 bars
- **THEN** the bar count remains 4.0
- **AND** the loop region sent to the audio engine is unchanged

### Requirement: Beat snapping rules
The system SHALL snap auto-loop marker adjustments to the nearest musical 1/64-note grid point
derived from the effective BPM and analysis grid anchor.

The effective BPM used for snapping SHALL be the same effective BPM used for auto-loop duration
computation, using manual BPM override first and analysis BPM second.

If no effective BPM is available, the system SHALL NOT perform musical grid snapping.

A 1/64-note grid interval MUST be defined as 1/16 of a beat. In seconds, the interval SHALL be:

- `beat_sec = 60 / effective_bpm`
- `grid_step_sec = beat_sec / 16`

The grid anchor MUST be `analysis_grid_onset_sec + grid_offset_sec`.
`analysis_grid_onset_sec` SHALL be `downbeats[0]` when available, else `beats[0]`, else `0.0`.
`grid_offset_sec` SHALL be the per-pad sample grid offset converted to seconds, or `0` when no
offset exists.

After snapping to the musical grid, the stored marker position MUST be quantized to an integer
sample index at the loaded sample rate, and the stored marker time MUST be exactly
`sample_index / sample_rate_hz`.

When auto-loop is disabled, loop start and loop end adjustments SHALL not snap and SHALL be free to
any finite valid time value.

The newly loaded default loop start `0.0` and `ALL` full-track region SHALL be explicit actions and
SHALL NOT be shifted to the analysis grid anchor merely because auto-loop or grid snapping exists.

#### Scenario: Auto-loop enabled loop start is exactly sample-accurate on 1/64 grid
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** the grid anchor is 10.0 seconds
- **AND** the loaded sample rate is 48000 Hz
- **WHEN** the performer drags the loop-start marker near 10.031 seconds
- **THEN** the stored loop start is exactly 10.03125 seconds
- **AND** it corresponds to sample index 481500 at 48000 Hz

#### Scenario: Auto-loop disabled does not snap
- **GIVEN** auto-loop is disabled
- **WHEN** the performer sets loop start to approximately T
- **THEN** the stored loop start is approximately T and is not modified by musical grid snapping

#### Scenario: Loaded default is not shifted to analysis onset
- **GIVEN** a newly loaded pad has analysis with `downbeats[0] = 2.0`
- **AND** auto-loop is enabled
- **WHEN** the default loop settings are evaluated
- **THEN** the stored loop start remains exactly 0.0 seconds

## REMOVED Requirements

### Requirement: Default loop region uses analysis downbeat onset
**Reason**: Newly loaded tracks now default to loop start `0.0`; analysis onset is used for the
musical grid anchor and snapping, not for the loaded loop start default.

**Migration**: Existing projects keep stored loop markers. Newly loaded pads use the track-start
default.

### Requirement: Reset restores a 4-bar auto-loop
**Reason**: The waveform editor no longer exposes Reset for loop regions. `ALL` replaces it with
an explicit full-track region action.

**Migration**: Use `ALL` for full-track playback or set auto-loop bars explicitly for musical loop
lengths.

