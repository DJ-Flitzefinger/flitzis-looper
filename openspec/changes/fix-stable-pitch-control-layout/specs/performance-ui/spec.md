## ADDED Requirements

### Requirement: Stable Pitch control geometry during asynchronous feedback

The system SHALL keep the Pitch fader rectangle and adjacent control geometry unchanged for an unchanged sidebar viewport and UI scale when asynchronous rate or Key Lock feedback changes. Diagnostic strings and receipt states MUST NOT participate in the Pitch fader's position or size calculation. A held stationary pointer MUST NOT change the requested speed because feedback changes the layout. The system SHALL bound the fader height using stable viewport and font geometry so the step, Reset and lock controls remain accessible at the normal supported window height. Requested and audio-confirmed speed SHALL remain distinguishable through the BPM display hover details without inserting variable-height rows. Existing rate, BPM step, Learn, reset and center-marker semantics SHALL remain.

#### Scenario: Feedback changes while a performer holds the Pitch fader

- **GIVEN** a performer drags the Pitch fader and holds the pointer stationary
- **WHEN** rate feedback changes between queued, preparing, applied and rejected
- **THEN** the fader rectangle and adjacent control rectangles remain unchanged
- **AND** the stationary pointer does not introduce a new requested speed

#### Scenario: Narrow sidebar receives long diagnostic feedback

- **GIVEN** the expanded right sidebar at an unchanged viewport and UI scale
- **WHEN** asynchronous feedback includes a long diagnostic string
- **THEN** the diagnostic does not change fader geometry or toggle sidebar layout
- **AND** the BPM hover details keep requested and confirmed audio speed separate

### Requirement: English Key Lock start settings

The system SHALL present both Key Lock start policies and their explanations in English, using "First sound at grid" and "Attack at grid" as the policy labels. The stopped-playback editing hint SHALL also be English. Policy behavior and persisted values MUST remain unchanged.

#### Scenario: Performer opens Key Lock start settings

- **WHEN** the performer opens Settings
- **THEN** the policy chooser is labeled "Key Lock start behavior"
- **AND** both policy choices and their explanations are English
- **AND** playback disables changes with the hint "Stop playback to change"
