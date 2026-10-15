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
The system SHALL capture and guard each pitch action at authoritative admission.

#### Scenario: Immutable action envelope and lifetime fencing
- **WHEN** GUI or MIDI pitch input reaches authoritative admission
- **THEN** the following complete capture and lifetime contracts SHALL apply:

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

### Requirement: Discarded unpublished pitch captures cannot become requested intent
The system SHALL cancel the exact original Native owner when disabled mapping,
Learn or an explicit Learn queue drain discards an unpublished pitch capture.
A later capture SHALL NOT inherit numeric intent solely from a discarded
capture, including when cancellations arrive in FIFO order or separate polls.
Cancellation SHALL preserve newer retained captures and authoritative projected
requested intent. Removal, replacement and timing refresh SHALL NOT restore an
old source's unresolved shadow into a new source; Move SHALL follow content.

#### Scenario: Several discarded captures have no surviving predecessor
- **GIVEN** two or three unpublished extra-pitch captures precede their first cancellation
- **WHEN** disabled mapping or Learn discards every capture in FIFO order, together or across separate polls
- **THEN** each exact original owner SHALL be cancelled without publication
- **AND** later Base SET composition SHALL use the last retained requested anchor and SHALL NOT resurrect any cancelled predecessor
- **AND** the same result SHALL hold for other cancellation orders

#### Scenario: An older discard preserves a newer chosen owner
- **WHEN** an older unpublished pitch capture is discarded while a newer capture remains retained or its intent is authoritatively projected
- **THEN** cancellation SHALL affect only the discarded owner and its predecessor links
- **AND** the newer requested tuple, sequence and eventual effect SHALL remain unchanged

### Requirement: Requested pitch rollback uses bounded control custody
The system SHALL bound unresolved requested-shadow bookkeeping to 64 scalar
records under the existing control owner. These records MAY retain lightweight
atomic status observations but SHALL NOT retain Native adapters, action-work
owners, PCM pins or an unbounded chain of original owners. Journal reconciliation
SHALL remain outside the audio callback and SHALL NOT advance unrelated source,
timing or RuntimeProjection authority.

#### Scenario: Requested-shadow capacity is exhausted and recovered
- **GIVEN** all 64 unresolved control records remain owned
- **WHEN** another pitch capture is attempted
- **THEN** it SHALL reject with capacity before allocating its original owner or changing requested intent
- **AND** cancellation or authoritative settlement SHALL release bounded bookkeeping so a later original capture can succeed

#### Scenario: A stale source cannot leave phantom intent after reconciliation
- **WHEN** an unresolved capture is fenced by a logical source or timing refresh and later cancelled
- **THEN** its record SHALL settle without overwriting the current source's authoritative requested anchor
- **AND** the next capture SHALL compose from that current anchor rather than an unresolved old-source tuple
