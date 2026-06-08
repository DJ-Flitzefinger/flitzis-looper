## ADDED Requirements

### Requirement: Commands And Parameters Use Separate Bounded Paths
The system SHALL route discrete control commands and continuous parameter updates through separate
bounded control-to-audio paths.

The ordered command path SHALL carry playback triggers, stop commands, sample publication,
transport/mode changes, stem state changes, loop-region state changes, pause/resume, unload, and
other order-sensitive operations. The parameter path SHALL carry fast scalar updates such as
volume, speed, master BPM, per-pad BPM, per-pad gain, per-pad EQ, and future DSP parameters.

#### Scenario: Parameter burst does not fill command queue
- **GIVEN** the continuous parameter path is full of pending parameter updates
- **WHEN** a trigger or stop command is sent through the ordered command path
- **THEN** command acceptance depends on command path capacity
- **AND** the parameter backlog does not occupy command path slots
- **AND** the audio callback remains bounded and nonblocking

### Requirement: Parameter Updates Are Coalesced Before Audio-State Application
The system SHALL coalesce continuous parameter updates by parameter identity before applying them
to audio-thread state during one callback invocation.

When multiple drained parameter messages target the same parameter identity, the callback SHALL
apply the latest drained value for that identity and SHALL NOT repeatedly apply superseded values
from the same callback drain. Future DSP parameters SHALL use this parameter path and SHALL apply
audio-side smoothing before sample processing.

#### Scenario: Repeated EQ updates use latest drained value
- **GIVEN** multiple per-pad EQ parameter messages for the same pad are pending
- **WHEN** one audio callback invocation drains those parameter messages
- **THEN** only the latest drained EQ value for that pad is applied to mixer state
- **AND** no parameter processing blocks, logs, touches disk, acquires the Python GIL, polls MIDI
  ports, loads plugins, or allocates unbounded audio-thread state

### Requirement: Direct Input Multi-Message Dispatch Is Atomic
The system SHALL enqueue direct Rust input-dispatch command sequences all-or-nothing when one
input action requires more than one ordered command message.

If the ordered command path lacks capacity for the complete sequence, the dispatcher SHALL reject
the direct sequence without enqueueing a partial loop-region update, partial trigger, or partial
stop sequence.

#### Scenario: Trigger dispatch rejects partial loop-and-play sequence
- **GIVEN** a mapped direct MIDI trigger needs to send a loop-region update followed by a play
  command
- **AND** the ordered command queue has capacity for only one message
- **WHEN** the dispatcher handles the trigger
- **THEN** it sends no command messages
- **AND** it reports the direct dispatch as not dispatched
- **AND** the audio callback later observes no partial trigger transaction
