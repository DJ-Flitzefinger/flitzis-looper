## ADDED Requirements

## MODIFIED Requirements

### Requirement: Waveform editor provides transport and navigation controls

The waveform editor SHALL provide control buttons in an upper area above the waveform display.

The toolbar SHALL start with five icon-only buttons (no text labels), ordered left-to-right as follows:

- Pause: icon is two vertical bars; pauses loop playback of the selected pad if it is currently playing
- Play: icon is a right-pointing triangle; starts loop playback of the selected pad AND ALWAYS restarts from the loop start each time it is pressed
- Stop: icon is a square; stops loop playback of the selected pad AND resets the pad playhead to the loop start immediately
- View-Jump-Start: icon is a left-pointing triangle; moves ONLY the waveform VIEW to the start of the full track for the selected pad; MUST NOT change playback state
- View-Jump-End: icon is a right-pointing triangle; moves ONLY the waveform VIEW to the end of the full track for the selected pad; MUST NOT change playback state

All five buttons SHALL behave as triggers:

- The action SHALL execute on press (mouse-down), not on mouse release.

All other, already-existing waveform editor toolbar controls SHALL remain unchanged and SHALL be positioned to the right of these five buttons.
The previously existing Play/Pause UI control in the waveform editor toolbar SHALL be removed from the UI in favor of the buttons above (reusing existing logic as needed).

#### Scenario: Play trigger restarts from loop start on press

- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses Play in the waveform editor (mouse-down)
- **THEN** Pad A playback continues but restarts from the loop start immediately

#### Scenario: Stop stops playback and resets to loop start on press

- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses Stop in the waveform editor (mouse-down)
- **THEN** Pad A playback stops immediately
- **AND** Pad A playhead is reset to the loop start immediately

#### Scenario: Playback controls affect only the selected pad

- **GIVEN** Pad A is selected
- **AND** another pad (Pad B) is active
- **WHEN** the performer presses Play, Pause, or Stop in the waveform editor (mouse-down)
- **THEN** only Pad A playback state changes
- **AND** Pad B playback is not stopped by this action

#### Scenario: View-Jump controls do not affect playback

- **GIVEN** Pad A is selected
- **AND** Pad A is currently playing
- **WHEN** the performer presses View-Jump-Start or View-Jump-End (mouse-down)
- **THEN** only the waveform view scroll/pan position changes for Pad A
- **AND** Pad A playback state does not change
- **AND** other pads’ playback is not stopped by this action

## REMOVED Requirements

## RENAMED Requirements
