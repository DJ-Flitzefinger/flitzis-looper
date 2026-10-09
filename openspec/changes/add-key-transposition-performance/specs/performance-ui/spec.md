## ADDED Requirements

### Requirement: Key and Transposition share explicit content state
The system SHALL show one Key + Transposition group with source key/correction, base Key, extra semitones, understandable known-key result and per-content Retrigger on pitch choice checkbox.

#### Scenario: Selected content and closed summaries stay current
- **WHEN** mouse/MIDI changes A or selection switches to B
- **THEN** open highlights and closed summaries render the common selected content state
- **AND** base highlight remains base, extra highlight remains extra, result is nominal when speed coupling applies

### Requirement: Both pitch menus remain independently playable
The system SHALL let each menu header open or close its own compact scrollable menu and SHALL keep both simultaneously open through selections, repeats and MIDI input.

#### Scenario: Perform repeated notes from both surfaces
- **WHEN** both menus are opened and values are repeatedly selected by mouse or MIDI
- **THEN** neither auto-collapses or blocks normal MIDI processing
- **AND** current value remains visible until explicitly changed or its header closes

### Requirement: Keyboard appearance shows root relative signed semitones
The system SHALL center0 among available signed semitone values, distinguish white/black keys relative to the base root, use consistent unique enharmonic pitches and visibly separate -12,0,+12.

The note reference SHALL describe the shifted key root, not a detected first sample
note. Light/dark SHALL mean keyboard key color only, with no green harmony/Camelot/
automatic scale correction. Unknown roots SHALL retain signed semitone use.

#### Scenario: Root changes the white black pattern
- **WHEN** base is C or D
- **THEN**0 corresponds to C or D and+1 to C-sharp or D-sharp respectively
- **AND** octave-separated equal pitch classes remain individually selectable and signed

### Requirement: Chosen value highlights survive MIDI release
The system SHALL keep the last selected base and extra value clearly highlighted on both light/dark keys until their common state changes, including after NoteOff.

#### Scenario: MIDI chooses a value while menus are closed
- **WHEN** MIDI chooses+2 and then NoteOff arrives
- **THEN** closed summary updates without opening and+2 remains highlighted when reopened
- **AND** the base menu does not highlight the result instead of its base
