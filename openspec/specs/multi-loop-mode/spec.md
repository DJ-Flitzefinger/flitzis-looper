# multi-loop-mode Specification

## Purpose
To define the global MultiLoop mode that controls whether pads can loop concurrently (polyphonic looping) or enforce one-at-a-time playback, with immediate onset on trigger.

## Requirements
### Requirement: MultiLoop Mode State
The system SHALL provide a global MultiLoop mode setting that can be enabled or disabled at runtime.

#### Scenario: MultiLoop defaults to disabled
- **WHEN** the application is started
- **THEN** MultiLoop mode is disabled

#### Scenario: MultiLoop can be enabled and disabled
- **WHEN** the performer enables MultiLoop mode
- **THEN** MultiLoop mode becomes enabled
- **WHEN** the performer disables MultiLoop mode
- **THEN** MultiLoop mode becomes disabled

### Requirement: MultiLoop Enabled Allows Concurrent Loops
When MultiLoop mode is enabled, the system SHALL allow multiple pads to be active simultaneously. Triggering a pad SHALL start or restart that pad’s loop without stopping other active pads.

#### Scenario: Two pads play concurrently
- **WHEN** MultiLoop mode is enabled
- **AND** the performer triggers pad 1
- **AND** the performer triggers pad 2
- **THEN** pad 1 is active
- **AND** pad 2 is active
- **AND** both pads contribute to the mixed audio output (see `play-samples`)

#### Scenario: Retrigger affects only the selected pad
- **WHEN** MultiLoop mode is enabled
- **AND** pad 1 is active
- **AND** pad 2 is active
- **AND** the performer retriggers pad 1
- **THEN** pad 2 remains active

### Requirement: MultiLoop Disabled Enforces One-at-a-time
When MultiLoop mode is disabled, triggering a pad SHALL stop all other active pads before starting the triggered pad.

#### Scenario: Triggering a new pad stops the previously active pad
- **WHEN** MultiLoop mode is disabled
- **AND** pad 1 is active
- **AND** the performer triggers pad 2
- **THEN** pad 1 becomes inactive promptly
- **AND** pad 2 becomes active

### Requirement: Loop Onset Is Determined by Trigger Time
The system SHALL start loops immediately when a pad trigger action occurs. Loop onset SHALL be determined by the user’s trigger timing (no quantization to a global grid).

#### Scenario: Onsets are independent
- **WHEN** MultiLoop mode is enabled
- **AND** pad 1 is triggered at time T1
- **AND** pad 2 is triggered at time T2
- **THEN** pad 1 starts playback at time T1
- **AND** pad 2 starts playback at time T2

### Requirement: Triggering an Unloaded Pad Has No Effect
Triggering a pad with no loaded audio SHALL NOT stop other pads and SHALL NOT change active-pad state.

#### Scenario: Empty pad trigger is ignored
- **WHEN** MultiLoop mode is disabled
- **AND** pad 1 is active
- **AND** the performer triggers an unloaded pad
- **THEN** pad 1 remains active


<!-- Added from add-phase-aware-playback-sync -->
### Requirement: Quantized Single-Loop Transitions Remain Atomic And Loop-Start Based
The system SHALL keep quantized single-loop transitions atomic when MultiLoop mode is
disabled and trigger quantization is enabled.

When a loaded pad is triggered, the system SHALL keep the stop-all operation and the
pad start as one atomic scheduled transition at one absolute output frame.

The target pad SHALL start from its effective loop start at that transition frame. Rust SHALL
NOT use phase-aware source-frame offsets or late-click catch-up to start the target pad from the
middle or end of its loop.

If the scheduler cannot accept the transition, currently playing pads SHALL remain
unchanged. If the target pad cannot play, currently playing pads SHALL remain unchanged.

#### Scenario: Quantized switch happens at one frame and loop start
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled with grid step `1/16`
- **AND** pad 1 is active
- **AND** loaded pad 2 has valid BPM and phase-anchor metadata
- **WHEN** the performer triggers pad 2
- **THEN** Rust schedules one transition that stops pad 1 and starts pad 2 at the same output frame
- **AND** pad 2 starts at its effective loop-start source frame

#### Scenario: Rejected quantized switch does not stop the active pad
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled
- **AND** pad 1 is active
- **AND** the scheduler is full
- **WHEN** the performer triggers loaded pad 2
- **THEN** the transition is rejected
- **AND** pad 1 remains active


<!-- Added from add-rust-transport-timeline -->
### Requirement: Quantized Single-Loop Transitions Are Atomic
The system SHALL keep quantized single-loop transitions atomic when MultiLoop mode is
disabled and trigger quantization is enabled.

Triggering a loaded pad SHALL schedule the stop-other-pads operation and the requested pad
start as one atomic transition at the same absolute output frame.

If the scheduler cannot accept that transition, the system SHALL leave currently playing
pads unchanged.

#### Scenario: Quantized one-at-a-time switch happens at one frame
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled with grid step `1/16`
- **AND** pad 1 is active
- **AND** the performer triggers loaded pad 2
- **THEN** Rust schedules pad 1 to stop and pad 2 to start at the same selected-grid output frame

#### Scenario: Rejected quantized switch does not stop the active pad
- **WHEN** MultiLoop mode is disabled
- **AND** trigger quantization is enabled
- **AND** pad 1 is active
- **AND** the scheduler is full
- **AND** the performer triggers loaded pad 2
- **THEN** the transition is rejected
- **AND** pad 1 remains active


<!-- Added from repair-multi-loop-bpm-sync -->
### Requirement: BPM-Locked MultiLoop Pads Remain Phase-Stable Across Loop Wraps
The system SHALL keep BPM-locked MultiLoop pads that represent the same musical loop length phase-stable against the Rust master output timeline across repeated loop-region wraps.

When Multi Loop is enabled and BPM Lock has a valid master BPM plus valid per-pad BPM metadata, pads with different source BPMs but the same musical loop length SHALL complete each musical loop cycle at the same output-frame boundary within a bounded frame tolerance.

This phase stability SHALL hold for global Pitch/Speed values including `1.0x`, `1.25x`, `1.5x`, and `2.0x`, with Key Lock disabled or enabled, and with fixed or variable callback segment sizes.

Normal loop wrapping SHALL NOT accumulate independent per-pad phase error. Manual START/STOP and explicit retrigger SHALL remain phase-reset operations that restart the affected pad from its effective source loop start.

#### Scenario: Different-BPM pads share one master loop cycle
- **GIVEN** Multi Loop is enabled
- **AND** BPM Lock is enabled with a valid master BPM
- **AND** pad 1 and pad 2 have valid BPM metadata
- **AND** both pads have loop regions representing the same four-bar musical length at their own BPMs
- **WHEN** both pads are started together
- **THEN** both pads complete each four-bar cycle at the same output-frame boundary
- **AND** neither pad accumulates local wrap drift relative to the other

#### Scenario: Drift does not grow after repeated wraps at 1.5x
- **GIVEN** BPM-locked Multi Loop playback is running at global Pitch/Speed `1.5x`
- **AND** the callback renders fixed and variable segment sizes over at least ten loop repeats
- **WHEN** the pads continue through normal loop-region wrapping
- **THEN** their musical phase error remains bounded
- **AND** the phase error does not grow on each loop repeat

#### Scenario: Retrigger remains a phase reset
- **GIVEN** two BPM-locked Multi Loop pads are active
- **AND** either pad has accumulated any prior playback phase offset
- **WHEN** the performer presses START/STOP or explicitly retriggers the pad
- **THEN** the retriggered pad restarts from its effective source loop start
- **AND** the retrigger acts as a phase reset for that pad


<!-- Added from repair-multi-loop-bpm-sync -->
### Requirement: Prepared Stems Share BPM-Locked MultiLoop Timing
The system SHALL route prepared-stem playback through the same BPM-locked source timing and loop-wrap path as full-mix playback.

Switching a pad from full-mix playback to prepared stems, or changing the enabled prepared-stem mask, SHALL NOT create a second loop clock, reset the BPM-locked phase, or bypass the repaired Multi Loop timing path.

#### Scenario: Prepared stems remain phase-stable with full mix
- **GIVEN** Multi Loop and BPM Lock are enabled
- **AND** pad 1 plays the full mix
- **AND** pad 2 plays validated prepared stems
- **AND** both pads have valid BPM metadata and matching musical loop lengths
- **WHEN** playback continues across repeated loop wraps
- **THEN** the prepared-stem pad remains phase-stable with the full-mix pad


<!-- Added from repair-multi-loop-bpm-sync -->
### Requirement: Missing BPM Metadata Does Not Claim Phase Lock
The system SHALL keep pads without valid BPM metadata on the documented global-speed fallback and SHALL NOT claim BPM-locked phase stability for those pads.

Pads that lack valid BPM metadata MAY continue playing concurrently in Multi Loop mode, but they SHALL NOT redefine the master output timeline, force synced pads to their local phase, or corrupt the phase-stable path used by pads with valid metadata.

#### Scenario: Missing BPM falls back without disturbing synced pads
- **GIVEN** Multi Loop and BPM Lock are enabled
- **AND** pad 1 and pad 2 have valid BPM metadata and are phase-stable
- **AND** pad 3 lacks valid BPM metadata
- **WHEN** pad 3 is started
- **THEN** pad 3 follows the documented global-speed fallback
- **AND** pad 1 and pad 2 remain phase-stable against the Rust master output timeline

