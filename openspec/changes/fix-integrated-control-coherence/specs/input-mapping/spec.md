## ADDED Requirements

### Requirement: Mapped input admission failures remain within the input boundary
The system SHALL contain expected Native RuntimeError/ValueError/TypeError failures from a recognized keyboard or MIDI controller action at its input event boundary, expose the error through existing mapping feedback and continue processing later independent input. Explicit public timestamp validation SHALL remain enforced. Direct controller APIs SHALL retain their meaningful failure contract.

No failed event SHALL be recaptured against a later selected/replacement source, silently replayed, translated to an unguarded fallback or acknowledged as effective. Learning, original captured pitch/trigger ownership and input timestamps SHALL remain on their existing authoritative paths. Successful explicit actions SHALL clear only the relevant prior admission feedback according to the existing feedback contract.

#### Scenario: A keyboard global STOP is refused during source publication
- **GIVEN** global STOP is bound to a keyboard key and Native publication stays busy
- **WHEN** the performer presses that key
- **THEN** the GUI remains alive and mapping feedback reports the admission failure
- **AND** active/paused/restore state and original pending owners are not fabricated

#### Scenario: One MIDI fallback action fails before a later independent event
- **GIVEN** a mapped pad STOP fails Native admission
- **WHEN** the input poll continues to another independently delivered event
- **THEN** the failure is visible without terminating the GUI or replacing captures

### Requirement: Native MIDI selection requires delivery of its original UI receipt
The Native dispatcher SHALL publish a selected-content mutation only if the corresponding original selection receipt has been accepted by its existing bounded event channel. Delivery and mutation SHALL share the existing runtime-state exclusion so a receipt consumer cannot observe a half-published selection. Full or disconnected delivery SHALL leave Native selection and its revision unchanged. The implementation SHALL NOT compensate by guessing a later GUI pad, rolling back a newer selection or capturing a replacement owner.

#### Scenario: A MIDI pad selection meets a full event channel
- **GIVEN** UI and Native agree on selected content A and the output event channel is full
- **WHEN** a mapped input attempts to select content B
- **THEN** no selection mutation or revision is admitted without its deliverable receipt
- **AND** subsequent selected keyboard/MIDI pitch cannot secretly target B

#### Scenario: Selection receipt delivery succeeds
- **GIVEN** the original mapped selection has a current source and an available event slot
- **WHEN** its receipt is delivered under the runtime-state exclusion
- **THEN** Native commits the matching original revision before a consumer validates it
- **AND** the existing Python projection follows that exact receipt normally

### Requirement: MIDI selection retains original input admission through commit
The actual MIDI SelectPad dispatcher SHALL validate its original mapping owner and input admission incarnation, enabled state and Learn exclusion immediately before delivering the selection receipt and committing Native selection under the existing runtime exclusion. Revocation after lookup but before this admission SHALL leave selection and its revision unchanged. Returning flags or mappings to their previous values SHALL NOT revive the old gesture, and Python SHALL NOT replay a refused selection against current content.

#### Scenario: Input is revoked while original selection waits for runtime state
- **GIVEN** the actual dispatcher retained an original SelectPad mapping but has not committed selection
- **WHEN** that mapping is replaced, input is disabled or Learn becomes active before commit
- **THEN** its original receipt reports refusal and Native selection remains unchanged
- **AND** only a later independent gesture can acquire fresh input authority

#### Scenario: Original input state is restored before selection commit
- **GIVEN** a disable/enable, Learn cycle or mapping replacement revoked an original selection gesture
- **WHEN** the waiting gesture reaches commit after the old flags or action text were restored
- **THEN** the changed admission incarnation still rejects that gesture without replay

### Requirement: Fresh selected pitch captures use current same-source authority
The input runtime SHALL preserve an explicit selected content instance across same-source Loop, BPM and grid-origin authority edits. A new selected keyboard or MIDI pitch capture SHALL use the current validated binding authority without inventing a new selection revision. Previously captured actions SHALL retain their original authority and become stale when that authority changes. The runtime SHALL NOT follow replacement source requests, reused PCM with a new request incarnation, new content, disabled mappings or Learn transitions through this refresh path.

#### Scenario: A selected pad receives a same-source control edit
- **GIVEN** explicit pad capture works for the unchanged selected source after a control edit
- **WHEN** a fresh selected keyboard or MIDI pitch gesture is captured
- **THEN** it binds that current source authority and retains its original input timestamp
- **AND** older captured actions remain rejected and the user selection revision is unchanged

#### Scenario: A new source request reuses the same PCM buffer
- **GIVEN** the selected content cell now carries a different source request incarnation
- **WHEN** a selected pitch gesture is captured without a new explicit selection
- **THEN** it is rejected even if PCM address and content cell happen to match
### Requirement: Direct MIDI stop retains original mapping admission authority
Direct MIDI stop SHALL validate its original mapping owner, enabled state and Learn exclusion at its actual control-producer admission. Revocation before that admission SHALL leave launch cancellation, audio commands and source placement untouched. A stop already validly admitted SHALL retain its original placement and event receipt through later input-state changes; it SHALL NOT retarget a new mapping or replacement content. Restoring input state SHALL NOT replay a refused stop.

#### Scenario: Mapping retirement, disable or Learn precedes stop admission
- **GIVEN** a stop gesture resolved its original mapping before input state changed
- **WHEN** its admission is attempted after that original authority was revoked
- **THEN** no stop or launch cancellation is admitted and no automatic replay occurs

#### Scenario: An admitted stop precedes replacement of its content
- **GIVEN** a stop validly admitted for its original source placement
- **WHEN** input state or content changes before callback execution
- **THEN** the original receipt remains truthful and replacement content is not substituted

#### Scenario: Input state returns to its prior flags before stop admission
- **GIVEN** an original stop lookup preceded a disable/enable or Learn/on-off cycle
- **WHEN** the old stop reaches producer admission after the flags match their old values
- **THEN** the changed input admission incarnation rejects that old stop
- **AND** existing queued launches and original content remain untouched
- **AND** a fresh stop lookup may use the new stable incarnation normally

#### Scenario: A refused direct MIDI stop receipt reaches Python
- **GIVEN** Native refused the original direct stop and delivered dispatched=false
- **WHEN** Python handles that original receipt with a current or replacement pad
- **THEN** it reports the refusal and never performs a second stop or launch cancellation
- **AND** later independent events continue and no automatic replay is scheduled
