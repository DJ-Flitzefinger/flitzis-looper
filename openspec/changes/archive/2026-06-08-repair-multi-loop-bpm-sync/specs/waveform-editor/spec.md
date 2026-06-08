## ADDED Requirements

### Requirement: Loop Editor Source Grid Remains Stable During Playback Sync Changes
The system SHALL keep the Loop Editor source-side grid anchor and snapped loop markers stable when playback sync state changes.

Changing global Pitch/Speed, enabling or disabling BPM Lock, recomputing master BPM, enabling or disabling Key Lock, toggling trigger quantization, changing the trigger quantization step, or starting/stopping/retriggering another pad SHALL NOT move a pad's Loop Editor Grid Offset anchor, snapped loop start, snapped loop end, or visible source-side grid lines unless the performer edits that pad's loop or grid settings.

The Loop Editor grid SHALL remain a source-domain editing grid. The Rust transport timeline and trigger quantization grid MAY share the same 1/64-note unit basis, but they SHALL NOT reinterpret or move the source-side Loop Editor grid.

#### Scenario: Snapped loop start stays on the shifted grid at 1.5x
- **GIVEN** a pad has analysis downbeat metadata
- **AND** the performer sets a non-zero Grid Offset
- **AND** auto-loop snapping stores the loop start on the shifted 1/64-note source grid
- **WHEN** global Pitch/Speed changes to `1.5x`
- **AND** BPM Lock and Key Lock are toggled
- **THEN** the stored loop start remains at the same source time
- **AND** the visible Loop Editor grid anchor remains at the same source time

#### Scenario: Other pad playback does not move the editor grid
- **GIVEN** the Loop Editor is open for pad 1
- **AND** pad 1 has a shifted source-side grid anchor and snapped loop start
- **WHEN** pad 2 starts, stops, or is retriggered
- **THEN** pad 1's grid anchor and snapped loop start remain unchanged

#### Scenario: Trigger quantization does not redefine the source grid
- **GIVEN** a pad has a visible Loop Editor musical grid
- **WHEN** trigger quantization is enabled, disabled, or changed between supported grid steps
- **THEN** the pad's Loop Editor source grid remains unchanged
- **AND** future triggers still use the Rust transport grid only to choose output start time
