## ADDED Requirements

### Requirement: Input Mapping Settings Controls
The system SHALL provide Settings controls for enabling input mapping and clearing keyboard or
MIDI mappings.

The Settings page SHALL include `Input Mapping: ON/OFF`, `Delete all Keyboard Mappings`, and
`Delete all MIDI Mappings`. It SHALL NOT add a mapping editor, MIDI device selector, MIDI output,
LED feedback, conflict dialog, `Open midi.json`, or `Open keyboard.json` in this change.

#### Scenario: Settings toggles input mapping
- **GIVEN** the Settings page is open
- **WHEN** the performer turns Input Mapping on
- **THEN** project state records input mapping as enabled
- **AND** Python publishes the enabled state to the Rust input layer

#### Scenario: Settings clears all MIDI mappings
- **GIVEN** the Settings page is open
- **WHEN** the performer activates `Delete all MIDI Mappings`
- **THEN** `config/input/midi.json` is rewritten with `mappings = []`
- **AND** the Rust MIDI mapping snapshot is refreshed

#### Scenario: Settings clears all keyboard mappings
- **GIVEN** the Settings page is open
- **WHEN** the performer activates `Delete all Keyboard Mappings`
- **THEN** `config/input/keyboard.json` is rewritten with `mappings = []`
- **AND** normal keyboard playback no longer resolves the cleared mappings

### Requirement: Learn Control Is Direct And Non-Destructive
The system SHALL provide a direct Learn control without replacing the performance surface.

The `L` control SHALL activate Learn, show active/pending state through session/UI state, and clear
text input focus when Learn starts. The Learn control SHALL NOT open a separate mapping editor or
require a MIDI device selector.

#### Scenario: Learn starts from bottom bar
- **GIVEN** input mapping is enabled
- **WHEN** the performer activates `L`
- **THEN** Learn waits for one keyboard or MIDI input
- **AND** text input focus is cleared
