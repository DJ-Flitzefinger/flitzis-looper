## ADDED Requirements

### Requirement: Pitch Learn stores stable immutable action variants
The system SHALL provide separate stable SET and SET_RETRIGGER semantic actions for every supported absolute Key and extra integer including0 and SHALL persist the variant selected during Learn.

Actions SHALL encode pitch class/mode or signed semitones rather than menu position.
Checkbox changes SHALL affect GUI and newly learned variant only. Both variants
for the same target SHALL be simultaneously learnable and restorable.

#### Scenario: Two bindings survive checkbox and pad changes
- **WHEN** SET0 and SET_RETRIGGER0 are learned on different keys and checkbox/selection later changes
- **THEN** each saved binding keeps its semantic target/variant through save/reopen

### Requirement: Pitch input captures selected content at authoritative admission
The system SHALL capture explicit GUI content or authoritative native selected ContentInstance/lifetime for MIDI with unique accepted action sequence, original timestamp, full numeric/render tuple, source/timing/native/prepared permit and existing trigger intent.

Direct native, controller fallback and pending retries SHALL use the same captured
envelope and selection-to-pitch ordering across existing ports/channels/MultiLoop.
Merely queued polling/preparation SHALL NOT be described as accepted action.
Selection after admission SHALL NOT redirect the event. Move/Swap SHALL carry live
identity; removal/reassignment/overwrite SHALL fence its old lifetime.

#### Scenario: Selection from controller then pitch from keyboard
- **WHEN** one routed device selects A and another admits a pitch action, followed by selecting B
- **THEN** the admitted action belongs to A with original timestamp in direct/fallback/pending paths
- **AND** existing device/channel collision rules are tested rather than invented per-device bindings

#### Scenario: Move or overwrite after admission
- **WHEN** A moves/swaps or its lifetime is removed before the event applies
- **THEN** it follows live A or terminates fenced, never targets the former slot's replacement

### Requirement: Pitch note releases have no attack or reset semantics
The system SHALL treat MIDI NoteOff and NoteOn velocity0 as nonattack/nonreset for new pitch actions and SHALL preserve chosen pitch/highlights without resuming a mouse waveform hold.

#### Scenario: Release after repeated attacks
- **WHEN** repeated equal-target NoteOn attacks are followed by NoteOff or velocity0
- **THEN** attacks remain distinct and release neither adds attack nor resets pitch0/highlight/hold
