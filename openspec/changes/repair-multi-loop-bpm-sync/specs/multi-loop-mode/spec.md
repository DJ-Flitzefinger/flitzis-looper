## ADDED Requirements

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
