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

### Requirement: Stem Mix Controls
The system SHALL provide selected-pad controls for choosing full-mix playback or all prepared
stems when a current prepared stem set is available.

New projects SHALL default to full-mix playback. Selecting all-stems mode SHALL request
prepared-stem playback for the selected pad when valid stems are available and SHALL fall back
to full mix when stems are unavailable, stale, incomplete, rejected, or disabled.

#### Scenario: New projects default to full mix
- **WHEN** the application starts with a new project
- **THEN** the selected-pad stem mix control defaults each pad to full-mix playback
- **AND** prepared stems do not affect playback until the performer chooses stem playback

#### Scenario: All-stems mode requires current prepared stems
- **GIVEN** the selected pad has a complete current prepared stem set
- **WHEN** the performer selects all-stems mode
- **THEN** the project stem mix preference for that pad becomes all-stems
- **AND** the control layer sends a bounded stem mix update to the Rust audio engine

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
