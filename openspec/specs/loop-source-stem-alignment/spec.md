# loop-source-stem-alignment Specification

## Purpose

<!-- Purpose to be updated after merge -->

## Requirements

<!-- From clarify-loop-source-stem-alignment -->
### Requirement: Output Time And Source Position Are Distinct
The system SHALL treat output-frame time and source-frame position as distinct runtime concepts.

Output-frame time SHALL be owned by the Rust transport timeline and scheduler. Source-frame
position SHALL be owned by active Rust mixer voices and SHALL identify the frame read from a pad's
loaded full-mix buffer or aligned prepared-stem buffers.

#### Scenario: Quantized trigger chooses output time without source seeking
- **GIVEN** trigger quantization is enabled
- **AND** a pad has an effective loop region
- **WHEN** the performer triggers the pad
- **THEN** Rust schedules when the pad becomes audible on the output-frame timeline
- **AND** the new voice starts from the effective loop start in source-frame space


<!-- From clarify-loop-source-stem-alignment -->
### Requirement: Loop Regions Resolve To Source-Frame Ranges
The system SHALL resolve published loop regions to bounded half-open source-frame ranges before
audio rendering.

Python MAY persist editable loop intent in seconds, but live Rust playback SHALL use integer
source-frame positions at the mixer sample rate. If a published loop end is missing, invalid, or
past the loaded source, Rust SHALL fall back to the full sample end. If the resolved end is not
after the start, Rust SHALL fall back to a valid full-sample loop for rendering.

#### Scenario: Live loop edit preserves an in-range playhead
- **GIVEN** a pad is actively playing
- **AND** its current source-frame position is inside the newly published loop region
- **WHEN** the loop region reaches the audio thread
- **THEN** subsequent rendering continues from the current source-frame position
- **AND** the voice does not restart from the loop start

#### Scenario: Live loop edit clamps an out-of-range playhead
- **GIVEN** a pad is actively playing
- **AND** its current source-frame position is outside the newly published loop region
- **WHEN** the loop region reaches the audio thread
- **THEN** subsequent rendering clamps the voice to the new loop start


<!-- From clarify-loop-source-stem-alignment -->
### Requirement: Prepared Stems Share The Full-Mix Source Frame Model
The system SHALL render prepared stems through the same source-frame playhead and loop model as
full-mix playback.

Prepared stems SHALL be eligible for audio-thread rendering only when they match the current
loaded full mix by source-version identity, sample rate, channel layout, frame count, and
source-frame origin. Full-mix mode, all-stems mode, and enabled-stem mask updates SHALL NOT
restart active voices or alter their source-frame playheads.

#### Scenario: Stem mask changes keep the current source frame
- **GIVEN** a pad is playing from a current prepared stem set in all-stems mode
- **AND** the voice has advanced inside its loop region
- **WHEN** the enabled-stem mask changes
- **THEN** subsequent rendering reads the newly selected stems at the existing source-frame
  position
- **AND** the voice is not retriggered

#### Scenario: Stale prepared stems do not become live source truth
- **GIVEN** a pad has prepared stems for an old source version
- **WHEN** the pad's loaded full mix changes source version
- **THEN** Rust rejects or ignores the stale prepared stems for live rendering
- **AND** full-mix playback remains available


<!-- From clarify-loop-source-stem-alignment -->
### Requirement: Click-Free Transition Work Is Deferred And Bounded
The system SHALL defer click-free loop and stem transitions to a later bounded Rust-side
transition stage before new DSP/FX are added.

The current Stage-6 clarification SHALL NOT add a new DSP effect, plugin host, neural inference,
disk I/O, logging, blocking wait, heap allocation, or Python/GIL access to the audio callback.
Future transition smoothing SHALL use preallocated Rust-owned state and bounded scalar parameters.

#### Scenario: Stage-6 clarification does not add DSP work
- **GIVEN** the Stage-6 alignment clarification is applied
- **WHEN** the audio callback renders pads
- **THEN** it continues to use the existing source reader, loop wrap, prepared-stem selection,
  playback-rate, Key Lock, gain/EQ, metering, and telemetry path
- **AND** it does not run a new EQ, FX node, stem-generation task, plugin host, disk I/O, logging,
  blocking wait, heap allocation, or Python/GIL operation


<!-- From prepare-click-free-stem-transitions -->
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


<!-- From prepare-click-free-stem-transitions -->
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


<!-- From prepare-click-free-stem-transitions -->
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


