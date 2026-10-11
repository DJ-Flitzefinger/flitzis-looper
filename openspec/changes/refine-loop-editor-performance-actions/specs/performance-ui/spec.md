## MODIFIED Requirements

### Requirement: Sidebar Load/Unload Actions For Selected Pad
The system SHALL provide Load Audio for an empty selected pad and Unload Audio
for a loaded selected pad, and SHALL require a warning confirmation before the
performer's Unload Audio action takes effect (E11-16).

Load Audio SHALL retain the existing file dialog and supported audio filters.
The sidebar and existing mapped keyboard/MIDI Unload actions SHALL request the
same transient warning intent. Learn SHALL capture mappings without opening a
warning or executing the action. The warning SHALL identify its captured pad and
source and remain usable independently of current sidebar selection/content.
It SHALL capture action, pad ID, content-instance ID, path and the current native
source assignment identity (generation, digest, frames and rate). Acceptance
SHALL consume the intent once and revalidate the same captured target and its
current eligibility through the authoritative controller; it SHALL NOT retarget
the selected pad. Equal-path/equal-byte reload, unload/rebind, rearrangement,
changed content instance, or lost/stale native source identity SHALL invalidate
acceptance. A mode/window-readiness change on the same assignment SHALL NOT
invalidate the warning.

Cancel, Escape, dismissal, stale or duplicate acceptance SHALL enqueue no work
and leave loaded audio, playback, saved state and resource owners unchanged.
Dismissal SHALL clear only the warning intent and SHALL NOT cancel a pending or
claimed Residency/KEYLOCK transaction. Internal lifecycle cleanup SHALL retain
its authoritative direct unload path without a performer warning.

#### Scenario: Sidebar shows "Load Audio" for an empty selected pad
- **GIVEN** the selected pad has no loaded audio
- **WHEN** the left sidebar is rendered
- **THEN** it contains Load Audio for that pad

#### Scenario: Sidebar shows "Unload Audio" for a loaded selected pad
- **GIVEN** the selected pad has loaded audio
- **WHEN** the left sidebar is rendered
- **THEN** it contains Unload Audio, whose activation requests a warning

#### Scenario: Sidebar "Unload Audio" unloads the selected pad
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

#### Scenario: Equal source bytes do not bypass assignment binding
- **GIVEN** a warning captured pad A's content instance and native source assignment
- **WHEN** A is reloaded from the same path and bytes, rebound, or rearranged before acceptance
- **THEN** the old warning admits no unload for the new assignment
- **AND** loss or change of the captured native source identity also rejects acceptance

#### Scenario: Mapped input and Learn retain the same warning boundary
- **GIVEN** Unload Audio is mapped to keyboard or MIDI for pad A
- **WHEN** the performer activates that mapping outside Learn
- **THEN** the same bound warning appears without unloading A
- **WHEN** Learn captures the mapping gesture
- **THEN** no warning or unload is executed

#### Scenario: Warning cancellation preserves pending mode ownership
- **GIVEN** pad A has a pending or claimed KEYLOCK transaction and an unload warning
- **WHEN** the performer cancels, presses Escape or dismisses the warning
- **THEN** the transaction, readers, requested/effective feedback and playback are preserved
- **AND** a same-assignment mode/window-readiness change alone does not make confirmation stale

#### Scenario: Sidebar "Load Audio" opens a file dialog
- **GIVEN** the selected pad is empty
- **WHEN** the performer activates Load Audio
- **THEN** the existing audio file selection dialog opens for that pad
- **AND** its supported filters include wav, flac, mp3, aif/aiff and ogg

### Requirement: Selected-pad sidebar offers loop editing
The system SHALL place Adjust Loop between the selected-pad Pad and BPM sections
with matching horizontal separators, using the existing authoritative waveform
editor toggle (E11-17).

The action SHALL be available for a loaded selected pad, open its editor when
closed and close the existing open editor through the same close path. It SHALL
be separate from the lower Unload/Analyze action section and SHALL NOT change
playback or timing merely by opening or closing the editor. Existing analysis
activity SHALL NOT hide Adjust Loop when that pad remains eligible for editing.
Sidebar, mapped keyboard/MIDI Adjust and toolbar close SHALL share the same
authoritative editor open/retarget/close behavior, including release of the
closed or retargeted view; they SHALL NOT maintain separate editor flags.

#### Scenario: Adjust Loop action is available for loaded pad
- **GIVEN** a loaded pad is selected
- **WHEN** the sidebar is rendered
- **THEN** Pad, separator, Adjust Loop, separator and BPM appear in that order
- **WHEN** the performer activates Adjust Loop twice
- **THEN** the existing editor opens and closes through the shared actions
- **AND** no Unload/Analyze action or playback/timing mutation is dispatched

#### Scenario: Adjust Loop opens waveform editor
- **GIVEN** a loaded pad is selected and the editor is closed
- **WHEN** the performer activates Adjust Loop
- **THEN** that pad's editor opens in the center surface through the shared authority
- **AND** playback, loop markers and grid timing remain unchanged

#### Scenario: Mapped Adjust releases the same editor view
- **GIVEN** a loaded pad's editor is open, including during eligible analysis activity
- **WHEN** its mapped keyboard/MIDI Adjust action closes the editor
- **THEN** the shared close action releases the editor view and closes the same editor state
- **AND** playback, loop markers and grid timing remain unchanged

### Requirement: Unloading Edited Pad Returns To Performance View
The system SHALL close the edited pad's waveform editor and return to the
performance view after an authorized confirmed unload actually unloads that pad.

Opening or cancelling a warning SHALL NOT close the editor. Actual unload of a
different pad SHALL NOT close or retarget another pad's open editor.
Native unload admission SHALL precede Python cleanup and editor closure; failed
admission SHALL preserve the edited pad's editor and pending/claimed mode ownership.

#### Scenario: Sidebar unload returns to pad view
- **GIVEN** pad A's editor is open and a current content-bound unload warning targets A
- **WHEN** the performer confirms and the controller actually unloads A
- **THEN** A's editor closes and the performance surface returns

#### Scenario: Unloading another pad preserves editor
- **GIVEN** pad A's editor is open
- **WHEN** an unload warning is cancelled or pad B is actually unloaded
- **THEN** A's editor stays open without retargeting

#### Scenario: Failed native unload preserves the edited pad
- **GIVEN** A's editor is open and confirmed unload targets its current content
- **AND** A has a pending or claimed KEYLOCK transaction
- **WHEN** the native unload cannot be admitted
- **THEN** Python cleanup and editor closure do not occur
- **AND** the transaction, readers, feedback and playback remain intact
