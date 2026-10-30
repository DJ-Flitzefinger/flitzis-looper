## MODIFIED Requirements

### Requirement: Key Lock Native Status Is Separate From Intent

The system SHALL show Native confirmed/unknown mode and pending preparation separately from intent colors, using matching Native feedback. Error diagnostics SHALL be reported through the command window independently from those normal status indicators. Queue admission SHALL NOT confirm audio. Failure and observed audio SHALL NOT rewrite override origin. Transient status SHALL NOT persist as audio truth or claim musical/device or source-aligned acceptance.

#### Scenario: Failed explicit ON remains green while Native stays OFF

- **GIVEN** selected Pad 3 has confirmed Native OFF
- **WHEN** explicit ON publication or preparation fails
- **THEN** PAD remains green for explicit ON
- **AND** the reason appears in the command window while the UI retains confirmed Native OFF
- **AND** the override is not rewritten to OFF or inheritance

#### Scenario: Inheritance is visible while native preparation is pending

- **GIVEN** selected Pad 3 inherits an enabled global default
- **WHEN** matching native preparation or confirmation is pending
- **THEN** PAD is neutral and adjacent GLOBAL is green
- **AND** pending/preparing and the previous confirmed or unknown Native state are shown separately
- **AND** the UI does not claim the requested mode is already confirmed

### Requirement: Stem Availability Indicators

The system SHALL show per-pad stem availability, generation progress, and normal blocked/readiness state in the performance UI without blocking rendering. Detailed error diagnostics SHALL be reported through the command window and SHALL NOT add error text, tooltips, or styling to the performance UI. The pad grid MAY use compact normal status indicators, while the selected-pad sidebar SHALL provide the detailed normal status for the selected pad. Pad-grid compact indicators SHALL NOT show hover tooltips or hover status messages. UI rendering SHALL use controller/session snapshots and SHALL NOT inspect cache directories, decode audio, run inference, or perform blocking work.

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
- **WHEN** the failure reaches the Python control owner
- **THEN** the command window retains the error diagnostic
- **AND** the selected-pad sidebar retains normal availability/readiness status without error text
- **AND** the pad remains playable using full-mix playback

#### Scenario: Pad-grid indicators do not show hover messages

- **GIVEN** the pad grid shows a compact stem status indicator for a pad
- **WHEN** the performer hovers that pad
- **THEN** the UI does not show a stem status tooltip or hover message over the pad
- **AND** the selected-pad sidebar remains the detailed normal-status surface

## ADDED Requirements

### Requirement: Control diagnostics use the command window

The application SHALL report control/admission failures and asynchronous error diagnostics in the already opened command window through one Python control-thread console sink. Each line SHALL contain a timestamp, operation, reason, and human-readable pad number when applicable. Error details, error tooltips, and error styling SHALL NOT be displayed in performance panels, sidebars, settings, waveform controls, or confirmation dialogs. Normal progress, preparation, confirmed Native state, deliberate confirmation prompts, and available retry controls SHALL remain usable.

#### Scenario: Rejected pad control remains readable

- **WHEN** a pad control is refused with `prepared_mismatch` or `Cold load canceled`
- **THEN** the command window retains a line identifying the operation, pad, and refusal reason
- **AND** no detailed failure text appears in a panel or sidebar
- **AND** the application does not abort its GUI frame or replay the rejected action

#### Scenario: Polling does not flood the console

- **GIVEN** an unchanged asynchronous diagnostic is observed on successive control ticks
- **WHEN** the UI continues rendering
- **THEN** the sink emits only the first observation
- **AND** recovery followed by the same failure emits a new line

#### Scenario: A new failed attempt is reported again

- **GIVEN** an earlier deliberate action was refused
- **WHEN** the performer deliberately repeats that action and receives the same refusal
- **THEN** the command window receives another diagnostic line for the new attempt
- **AND** repeated calls belonging to one held-button gesture do not emit a line per frame

#### Scenario: A later successful event cannot erase a batch failure

- **GIVEN** an input poll receives a rejected pitch event followed by a successful original captured pitch action
- **WHEN** both events are handled in the same drain
- **THEN** the first rejection is printed before the later success clears its current snapshot
- **AND** only the already captured successful action can publish, without recapture or replay

#### Scenario: A cleared stem diagnostic remains available in the console

- **GIVEN** a control-side stem cleanup or backend diagnostic contains a private path
- **WHEN** a later event clears or replaces that diagnostic snapshot
- **THEN** the console retains the redacted operation and pad diagnostic from its original observation
- **AND** no raw diagnostic is rendered in the sidebar and no worker thread prints it

#### Scenario: Historical diagnostics do not become audio authority

- **GIVEN** a pad has a historical failure and its current Native start preconditions are met
- **WHEN** the performer starts it again
- **THEN** the action follows the existing authoritative Native admission path
- **AND** the old diagnostic alone does not disable its start control
- **AND** current source, identity, readiness, and capacity guards remain enforced

#### Scenario: Private data and realtime work remain protected

- **WHEN** an error contains a private source path or another absolute filesystem path
- **THEN** the printed reason redacts that path and does not dump audio or metadata payloads
- **AND** reporting runs only on the Python control thread
- **AND** the audio callback receives no logging, I/O, blocking, Python/GIL, allocation, or new work
