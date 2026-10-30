## MODIFIED Requirements

### Requirement: Waveform Grid Shares The Trigger Quantization Basis
The system SHALL keep the waveform editor musical grid and loop snapping aligned to the same
1/64-note unit basis used by trigger quantization and Rust pad timing metadata.

The finest editor grid interval SHALL remain one sixteenth of a beat in 4/4. The editor SHALL use
the same effective BPM as loop duration/snapping, choosing the manual override before analysis BPM.
Its origin SHALL be the analysis onset rounded in the loaded source sample-rate domain plus the
persisted signed `grid_offset_samples`. The system SHALL publish that same finite signed origin to
Rust without clamping a negative origin or changing its musical phase. The origin is a source-domain
grid reference and need not be a readable audio frame. Invalid analysis data SHALL use the existing
downbeat, beat, zero fallback before adding the signed offset.

Adjusting Grid Offset, changing the effective pad BPM or restoring its durable metadata SHALL
update bounded native timing metadata. Starting/stopping another pad, speed, BPMLOCK, KEYLOCK or
quantization controls SHALL NOT move source grid lines or persisted loop markers.

#### Scenario: Finest editor grid matches native subdivision
- **GIVEN** a pad has valid effective BPM
- **WHEN** the finest editor subdivision and native 1/64-note interval are calculated
- **THEN** both intervals represent one sixteenth of a beat in their respective frame domains

#### Scenario: Negative editor origin reaches native timing intact
- **GIVEN** the analysis onset is zero and Grid Offset is -4,800 samples at 48,000 Hz
- **WHEN** the system publishes the editor grid origin
- **THEN** native metadata retains the signed origin of -0.1 seconds
- **AND** the editor grid and Rust source phase agree
- **AND** persisted loop markers are unchanged

#### Scenario: Restore preserves signed phase and manual tempo
- **GIVEN** a project stores a negative grid offset and a manual BPM override
- **WHEN** the matching source and durable metadata are restored
- **THEN** the editor and native timing use the same signed origin and manual BPM
- **AND** a project without a grid-offset field continues to use zero offset

#### Scenario: Global controls do not reinterpret source grid
- **GIVEN** a pad has a corrected signed source grid and snapped loop markers
- **WHEN** speed, BPMLOCK, KEYLOCK, quantization or other-pad playback changes
- **THEN** the source grid and persisted markers remain at the same source positions
