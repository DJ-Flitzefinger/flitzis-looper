## MODIFIED Requirements

### Requirement: Waveform editor displays a single musical grid aligned to loop snapping
The system SHALL render one musical grid aligned to the current editor timing mode's snapping
coordinates, without a concurrent non-musical time grid.

Scalar mode SHALL retain manual BPM before analysis BPM, `beat_sec=60/effective_bpm`, and the
signed source origin `persisted_base_or_legacy_onset_sec + grid_offset_sec`. Its lines SHALL use
`origin + n*minor_step_sec`. Missing effective BPM SHALL disable the scalar musical grid.

Explicitly accepted variable-map mode SHALL derive line source positions from the shared
inverse map, not average BPM. It SHALL render only supported coverage and label unsupported
meter/coverage rather than fabricate a bar grid. Initially musical bar operations SHALL require
verified quarter-note 4/4 interpretation. The finest interval SHALL remain 1/16 beat (1/64 note).

Both modes SHALL choose visible subdivisions from 16, 4, 1, 1/2, 1/4, 1/8 and 1/16 beats;
1/32-note and 1/64-note names remain aliases of 1/8 and 1/16 beat. The finest visible step with
adjacent projected minor lines at least 12 pixels apart SHALL be selected, falling back to
16 beats when none fits. Variable mode SHALL evaluate spacing over the visible coverage using
bounded control-side projections. It SHALL NOT assume constant source-time spacing.

Major emphasis SHALL remain every four bars for one-bar minor steps, every bar for one-beat
minor steps, and every beat for finer steps. Extreme zoom SHALL support 1/64-note lines when
the readability constraint permits.

#### Scenario: Default zoom shows bars only with stronger 4-bar lines
- **GIVEN** scalar mode with valid BPM and one-beat lines closer than 12 pixels
- **AND** one-bar lines are at least 12 pixels apart
- **WHEN** the grid is rendered
- **THEN** one-bar minor lines and stronger four-bar lines are shown without a second grid

#### Scenario: Medium zoom shows beats with stronger bar lines
- **GIVEN** scalar mode and valid BPM
- **WHEN** one beat is the finest readable subdivision
- **THEN** beats are minor lines and bars are major lines

#### Scenario: Close zoom shows 1/16-note lines with stronger beat lines
- **GIVEN** scalar mode and valid BPM
- **WHEN** one-quarter beat is the finest readable subdivision
- **THEN** 1/16-note lines are minor and beats are major

#### Scenario: Extreme zoom shows 1/64-note lines
- **GIVEN** supported timing coverage and readable 1/16-beat projected spacing
- **WHEN** the grid is rendered
- **THEN** 1/64-note minor lines are shown using that mode's source coordinates

### Requirement: Waveform Grid Shares The Trigger Quantization Basis
The system SHALL keep scalar and accepted variable-map editor grids on the same quarter-note
beat unit and 1/64-note subdivision basis as Rust timing and trigger quantization.

Scalar mode SHALL retain the manual-before-analysis BPM, optional persisted base (otherwise legacy analysis onset) rounded at loaded rate,
and persisted signed grid offset. The same finite signed scalar origin SHALL be published to
Rust without negative clamping. Invalid onset metadata SHALL retain downbeat/beat/zero fallback
before applying the offset. That virtual origin need not be a readable audio frame.

Variable-map mode SHALL use the shared versioned evaluator for source positions; average BPM
SHALL NOT substitute for it. In this stage the map SHALL remain editor/diagnostic-only and its
revision SHALL be distinguished from scalar live state. Sharing musical units SHALL NOT be
presented as proof of active map-driven audio or audible synchronization.

Scalar offset/BPM changes and restore SHALL retain bounded native scalar publication. Global
speed, BPMLOCK, KEYLOCK, quantization and other-pad playback SHALL NOT move source grid lines or
persisted markers. Accepting a new variable-map revision SHALL NOT by itself move markers.

#### Scenario: Finest loop editor line spacing matches minimum quantization
- **GIVEN** supported scalar or accepted variable-map timing coverage
- **AND** the finest grid is readable at the current zoom
- **WHEN** the grid is rendered
- **THEN** adjacent finest lines differ by one sixteenth of a quarter-note beat
- **AND** trigger quantization uses the same musical subdivision for its `1/64` setting
- **AND** shared units do not claim that variable-map playback has been activated

#### Scenario: Grid offset updates Rust pad timing metadata
- **GIVEN** a loaded pad in scalar mode with valid effective BPM
- **WHEN** the performer changes its Grid Offset
- **THEN** source grid lines shift by that loaded-sample offset
- **AND** the identical finite signed origin is published as bounded Rust pad timing metadata

#### Scenario: Negative scalar origin survives publication and restore
- **GIVEN** a legacy project with zero onset, -4800 sample offset at 48 kHz and manual BPM
- **WHEN** its state is published or restored
- **THEN** scalar native origin is -0.1 seconds and manual BPM is preserved
- **AND** persisted markers are unchanged; missing legacy offset defaults to zero

#### Scenario: Other pad playback does not move the editor grid
- **GIVEN** an accepted scalar or variable source grid and saved loop markers
- **WHEN** another pad starts, stops, pauses, retriggers or unloads
- **THEN** the edited pad's source grid and markers remain at the same source positions

#### Scenario: Quantize toggle does not move the editor grid
- **GIVEN** an accepted scalar or variable source grid and saved loop markers
- **WHEN** quantization is enabled, disabled or changed between supported subdivisions
- **OR** global speed, BPMLOCK or KEYLOCK changes
- **THEN** that source grid and its markers remain at the same source positions

## ADDED Requirements

### Requirement: Current Pad Tempo Is Distinct From Master And Track Summary
The system SHALL display the local source tempo at the current pad playhead in accepted
variable-map mode, derived from the same Rust evaluator and map revision as the grid.
It SHALL distinguish this source-tempo value from master target BPM and any whole-track BPM
summary. A summary or first detected BPM SHALL NOT be presented as current local tempo.

The local source tempo SHALL follow the map's quarter-note slope at the reported source
position, including after seeks and loop wraps. Missing or uncertain coverage SHALL be
identified explicitly instead of silently substituting a track summary. A stopped pad SHALL
use its retained playhead position. Display rounding or refresh cadence SHALL NOT drive SYNC;
future SYNC SHALL use the complete map and master timeline independently of UI updates.

#### Scenario: A variable track reaches a faster segment
- **GIVEN** an accepted map whose local source tempo changes from 100 to 110 BPM
- **AND** master target tempo is 120 BPM
- **WHEN** the pad playhead crosses into the faster segment
- **THEN** local source BPM reflects 110 rather than the initial or whole-track BPM
- **AND** master target BPM remains separately identifiable as 120
- **AND** the display does not claim the source tempo equals the future synchronized output tempo

#### Scenario: Playhead enters unsupported map coverage
- **WHEN** the current source position has no supported local tempo estimate
- **THEN** the local tempo display reports unavailable or uncertain coverage
- **AND** it does not label a saved summary as the current tempo

### Requirement: Accepted Variable Maps Use One Editor Coordinate Evaluator
The system SHALL derive visible beat coordinates and snap positions for an explicitly accepted
variable beatmap through the same Rust map evaluator and revision used by control-side phase
diagnostics.

Python SHALL render bounded projections and send editing intent without implementing another
source-to-beat interpolation. Unsupported/uncertain coverage SHALL remain visible as such;
variable map acceptance SHALL NOT silently replace existing scalar-mode projects.

#### Scenario: Display and snapping share a variable segment
- **GIVEN** an accepted map with unequal neighboring beat intervals
- **WHEN** the editor draws a beat line and snaps a loop marker to that beat
- **THEN** both operations resolve to the same source coordinate within loaded-frame rounding
- **AND** neither operation derives its position from average BPM

### Requirement: Map Correction Does Not Move Master Time
The system SHALL keep whole-map source alignment, local beat corrections and creative playback
phase intent distinct, and SHALL NOT reanchor master transport or other playing pads through
editor map corrections.

Whole-map translation SHALL preserve beat intervals. Local corrections SHALL validate monotone
anchors and create a new revision. Existing source-time markers SHALL remain unchanged until
an explicit marker edit or resnap. Manual corrections SHALL survive reanalysis unless replaced
through explicit user intent.

#### Scenario: Signed alignment is changed while another pad plays
- **WHEN** a valid signed alignment correction is accepted for the edited source
- **THEN** its map revision changes without changing master position or the other pad's source
- **AND** persisted loop markers do not move merely because map coordinates changed

### Requirement: Pending Map Revisions Are Distinguished From Live State
The system SHALL distinguish the editor's accepted map revision from any revision currently
used for audible playback, and SHALL keep variable maps editor/diagnostic-only in this stage,
including after stop or retrigger. Audible map adoption SHALL require a later behavior change.

This change SHALL NOT silently seek active voices or claim variable-map playback is active.
Map construction/evaluation and UI snapshot work SHALL remain outside the audio callback.

#### Scenario: A map is edited during existing scalar playback
- **WHEN** the editor accepts a new variable map revision
- **THEN** playback continues with its existing addressing
- **AND** the UI identifies the revision as not yet adopted by audio
- **AND** no callback allocation, locking, GIL access or analysis is introduced

### Requirement: Accepted Beat Anchors Support Sample-Domain Correction
The system SHALL support explicit source-sample positioning of accepted variable-map anchors
while preserving raw detections, correction lineage and musical uncertainty separately.

Missing/extra beat count, downbeat labels and local source positions SHALL be distinct correction
operations. Optional automatic refinement SHALL use bounded, versioned evidence and SHALL NOT
equate the nearest transient with a correct beat in ambiguous, silent or syncopated passages.
Grid display zoom SHALL NOT change stored precision. Source-key metadata or future semitone
transposition SHALL NOT move accepted anchors or change their map revision.

#### Scenario: A performer corrects an estimated anchor by one source sample
- **GIVEN** an automatic anchor with retained model timestamp and uncertainty
- **WHEN** an explicit sample-domain edit is accepted
- **THEN** the chosen source index/rate and new map revision are recorded
- **AND** raw prediction remains available without being relabeled a perfect automatic detection

#### Scenario: An ambiguous beat has no reliable onset
- **WHEN** local refinement lacks convincing evidence for a supported new position
- **THEN** it retains uncertainty and does not force that beat onto a nearby transient

### Requirement: Variable Map Numbering Uses The Selected Loop Reference
The system SHALL display supported variable-map musical coordinates as
1+B(source)-B(loop_start), preserving the source map when the loop is moved.
Future SYNC SHALL use continuous native loop-relative musical phase and the
master timeline rather than integer UI line labels or display refresh cadence.
Unsupported coverage SHALL remain unavailable rather than invent an initial beat.

#### Scenario: A later loop is chosen on a variable map
- **GIVEN** supported map coverage at the new loop start
- **WHEN** the performer moves the loop to that position
- **THEN** the new loop reference is displayed as 1 without translating the map
- **AND** other pads retain their own maps and the shared master timeline
