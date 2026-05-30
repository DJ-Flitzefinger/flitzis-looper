## ADDED Requirements

### Requirement: Trigger Quantization Controls
The system SHALL expose trigger quantization as a bottom-bar `Q` toggle and move the
quantization grid selection to the Settings page.

The bottom-bar `Q` button SHALL default to disabled for new projects, SHALL render with the
disabled red mode style while disabled, and SHALL render with the enabled green mode style while
enabled. Activating `Q` SHALL toggle only the global trigger-quantization enabled state.

The performance view SHALL NOT render the previous `IMMEDIATE`/`BEAT`/`BAR` segmented trigger
quantization controls. The bottom-bar `Q`, input-learn `L`, Multi Loop button, selected-pad stem
mask buttons, and Settings toggle SHALL share a consistent horizontal alignment and SHALL be
visually grouped by function.

The Settings page SHALL expose the persisted trigger quantization grid as fixed musical steps:
`1/16`, `1/32`, and `1/64`. The default grid step SHALL be `1/32`, and the minimum `1/64` step
SHALL match the loop editor's finest musical grid line spacing when the loop editor is zoomed far
enough to show that grid.

The UI SHALL NOT bypass controller actions, send full beat-grid metadata, touch audio-thread
state directly, perform disk I/O in the audio callback, acquire the Python GIL in the audio
callback, or introduce unbounded audio-thread work.

#### Scenario: New projects default to disabled triggering
- **WHEN** the application starts with a new project
- **THEN** the bottom-bar `Q` button indicates disabled trigger quantization
- **AND** pad triggers preserve immediate behavior unless the performer enables `Q`
- **AND** the Settings page shows `1/32` as the selected quantization grid

#### Scenario: Enabling trigger quantization publishes the selected grid
- **GIVEN** the application is running with trigger quantization disabled
- **AND** the Settings page trigger quantization grid is `1/32`
- **WHEN** the performer activates the bottom-bar `Q` button
- **THEN** the project trigger quantization enabled state becomes `true`
- **AND** the control layer calls `AudioEngine.set_trigger_quantization("1_32")`
- **AND** the bottom-bar `Q` button indicates enabled trigger quantization

#### Scenario: Changing the Settings grid while disabled is persisted
- **GIVEN** trigger quantization is disabled
- **WHEN** the performer changes the Settings page trigger quantization grid to `1/32`
- **THEN** the project stores `trigger_quantization_step = "1_32"`
- **AND** the control layer does not send an audio-thread trigger quantization update until
  trigger quantization is enabled

#### Scenario: Legacy quantization mode is restored as a grid
- **GIVEN** a saved project has legacy trigger quantization mode `next_beat`
- **WHEN** the project is loaded
- **THEN** the project stores trigger quantization as enabled with grid step `1_16`
- **AND** the control layer applies `1_16` to the Rust audio engine
