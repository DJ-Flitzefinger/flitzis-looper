## MODIFIED Requirements

### Requirement: Beat snapping rules
When auto-loop is enabled, loop start and loop end adjustments SHALL snap to the nearest musical 1/64-note grid point derived from the effective BPM.

The effective BPM used for snapping SHALL be the same effective BPM used for auto-loop duration computation (manual BPM override first, else analysis BPM).

If no effective BPM is available, the system SHALL NOT perform musical grid snapping.

A 1/64-note grid interval MUST be defined as 1/16 of a beat.
In seconds, the interval SHALL be computed as:
- `beat_sec = 60 / effective_bpm`
- `grid_step_sec = beat_sec / 16`

**Grid anchor (sample-shifted)**
The grid MUST be anchored at `grid_anchor_sample`, defined as:
- `grid_anchor_sample = default_onset_sample + grid_offset_samples`

Where:
- `default_onset_sample` is the default onset time (per "Default loop region uses analysis downbeat onset") quantized to an integer sample index at the cached WAV sample rate.
- `grid_offset_samples` is a signed per-pad offset in samples (default 0).

For snapping computations, the anchor time SHALL be:
- `grid_anchor_sec = grid_anchor_sample / sample_rate_hz`

Snapping MUST use this shifted anchor so that waveform-editor grid visualization and loop marker snapping are aligned.

After snapping to the musical grid, the stored marker position MUST be quantized to an integer sample index at the cached WAV sample rate, and the stored marker time MUST be exactly `sample_index / sample_rate_hz` (i.e., the system MUST NOT store fractional-sample timestamps).

When auto-loop is disabled, loop start and loop end adjustments SHALL not snap and SHALL be free to any time value.

**Grid offset clamping (+/- 1 bar)**
When both an effective BPM and a cached WAV sample rate are available, `grid_offset_samples` MUST be clamped to +/- 1 bar worth of samples.

One bar MUST be defined as 4 beats (4/4), and the bar duration SHALL be:
- `bar_sec = beat_sec * 4`

The clamp limit in samples SHALL be derived by converting bar duration to samples at the cached WAV sample rate:
- `bar_samples = round(bar_sec * sample_rate_hz)`

The system MUST clamp `grid_offset_samples` to the inclusive range `[-bar_samples, +bar_samples]`.

If the effective BPM changes, the system MUST recompute `bar_samples` and re-clamp the stored `grid_offset_samples` to the new range (and use the clamped value for snapping).

#### Scenario: Auto-loop enabled uses shifted anchor and remains sample-accurate
- **GIVEN** auto-loop is enabled
- **AND** the effective BPM is 120
- **AND** the cached WAV sample rate is 48000 Hz
- **AND** the default onset is 10.0 seconds (so `default_onset_sample = 480000`)
- **AND** `grid_offset_samples = +1`
- **WHEN** the performer sets loop start to exactly 10.0 seconds
- **THEN** the stored loop start corresponds to sample index 480001 at 48000 Hz
- **AND** the stored loop start time is exactly `480001 / 48000` seconds

#### Scenario: Grid offset is clamped to one bar worth of samples
- **GIVEN** the effective BPM is 120
- **AND** the cached WAV sample rate is 48000 Hz
- **AND** one bar is 2.0 seconds (96000 samples)
- **WHEN** `grid_offset_samples` is set to 100000
- **THEN** the stored `grid_offset_samples` is 96000

#### Scenario: Effective BPM change re-clamps stored grid offset
- **GIVEN** the effective BPM is 120
- **AND** the cached WAV sample rate is 48000 Hz
- **AND** the stored `grid_offset_samples` is 96000
- **WHEN** the effective BPM changes to 240
- **THEN** the bar duration becomes 1.0 seconds (48000 samples)
- **AND** the stored `grid_offset_samples` is re-clamped to 48000

#### Scenario: Auto-loop disabled does not snap
- **GIVEN** auto-loop is disabled
- **WHEN** the performer sets loop start to approximately T
- **THEN** the stored loop start is approximately T and is not modified by musical grid snapping
