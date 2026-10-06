## ADDED Requirements

### Requirement: Long Source Coordinates Preserve Loaded Frames
The system SHALL preserve individual loaded-frame addresses through waveform query bounds,
returned X coordinates, seek commands and playhead telemetry using integer frames or binary64
source seconds throughout the public native/Python path.

Waveform amplitudes MAY remain binary32. Queries SHALL remain clamped to real source data,
including views with signed virtual space. Seek SHALL preserve existing active/paused,
intro/loop/tail and valid-read clamp semantics without changing loop or grid intent.

#### Scenario: One-frame query at a long source position
- **GIVEN** a loaded source at 44,100, 48,000 or 96,000 Hz
- **WHEN** a valid one-frame interval is queried at 600 or 1800 seconds
- **THEN** that addressed frame is returned
- **AND** 16 consecutive frame positions retain 16 distinct X coordinates

#### Scenario: Long seek and playhead round trip
- **GIVEN** an active or paused loaded voice at a supported rate
- **WHEN** it is sought to source frame F using F/rate seconds
- **THEN** native addressing and returned playhead seconds preserve F
- **AND** source bounds and existing seek progression policy are retained

### Requirement: Scalar Editor Consumers Share Source Projection
The system SHALL use one pure scalar source projection for visible grid lines, musical
snapping and automatic loop endpoint calculations, preserving full effective control BPM.

Source origin and period SHALL remain independent of selected loop and display numbering.
Continuous lines MAY fall between physical frames. Physical markers SHALL round an absolute
projected boundary once at the loaded rate, with at most half a frame of rounding error.
Native live BPM/rate precision and output-clock ownership SHALL remain unchanged in this slice.

#### Scenario: Exact manual reference across a long source
- **GIVEN** manual BPM 120, source origin zero and loaded rate 48,000 Hz
- **WHEN** all 1200 quarter-note positions from zero through 599.5 seconds are projected
- **THEN** they coincide with the independent pulse frames n*24000
- **AND** snap and automatic loop endpoints use the same projection

#### Scenario: Fractional grid with virtual origin
- **GIVEN** BPM 119.999 or 123.45 and a finite signed source origin
- **WHEN** subdivisions or later off-grid loops are edited
- **THEN** each physical boundary is rounded from its absolute continuous position
- **AND** repeated rounded-interval stepping is not used
- **AND** loop labels, zoom and playback speed do not move the source grid
