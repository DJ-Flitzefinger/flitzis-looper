## ADDED Requirements

### Requirement: Stem Availability Indicators
The system SHALL show per-pad stem availability, generation progress, blocked state, and
errors in the performance UI without blocking rendering.

The pad grid MAY use compact indicators, while the selected-pad sidebar SHALL provide the
detailed status for the selected pad. UI rendering SHALL use controller/session snapshots and
SHALL NOT inspect cache directories, decode audio, run inference, or perform blocking work.

#### Scenario: Selected pad shows available stems
- **GIVEN** the selected pad has loaded audio
- **AND** the pad has a complete current prepared stem cache
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar indicates that stems are available
- **AND** the pad grid may show a compact stem-available indicator for that pad

#### Scenario: Stem generation progress is visible
- **GIVEN** stem generation is running for the selected pad
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar shows the current generation stage and progress when available
- **AND** the UI remains responsive while generation continues outside the audio callback

#### Scenario: Stem generation error preserves pad usability
- **GIVEN** stem generation failed for the selected pad
- **WHEN** the performance UI is rendered
- **THEN** the selected-pad sidebar shows the error outside the audio callback
- **AND** the pad remains playable using full-mix playback

### Requirement: Selected-Pad Stem Generation Action
The system SHALL provide a selected-pad action for requesting offline stem generation through
the controller layer.

The action SHALL be available only for a loaded pad and SHALL follow the controller's
inactive-pad and per-pad background-task gating. The UI SHALL NOT call Rust background
generation directly, bypass the controller, or run stem work in the render loop.

#### Scenario: Generate action schedules a stopped loaded pad
- **GIVEN** the selected pad has loaded audio
- **AND** the selected pad is not playing, loading, analyzing, or already generating stems
- **WHEN** the performer activates Generate Stems
- **THEN** the UI emits a controller action for that selected pad
- **AND** stem generation is scheduled as offline/background work

#### Scenario: Generate action is blocked for an active pad
- **GIVEN** the selected pad is currently playing
- **WHEN** the performer tries to generate stems for that pad
- **THEN** the request is rejected or disabled through controller state
- **AND** no stem generation work runs in the audio callback

### Requirement: Selected-Pad Stem Deletion Action
The system SHALL provide a selected-pad Delete Stems action next to the Generate Stems action.

The Delete Stems action SHALL route through the controller layer, SHALL remove only the selected
pad's tracked project-local stem cache artifacts, SHALL clear the selected pad's stem cache
metadata, and SHALL leave the loaded full-mix audio available. UI rendering SHALL decide whether
to enable the action from controller/session snapshots and SHALL NOT inspect cache directories.

#### Scenario: Delete action removes selected pad stems
- **GIVEN** the selected pad has tracked cached stems
- **WHEN** the performer activates Delete Stems
- **THEN** the UI emits a controller action for that selected pad
- **AND** the selected pad returns to full-mix playback with no available stems

#### Scenario: Delete action is disabled without tracked stems
- **GIVEN** the selected pad has no tracked stem cache metadata
- **WHEN** the performance UI is rendered
- **THEN** the Delete Stems action is disabled
- **AND** rendering does not inspect cache directories, read files, decode audio, or run inference

### Requirement: Stem Mix Controls
The system SHALL provide selected-pad controls for choosing full-mix playback or all prepared
stems when a current prepared stem set is available.

New projects SHALL default to full-mix playback. The full-mix/all-stems selection buttons SHALL
be disabled when the selected pad has no current prepared stem set. Selecting all-stems mode
SHALL request prepared-stem playback for the selected pad only when valid stems are available and
SHALL fall back to full mix when stems are unavailable, stale, incomplete, rejected, deleted, or
disabled.

#### Scenario: New projects default to full mix
- **WHEN** the application starts with a new project
- **THEN** the selected-pad stem mix control defaults each pad to full-mix playback
- **AND** prepared stems do not affect playback until the performer chooses stem playback

#### Scenario: All-stems mode requires current prepared stems
- **GIVEN** the selected pad has a complete current prepared stem set
- **WHEN** the performer selects all-stems mode
- **THEN** the project stem mix preference for that pad becomes all-stems
- **AND** the control layer sends a bounded stem mix update to the Rust audio engine

#### Scenario: Mix mode buttons are disabled without stems
- **GIVEN** the selected pad has no current prepared stem set
- **WHEN** the selected-pad sidebar is rendered
- **THEN** the full-mix and all-stems buttons are disabled
- **AND** the UI does not persist an all-stems preference for that pad

#### Scenario: Revert to full mix
- **GIVEN** the selected pad is configured for all-stems mode
- **WHEN** the performer selects full-mix mode
- **THEN** the project stem mix preference for that pad becomes full-mix
- **AND** the pad uses the loaded full-mix buffer without requiring stem cache deletion

### Requirement: Bottom-Bar Per-Stem Mask Controls
The system SHALL render bottom-bar selected-pad stem mask controls as six compact buttons ordered
`V`, `D`, `M`, `B`, `I`, and `A`.

The buttons SHALL target the currently selected red-outlined pad. `V`, `D`, `M`, and `B` SHALL be
freely combinable component toggles. `I` SHALL select the instrumental preset Drums + Melody + Bass
and mute Vocals. `A` SHALL select the all-stems preset Vocals + Drums + Melody + Bass. `I` SHALL
NOT mean playing only `instrumental.wav`, and `A` SHALL NOT add `instrumental.wav` as a fifth
audible layer.
The system SHALL treat `I` and `A` as explicit preset display states rather than inferred aliases
for matching custom masks. Activating `V`, `D`, `M`, or `B` from either preset SHALL leave preset
display mode, enter custom display mode, and set the custom mask to only the clicked component stem.
The system SHALL treat `I` and `A` as one exclusive preset group and `V`, `D`, `M`, and `B` as a
separate component group. Activating a preset SHALL remember the last component-group custom mask,
switching between `I` and `A` SHALL preserve that remembered component mask, and clicking the
currently active preset again SHALL deactivate the preset group and restore the remembered
component mask.
Right-clicking `V`, `D`, `M`, or `B` SHALL set a non-momentary custom solo state for that component
stem without adding a separate mute feature.

#### Scenario: Selected pad controls are disabled without prepared stem playback
- **GIVEN** the selected pad is in full-mix mode
- **WHEN** the performance bottom bar is rendered
- **THEN** the `V`, `D`, `M`, `B`, `I`, and `A` buttons are disabled
- **AND** rendering does not inspect cache directories, compute source versions, read files, decode audio, run inference, or call low-level Rust background task APIs

#### Scenario: Component toggles update the selected pad
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **WHEN** the performer toggles `V`, `D`, `M`, or `B`
- **THEN** the selected pad's bounded enabled-stem mask is updated through controller actions
- **AND** currently playing voices are not stopped, retriggered, time-slipped, or moved to a different loop position by the toggle

#### Scenario: Preset buttons display exclusive state
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **WHEN** the performer selects `I`
- **THEN** only the `I` button appears active
- **AND** the underlying enabled-stem mask enables Drums, Melody, and Bass while Vocals remains disabled

#### Scenario: Component click leaves all-stems preset for custom mode
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **AND** the `A` preset display state is active
- **WHEN** the performer clicks `M`
- **THEN** the selected pad enters custom display mode
- **AND** only the Melody component appears active
- **AND** the `A` preset appears inactive

#### Scenario: Component click leaves instrumental preset for custom mode
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **AND** the `I` preset display state is active
- **WHEN** the performer clicks `V`
- **THEN** the selected pad enters custom display mode
- **AND** only the Vocals component appears active
- **AND** the `I` preset appears inactive

#### Scenario: Component right-click sets non-momentary solo
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **WHEN** the performer right-clicks `D`
- **THEN** the selected pad enters custom display mode
- **AND** only the Drums component appears active
- **AND** the state persists until the performer changes the component or preset buttons again

#### Scenario: Custom masks do not auto-select matching presets
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **WHEN** the custom component mask matches Drums + Melody + Bass or Vocals + Drums + Melody + Bass
- **THEN** the component buttons reflect the custom mask
- **AND** the `I` and `A` preset buttons remain inactive until explicitly clicked

#### Scenario: Preset deactivation restores remembered components
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **AND** the custom component mask enables Vocals and Bass
- **WHEN** the performer clicks `I`
- **AND** the performer clicks `I` again
- **THEN** the selected pad returns to custom display mode
- **AND** only Vocals and Bass appear active

#### Scenario: Preset switching preserves remembered components
- **GIVEN** the selected pad has a current prepared stem set
- **AND** the selected pad is in all-stems mode
- **AND** the custom component mask enables Drums and Melody
- **WHEN** the performer clicks `I`
- **AND** the performer switches between `I` and `A` one or more times
- **AND** the performer clicks the currently active preset again
- **THEN** the selected pad returns to custom display mode
- **AND** only Drums and Melody appear active

### Requirement: Settings Overlay
The system SHALL provide a bottom-right Settings toggle that replaces the main Looper display
area with a Settings page while open.

The closed state SHALL show a gear icon at the right edge of the center bottom bar, aligned with
the right edge of the bank-button row. The open state SHALL show an `X` close icon in the same
bottom-right location and SHALL return to the normal Looper display when activated. The first
Settings page controls SHALL configure bounded Demucs stem-generation quality values: shifts from
1 through 20, default 10, and overlap from 0.25 through 0.95, default 0.5. Rendering the Settings
page SHALL use project/session state and controller actions only; it
SHALL NOT inspect cache directories, compute source versions, read files, decode audio, invoke
Demucs, download models, or call low-level Rust background-task APIs from the render loop.

#### Scenario: Gear opens Settings
- **GIVEN** the normal Looper display is visible
- **WHEN** the performer activates the bottom-right gear icon
- **THEN** the Settings page replaces the main Looper display area
- **AND** the bottom-right toggle changes to an `X` close icon
- **AND** the toggle remains right-aligned with the bank-button row

#### Scenario: Close returns to Looper
- **GIVEN** the Settings page is open
- **WHEN** the performer activates the bottom-right `X` close icon
- **THEN** the normal Looper display area is rendered again
- **AND** the Settings page no longer covers the main Looper display area
- **AND** the toggle remains right-aligned with the bank-button row

#### Scenario: Stem quality controls update project settings
- **GIVEN** the Settings page is open
- **WHEN** the performer sets Demucs shifts to 4
- **AND** the performer sets Demucs overlap to 0.25
- **THEN** those bounded quality values are stored in project state
- **AND** the next Generate Stems request uses those values in its backend request

#### Scenario: Settings render loop stays non-blocking
- **GIVEN** the Settings page is open
- **WHEN** the UI renders a frame
- **THEN** rendering does not inspect cache directories, compute source versions, read files, decode audio, invoke Demucs, download models, or call low-level Rust background-task APIs
