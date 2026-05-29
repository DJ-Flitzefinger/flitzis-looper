## ADDED Requirements

### Requirement: Learn capture suppresses direct MIDI dispatch
The system SHALL make Learn capture take precedence over normal mapped dispatch, including direct Rust MIDI dispatch, for the first accepted MIDI input while Learn is waiting.

While Learn is waiting, accepted MIDI input SHALL be reported to Python as capture data and SHALL NOT enqueue trigger, stop, stop-all, loop-region, or other playback commands through the direct dispatch path.

#### Scenario: Mapped MIDI trigger is captured without playback during Learn
- **GIVEN** input mapping is enabled
- **AND** `midi:note:1:60` is mapped to `pad.trigger:0`
- **AND** Learn is waiting for a MIDI input
- **WHEN** the performer presses MIDI Note On channel 1 note 60 with velocity greater than zero
- **THEN** Python receives `midi:note:1:60` as the pending Learn input
- **AND** Rust does not enqueue a direct pad-trigger command for pad 1
- **AND** Python does not execute the mapped action as a fallback

#### Scenario: Learn capture still records continuous MIDI input
- **GIVEN** input mapping is enabled
- **AND** Learn is waiting for a MIDI input
- **WHEN** the performer moves MIDI Control Change channel 1 controller 7
- **THEN** Python receives `midi:cc:1:7` as the pending Learn input with its current value
- **AND** no mapped direct or fallback action runs for that input

### Requirement: Failed direct dispatch is visible and fallback-safe
The system SHALL treat direct Rust MIDI dispatch as successful only when the complete bounded command transaction is enqueued, and SHALL surface failed direct attempts to Python without partial audio commands.

If the direct dispatch path cannot enqueue the full command sequence, the input event SHALL include the mapped action key and a not-dispatched status so Python can apply controller-owned semantics outside the MIDI callback when appropriate.

#### Scenario: Full command queue prevents partial direct trigger
- **GIVEN** `midi:note:1:60` maps to `pad.trigger:0`
- **AND** direct trigger dispatch needs to enqueue a loop-region command followed by a play command
- **AND** the audio command path lacks capacity for both messages
- **WHEN** the performer presses the mapped MIDI input
- **THEN** Rust enqueues no partial loop-region or play command
- **AND** Python receives an input event for `pad.trigger:0` with direct dispatch marked not dispatched
- **AND** any fallback execution happens outside the MIDI callback

#### Scenario: Unloaded direct trigger does not disappear silently
- **GIVEN** `midi:note:1:61` maps to `pad.trigger:1`
- **AND** pad 2 is not loaded in the Rust input runtime state
- **WHEN** the performer presses the mapped MIDI input
- **THEN** Rust does not enqueue a pad-trigger command
- **AND** Python receives an event that includes `pad.trigger:1` and indicates the direct path did not dispatch it
- **AND** controller-owned fallback semantics may ignore the trigger using current project state
