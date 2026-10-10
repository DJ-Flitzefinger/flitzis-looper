## MODIFIED Requirements

### Requirement: Stem Source Changes Use Bounded Transition State
The system SHALL apply accepted active mode/mask changes, including the first
lazy activation of a valid selected stored set, through existing bounded
Rust-owned scalar source-selection and ramp state.

First missing residency SHALL be prepared off-thread and pass a dedicated
source/cache/set/ticket/voice/window/geometry/history/DSP/lease guarded native
adoption ACK before effective selection changes. FullMix SHALL remain effective
while pending/error. A queued mode command SHALL NOT establish effective-mode
acknowledgement. Offline generation MAY run under immutable leases while playing; active different-content replacement SHALL remain guarded until E11-05/19 separately prove new-set current-voice/history/own-ACK continuity. Existing128-source-frame crossfade and fixed-size state SHALL
preserve playhead, loop, voice, transport/quantization and musical timing without
stop/restart. No audio payload/path/dynamic chain or unbounded work enters ramps.

#### Scenario: Active full-mix to all-stems change crossfades
- **WHEN** a playing FullMix pad selects an already accepted current stem set
- **THEN** existing bounded transition reads accepted buffers at the same source frame
- **AND** effective feedback and source continuity remain intact

#### Scenario: Active stem mask change crossfades
- **WHEN** a current ALL STEMS voice changes component mask
- **THEN** bounded Rust transition crossfades old/new selections without retrigger/time slip

#### Scenario: First lazy switch while FullMix plays
- **GIVEN** FullMix plays with a valid selected disk set and no resident stem windows
- **WHEN** ALL STEMS is requested
- **THEN** FullMix continues during off-thread preparation
- **AND** genuine guarded residency and effective-mode feedback precede completion
- **AND** both crossfade sides use the same advancing source frame without restarting

#### Scenario: Mode enqueue succeeds but adoption rejects
- **WHEN** source/voice/history/window/STOP ownership invalidates a queued selection
- **THEN** the system preserves current effective audio and reports rejected/pending state
- **AND** UI command acceptance cannot claim effective ALL STEMS

#### Scenario: Active mask retains existing transition behavior
- **WHEN** a valid current ALL STEMS pad changes its component mask
- **THEN** bounded scalar transition changes selection at the current source position
- **AND** voice, loop and transport remain continuous
