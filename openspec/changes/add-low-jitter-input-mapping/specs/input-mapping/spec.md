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

Version 1 SHALL process Note On with velocity greater than zero, Control Change, and NRPN
increment/decrement messages carried by Control Change setup and data-increment/data-decrement
events. It SHALL treat Note On velocity zero as Note Off and SHALL NOT trigger Learn or playback
from it. It SHALL ignore NRPN setup Control Changes as standalone Learn/playback targets. It SHALL
ignore Active Sensing, MIDI Clock, SysEx, Program Change, Pitch Bend, Aftertouch, and MPE-style
messages.

#### Scenario: Note On velocity zero is ignored
- **WHEN** the MIDI callback receives Note On channel 1 note 60 velocity 0
- **THEN** the system treats it as Note Off
- **AND** it does not capture Learn input
- **AND** it does not trigger mapped playback

#### Scenario: MIDI Clock is ignored
- **WHEN** the MIDI callback receives a MIDI Clock message
- **THEN** the system drops the message for version 1 input mapping
- **AND** it does not enter mapping lookup

#### Scenario: NRPN increment/decrement is normalized as one binding
- **WHEN** the MIDI callback receives NRPN MSB and LSB Control Changes on channel 1 for parameter 0
- **AND** it then receives a Data Increment Control Change on channel 1
- **THEN** the system normalizes the input as `midi:nrpn:1:0`
- **AND** it reports an increment value for relative controller-owned dispatch
- **WHEN** it then receives a Data Decrement Control Change on channel 1
- **THEN** the system normalizes the same input as `midi:nrpn:1:0`
- **AND** it reports a decrement value for relative controller-owned dispatch

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

### Requirement: Learnable Control Coverage
The system SHALL allow Learn to save mappings for Tap BPM, bottom-bar stem mask buttons,
per-pad Gain, per-pad EQ band controls, Master Volume, and the global Speed/Pitch control.

Keyboard and MIDI Note mappings to continuous controls SHALL save bounded set-value LooperAction
keys from the value selected while learning. MIDI Control Change and NRPN increment/decrement
mappings to Master Volume, global Speed/Pitch, per-pad Gain, and per-pad EQ SHALL save
relative-step LooperAction keys and execute one bounded controller-owned increment or decrement
outside the audio callback for each detected encoder movement. Relative movement SHALL be
independent of the CC or NRPN start position and SHALL keep working across 0..127 wraparound or
repeated relative encoder values, including common single-step encodings such as `1`/`127` and
`65`/`63`. Target controls SHALL clamp at their existing minimum and maximum values when the MIDI
control keeps turning. The
first MIDI continuous-control event after Learn capture or startup SHALL establish the baseline
without changing the target parameter. Stem mask buttons SHALL remain selectable as Learn targets after an input is
pending, even when normal stem playback availability disables their execution.

#### Scenario: Learn saves Tap BPM
- **GIVEN** input mapping is enabled
- **AND** Learn has captured one MIDI or keyboard input
- **WHEN** the performer activates Tap BPM for selected pad 5
- **THEN** the system saves a mapping to `pad.tap_bpm:4`
- **AND** normal mapped dispatch registers a Tap BPM event for that pad

#### Scenario: Keyboard Learn saves bounded set-value controls
- **GIVEN** input mapping is enabled
- **AND** Learn has captured one keyboard input
- **WHEN** the performer targets Master Volume at 37 percent
- **THEN** the system saves a mapping to `global.volume:37`
- **WHEN** the performer targets the selected pad's Mid EQ at -3.5 dB
- **THEN** the system saves a mapping to a bounded per-pad EQ action
- **WHEN** the performer targets the selected pad's Gain at 37 percent
- **THEN** the system saves a mapping to `pad.gain:<pad>:37`
- **WHEN** the performer targets global Speed/Pitch at 123 percent
- **THEN** the system saves a mapping to `global.speed:123`
- **AND** mapped dispatch updates the controller state outside the audio callback

#### Scenario: MIDI CC Learn saves relative continuous controls
- **GIVEN** input mapping is enabled
- **AND** Learn has captured MIDI Control Change channel 1 controller 7 with value 64
- **WHEN** the performer targets Master Volume
- **THEN** the system saves a mapping to `global.volume.delta`
- **WHEN** the performer targets the selected pad's Gain
- **THEN** the system saves a mapping to `pad.gain.delta:<pad>`
- **WHEN** the performer targets global Speed/Pitch
- **THEN** the system saves a mapping to `global.speed.delta`
- **WHEN** that CC value later changes from 64 to 65
- **THEN** mapped dispatch increases Master Volume by one fixed step outside the audio callback
- **WHEN** that CC value repeats at 65
- **THEN** mapped dispatch increases Master Volume by another fixed step outside the audio callback
- **WHEN** that CC value later changes from 65 to 63
- **THEN** mapped dispatch decreases Master Volume by one fixed step outside the audio callback
- **WHEN** that CC value repeats at 63
- **THEN** mapped dispatch decreases Master Volume by another fixed step outside the audio callback

#### Scenario: NRPN Learn saves relative continuous controls
- **GIVEN** input mapping is enabled
- **AND** Learn has captured NRPN parameter 0 on channel 1 with an increment value
- **WHEN** the performer targets Master Volume
- **THEN** the system saves a mapping to `global.volume.delta` for `midi:nrpn:1:0`
- **WHEN** that NRPN input later reports another increment value
- **THEN** mapped dispatch increases Master Volume by one fixed step outside the audio callback
- **WHEN** that NRPN input later reports a decrement value
- **THEN** mapped dispatch decreases Master Volume by one fixed step outside the audio callback

#### Scenario: Endless MIDI encoder covers the full bounded target range
- **GIVEN** input mapping is enabled
- **AND** `midi:cc:1:7` is mapped to `global.volume.delta`
- **WHEN** the performer keeps turning an endless encoder clockwise across the CC 127 to 0 wrap
- **THEN** mapped dispatch continues increasing Master Volume by fixed steps
- **WHEN** Master Volume reaches 100 percent and the performer keeps turning clockwise
- **THEN** Master Volume stays at 100 percent
- **WHEN** the performer keeps turning the encoder counter-clockwise
- **THEN** mapped dispatch decreases Master Volume by fixed steps down to 0 percent

#### Scenario: MIDI Note Learn saves bounded set-value controls
- **GIVEN** input mapping is enabled
- **AND** Learn has captured MIDI Note On channel 1 note 60
- **WHEN** the performer targets Master Volume at 37 percent
- **THEN** the system saves a mapping to `global.volume:37`
- **AND** mapped dispatch updates the controller state outside the audio callback

#### Scenario: Learn saves disabled stem buttons
- **GIVEN** input mapping is enabled
- **AND** Learn has captured one MIDI or keyboard input
- **AND** the selected pad does not currently allow normal stem-mask execution
- **WHEN** the performer activates the `A` stem button as the Learn target
- **THEN** the system saves the `A` button's all-stems preset action
- **AND** it does not execute stem generation, cache scanning, disk I/O, or audio-callback work

### Requirement: Python Rust Boundary Is Typed And Testable
The system SHALL expose a clear Python/Rust boundary for mapping snapshots, captured input events,
runtime state, and diagnostics.

Python SHALL be able to turn input mapping on or off, receive captured MIDI input for Learn,
publish updated MIDI mapping snapshots, publish the pad runtime state needed for direct dispatch,
and poll diagnostics/timestamp data without entering the audio callback. The bridge SHALL be
testable without physical MIDI hardware through injected MIDI messages.

#### Scenario: Python receives captured MIDI event for Learn
- **GIVEN** Learn is waiting for input
- **WHEN** Rust reports a normalized MIDI event with binding key, MIDI value, and timestamp
- **THEN** Python stores that binding as the pending Learn input
- **AND** the audio callback is not entered for Learn UI state changes

#### Scenario: Hardware-free bridge test injects MIDI
- **WHEN** a test injects Note On channel 1 note 60 velocity 100 through the bridge
- **THEN** Rust normalizes and queues the same event shape used by real MIDI input
- **AND** Python can poll the event without physical MIDI hardware
