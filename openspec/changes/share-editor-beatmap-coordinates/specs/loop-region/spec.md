## MODIFIED Requirements

### Requirement: Auto-loop defines loop end by bars in 4/4
The system SHALL calculate an explicitly requested auto-loop end in the selected editor timing
mode using four quarter-note beats per bar.

Scalar mode SHALL preserve duration `bars*4*60/effective_bpm` with manual BPM before analysis
BPM, including its existing recalculation on effective-BPM changes; without valid BPM it SHALL
leave musical duration unavailable. Accepted variable-map mode
SHALL compute `S(B(loop_start)+4*bars)` through the shared evaluator, round the resulting source
marker at the loaded sample rate, and require supported 4/4 coverage over the whole interval.
Unknown coverage/meter SHALL make the musical operation unavailable rather than fall back to
an unlabeled average tempo. Existing source-duration clamping SHALL remain explicit.

Existing half-bar granularity SHALL remain available. Accepting/editing a map revision alone
SHALL preserve already stored source-time markers; variable-map recomputation SHALL require
explicit loop start/bar-count/reset/resnap intent. This SHALL NOT activate variable-map audio progression.

#### Scenario: Auto-loop computes loop end from BPM
- **GIVEN** scalar mode at 120 BPM with start 10 seconds
- **WHEN** an auto-loop operation requests four bars
- **THEN** its end is 18 seconds subject to available source duration

#### Scenario: Variable map follows the actual sixteen-beat interval
- **GIVEN** supported 4/4 map coverage with B(start)=8 and S(24)=19.2 seconds
- **WHEN** four bars are requested from that start
- **THEN** the end is the loaded-frame-quantized position corresponding to 19.2 seconds
- **AND** average BPM does not determine the result

#### Scenario: Auto-loop is unavailable without BPM
- **GIVEN** scalar mode without valid manual or analysis BPM
- **WHEN** auto-loop controls are evaluated
- **THEN** musical duration is unavailable and saved source markers remain intact

#### Scenario: Unsupported coverage does not invent a musical duration
- **GIVEN** insufficient variable-map coverage for the requested interval
- **WHEN** auto-loop controls are evaluated
- **THEN** the operation is indicated as unavailable and saved source markers remain intact

### Requirement: Beat snapping rules
The system SHALL snap auto-loop marker edits to the nearest 1/16-beat point in the selected
editor timing mode and then quantize the chosen source position to an integer loaded-frame index.

Scalar mode SHALL retain `beat_sec=60/effective_bpm`, `grid_step_sec=beat_sec/16` and signed
origin `persisted_base_or_legacy_onset_sec+grid_offset_sec`, with manual BPM before analysis. Without valid
scalar BPM it SHALL perform no musical snap.

Accepted variable-map mode SHALL evaluate B(marker), select the nearest musical 1/16-beat point
and invert through the shared S, using the same map revision as display/auto-loop duration.
Exact halfway ties SHALL select the later musical point in that mode. Unsupported coverage
SHALL leave the attempted musical operation unavailable; it SHALL NOT use average BPM instead.

Stored marker time SHALL equal `sample_index/loaded_sample_rate`. When auto-loop is disabled,
no musical snap SHALL be applied and the existing sample-accurate marker contract SHALL remain.
Explicit marker changes SHALL retain immediate physical-loop publication; accepting a new map
alone SHALL NOT resnap or republish a moved region.

#### Scenario: Auto-loop enabled loop start is exactly sample-accurate on 1/64 grid
- **GIVEN** auto-loop, 120 BPM, origin 10 seconds and loaded rate 48000 Hz
- **WHEN** start is set near 10.031 seconds
- **THEN** its snapped position is 10.03125 seconds
- **AND** its sample index is 481500

#### Scenario: Auto-loop enabled loop end is exactly sample-accurate on 1/64 grid
- **GIVEN** auto-loop, 120 BPM, origin 10 seconds and loaded rate 48000 Hz
- **WHEN** end is set near 10.062 seconds
- **THEN** its snapped position is 10.0625 seconds
- **AND** its sample index is 483000

#### Scenario: Variable map snap agrees with the displayed point
- **GIVEN** supported variable-map coverage and an auto-loop marker edit
- **WHEN** the nearest musical point is selected
- **THEN** the stored loaded-frame position equals that displayed point's rounded source position
- **AND** both operations use one map revision

#### Scenario: Auto-loop disabled does not snap
- **GIVEN** auto-loop is disabled
- **WHEN** a marker is edited
- **THEN** the existing physical sample-accurate operation is used without musical snapping
