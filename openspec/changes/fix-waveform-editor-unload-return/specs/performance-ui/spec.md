## ADDED Requirements

### Requirement: Unloading Edited Pad Returns To Performance View
The system SHALL close the waveform editor and return the center surface to the performance pad
view when audio is unloaded from the pad currently being edited.

Unloading audio from a different pad MUST NOT close or retarget an open waveform editor for another
loaded pad.

#### Scenario: Sidebar unload returns to pad view
- **GIVEN** the waveform editor is open for pad `id`
- **AND** pad `id` has loaded audio
- **WHEN** the performer activates "Unload Audio" for pad `id`
- **THEN** pad `id` audio is unloaded
- **AND** the waveform editor is closed
- **AND** the center surface renders the performance pad view

#### Scenario: Unloading another pad preserves editor
- **GIVEN** the waveform editor is open for pad `A`
- **AND** pad `A` has loaded audio
- **WHEN** audio is unloaded from pad `B`
- **THEN** the waveform editor remains open for pad `A`
