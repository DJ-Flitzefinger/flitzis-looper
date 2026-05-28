## ADDED Requirements

### Requirement: Stem Source Changes Use Bounded Transition State
The system SHALL apply accepted stem mode and enabled-mask changes for active pads through bounded
Rust-owned transition state.

The transition state SHALL store only fixed-size scalar source-selection and ramp progress data.
It SHALL NOT own audio payloads, file paths, Python objects, plugin handles, dynamically sized DSP
chains, or unbounded callback work.

#### Scenario: Active full-mix to all-stems change crossfades
- **GIVEN** a pad is actively playing from its loaded full mix
- **AND** a matching prepared stem set has already been accepted
- **WHEN** the performer selects all-stems mode for that pad
- **THEN** Rust renders a short bounded transition from the full-mix source to the selected stem
  source
- **AND** the transition uses only already accepted buffers and fixed-size Rust state

#### Scenario: Active stem mask change crossfades
- **GIVEN** a pad is actively playing from a matching prepared stem set in all-stems mode
- **WHEN** the enabled component-stem mask changes
- **THEN** Rust renders a short bounded transition from the previous stem mask to the new stem
  mask
- **AND** the pad is not stopped, retriggered, or time-slipped by the mask change itself

### Requirement: Stem Transitions Preserve Source-Frame Continuity
The system SHALL preserve active voice source-frame continuity and loop wrapping while a stem
transition is active.

Stem transitions SHALL read the previous and current source selections at the same source frame
before existing playback-rate, Key Lock, gain/EQ, metering, and playhead reporting stages. The
transition SHALL NOT change output-frame scheduling, transport phase, loop start/end policy, voice
playhead position, BPM-lock timing, or Key Lock mode.

#### Scenario: Transition keeps loop-relative playback position
- **GIVEN** a pad is playing inside a configured loop region
- **AND** a stem source transition is active
- **WHEN** the mixer renders the next callback block
- **THEN** both transition sides read from the same loop-relative source-frame positions
- **AND** the voice playhead advances as it would without the transition

### Requirement: Stem Transition Preparation Has Narrow Non-Goals
The system SHALL keep click-free stem transition preparation separate from DSP/FX foundation and
loop-edit transition policy.

This stage SHALL NOT implement a new EQ, visible effect, DSP-chain foundation, plugin host,
real-time stem separation, live loop-edit crossfade, neural inference, disk I/O, logging, blocking
wait, heap allocation, or Python/GIL access in the audio callback.

#### Scenario: Stem transition stage does not add DSP work
- **GIVEN** the click-free stem transition preparation is applied
- **WHEN** the audio callback renders pads
- **THEN** it continues to use the existing source reader, loop wrap, playback-rate, Key Lock,
  gain/EQ, metering, and telemetry path
- **AND** it does not run a new EQ, FX node, stem-generation task, plugin host, disk I/O, logging,
  blocking wait, heap allocation, or Python/GIL operation
