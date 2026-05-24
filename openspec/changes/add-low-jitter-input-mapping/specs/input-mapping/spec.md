## ADDED Requirements

### Requirement: Learn Workflow Preserves Existing UX
The system SHALL preserve the successful Learn workflow for keyboard and MIDI mappings.

The performer SHALL be able to activate `L`, press one keyboard or MIDI input, click a learnable
UI action, and have the mapping saved for later playback. If the performer activates `L`, presses
an input that already has a mapping, and activates `L` again before choosing an action, the system
SHALL delete that input's existing mapping.

#### Scenario: Learn saves a MIDI mapping
- **GIVEN** input mapping is enabled
- **AND** Learn is waiting for an input
- **WHEN** the performer presses MIDI Note On channel 1 note 60 with velocity greater than zero
- **AND** the performer clicks pad 1 trigger
- **THEN** the system saves a mapping from `midi:note:1:60` to `pad.trigger:0`
- **AND** normal playback uses the saved mapping without simulating a mouse click

#### Scenario: Learn deletes an existing input mapping
- **GIVEN** input mapping is enabled
- **AND** `midi:cc:1:7` is mapped to a learnable action
- **WHEN** the performer activates `L`
- **AND** presses MIDI Control Change channel 1 controller 7
- **AND** activates `L` again before selecting an action
- **THEN** the system deletes the `midi:cc:1:7` mapping

### Requirement: Keyboard Mapping Remains Responsiveness Reference
The system SHALL preserve keyboard mapping responsiveness and use it as the behavioral reference
for mapped input dispatch.

Keyboard mappings SHALL include the key plus normalized modifier state. Keyboard mappings SHALL
NOT execute while text input is focused. Activating Learn SHALL clear text input focus, and while
Learn is waiting for keyboard input, captured keyboard input SHALL NOT execute an existing
keyboard mapping.

#### Scenario: Keyboard mapping is suppressed while typing
- **GIVEN** input mapping is enabled
- **AND** `keyboard:A:0000` is mapped to pad 1 trigger
- **AND** a text input is focused
- **WHEN** the performer presses `A`
- **THEN** the system does not trigger pad 1
- **AND** the key press remains available to the focused text input

#### Scenario: Keyboard Learn capture does not execute old mapping
- **GIVEN** input mapping is enabled
- **AND** `keyboard:A:0000` is already mapped to pad 1 trigger
- **AND** Learn is waiting for an input
- **WHEN** the performer presses `A`
- **THEN** the system captures `keyboard:A:0000` as the pending Learn input
- **AND** it does not trigger pad 1

### Requirement: MIDI Hot Path Minimizes Latency And Jitter
The system SHALL capture, timestamp, normalize, and enqueue MIDI input as close to the MIDI
backend callback as practical.

The MIDI callback SHALL stamp incoming events immediately with a monotonic timestamp and SHALL
avoid logging, UI updates, JSON access, Python/GIL access, and unnecessary object work. Normal
mapped MIDI playback SHOULD NOT wait for UI-frame ticks when the mapped action can be dispatched
through the bounded Rust control path.

#### Scenario: MIDI Note On is normalized with timestamp
- **WHEN** the MIDI backend callback receives Note On channel 1 note 60 velocity 100
- **THEN** the Rust input layer records a monotonic received timestamp
- **AND** it normalizes the input as `midi:note:1:60`
- **AND** it enqueues the normalized event without touching JSON or Python UI state

#### Scenario: Mapped MIDI pad trigger bypasses UI-frame dispatch
- **GIVEN** input mapping is enabled
- **AND** `midi:note:1:60` is mapped to pad 1 trigger
- **WHEN** the MIDI backend callback receives Note On channel 1 note 60 velocity 100
- **THEN** the Rust input layer resolves the mapping from in-memory state
- **AND** it forwards the trigger through the existing bounded control-command path
- **AND** it does not simulate a mouse click or wait for UI-frame polling before requesting playback

### Requirement: MIDI Filtering Handles Version One Scope
The system SHALL filter irrelevant MIDI messages before mapping resolution.

Version 1 SHALL process Note On with velocity greater than zero and Control Change. It SHALL treat
Note On velocity zero as Note Off and SHALL NOT trigger Learn or playback from it. It SHALL ignore
Active Sensing, MIDI Clock, SysEx, Program Change, Pitch Bend, Aftertouch, and MPE-style messages.

#### Scenario: Note On velocity zero is ignored
- **WHEN** the MIDI callback receives Note On channel 1 note 60 velocity 0
- **THEN** the system treats it as Note Off
- **AND** it does not capture Learn input
- **AND** it does not trigger mapped playback

#### Scenario: MIDI Clock is ignored
- **WHEN** the MIDI callback receives a MIDI Clock message
- **THEN** the system drops the message for version 1 input mapping
- **AND** it does not enter mapping lookup

### Requirement: Normal Playback Uses In-Memory Mappings
The system SHALL use in-memory mapping snapshots for normal mapped playback.

Python SHALL own mapping-file edits, schema preservation, and publishing snapshots to Rust after
load, save, delete, clear-all, and enable changes. Normal mapped playback SHALL NOT read or write
`config/input/keyboard.json` or `config/input/midi.json`.

#### Scenario: MIDI mapping save refreshes Rust snapshot
- **GIVEN** the performer saves a MIDI mapping through Learn
- **WHEN** the mapping file is written
- **THEN** Python publishes the updated MIDI mapping snapshot to Rust
- **AND** subsequent normal MIDI playback resolves the mapping from memory

#### Scenario: Normal trigger does not read JSON
- **GIVEN** a MIDI mapping snapshot is already loaded in Rust
- **WHEN** the performer triggers the mapped MIDI input
- **THEN** the system resolves the action from the in-memory snapshot
- **AND** no JSON file is read or written in the hot path

### Requirement: Shared Action Semantics Replace Simulated Clicks
The system SHALL route mapped keyboard and MIDI inputs through shared LooperAction command
semantics.

Mapped input dispatch SHALL NOT simulate mouse clicks, replay UI target selection, or depend on
the cursor position. Keyboard and MIDI mappings SHALL converge on the same action identifiers and
controller/audio command behavior after input normalization.

#### Scenario: Keyboard and MIDI trigger the same action
- **GIVEN** `keyboard:A:0000` and `midi:note:1:60` are both mapped to pad 1 trigger
- **WHEN** either input is performed
- **THEN** the system resolves the same `pad.trigger:0` action
- **AND** both inputs request the same pad-trigger command semantics

### Requirement: Python Rust Boundary Is Typed And Testable
The system SHALL expose a clear Python/Rust boundary for mapping snapshots, captured input events,
runtime state, and diagnostics.

Python SHALL be able to turn input mapping on or off, receive captured MIDI input for Learn,
publish updated MIDI mapping snapshots, publish the pad runtime state needed for direct dispatch,
and poll diagnostics/timestamp data without entering the audio callback. The bridge SHALL be
testable without physical MIDI hardware through injected MIDI messages.

#### Scenario: Python receives captured MIDI event for Learn
- **GIVEN** Learn is waiting for input
- **WHEN** Rust reports a normalized MIDI event with binding key and timestamp
- **THEN** Python stores that binding as the pending Learn input
- **AND** the audio callback is not entered for Learn UI state changes

#### Scenario: Hardware-free bridge test injects MIDI
- **WHEN** a test injects Note On channel 1 note 60 velocity 100 through the bridge
- **THEN** Rust normalizes and queues the same event shape used by real MIDI input
- **AND** Python can poll the event without physical MIDI hardware
