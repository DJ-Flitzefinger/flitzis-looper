## MODIFIED Requirements

### Requirement: Sidebar Load/Unload Actions For Selected Pad
The system SHALL provide Load Audio for an empty selected pad and Unload Audio
for a loaded selected pad, and SHALL require a warning confirmation before the
performer's Unload Audio action takes effect (E11-16).

Load Audio SHALL retain the existing file dialog and supported audio filters.
The unload warning SHALL capture pad and current content identity. Acceptance
SHALL revalidate that binding and eligibility through the authoritative
controller; it SHALL NOT retarget a newly selected or replaced pad. Cancel,
Escape, stale acceptance or dismissed warning SHALL leave loaded audio,
playback, saved state and resource owners unchanged.

#### Scenario: Unload executes only after bound confirmation
- **GIVEN** loaded pad A is selected
- **WHEN** the performer activates Unload Audio
- **THEN** a warning for pad A and its loaded content appears and no unload occurs
- **WHEN** the performer confirms and the captured content is still eligible
- **THEN** the authoritative unload executes exactly once for pad A

#### Scenario: Cancel or stale content does not unload
- **GIVEN** an unload warning captured pad A content X
- **WHEN** the performer cancels, presses Escape, or content X is replaced/unloaded before acceptance
- **THEN** no unload is authorized by that warning
- **AND** selection of pad B cannot retarget the warning to pad B

#### Scenario: Empty pad keeps loading action
- **GIVEN** the selected pad is empty
- **WHEN** the performer activates Load Audio
- **THEN** the existing audio file selection dialog opens for that pad

### Requirement: Selected-pad sidebar offers loop editing
The system SHALL place Adjust Loop between the selected-pad Pad and BPM sections
with matching horizontal separators, using the existing authoritative waveform
editor toggle (E11-17).

The action SHALL be available for a loaded selected pad, open its editor when
closed and close the existing open editor through the same close path. It SHALL
be separate from the lower Unload/Analyze action section and SHALL NOT change
playback or timing merely by opening or closing the editor.

#### Scenario: Adjust Loop has its own section and shared toggle
- **GIVEN** a loaded pad is selected
- **WHEN** the sidebar is rendered
- **THEN** Pad, separator, Adjust Loop, separator and BPM appear in that order
- **WHEN** the performer activates Adjust Loop twice
- **THEN** the existing editor opens and closes through the shared actions
- **AND** no Unload/Analyze action or playback/timing mutation is dispatched

### Requirement: Unloading Edited Pad Returns To Performance View
The system SHALL close the edited pad's waveform editor and return to the
performance view after an authorized confirmed unload actually unloads that pad.

Opening or cancelling a warning SHALL NOT close the editor. Actual unload of a
different pad SHALL NOT close or retarget another pad's open editor.

#### Scenario: Confirmed unload of edited pad closes editor
- **GIVEN** pad A's editor is open and a current content-bound unload warning targets A
- **WHEN** the performer confirms and the controller actually unloads A
- **THEN** A's editor closes and the performance surface returns

#### Scenario: Warning or unrelated unload preserves editor
- **GIVEN** pad A's editor is open
- **WHEN** an unload warning is cancelled or pad B is actually unloaded
- **THEN** A's editor stays open without retargeting
