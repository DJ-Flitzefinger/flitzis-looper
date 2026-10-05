## ADDED Requirements

### Requirement: Variable Map Rendering Has An Offline Comparison Diagnostic
The system SHALL provide an isolated offline diagnostic comparing retained Rubber Band render
paths on identical known source-to-musical maps before any live variable-map activation.

The diagnostic SHALL record source/map/native-build identities, rate/output trajectories,
keyframe endpoint handling, audio duration, onset/energy/peak errors, loop seams, resource costs
and every failed predicate. It SHALL distinguish ideal coordinate agreement from acoustic
agreement and SHALL preserve the original causal timing/retention criteria without refitting.

#### Scenario: A warp hits coordinates but deforms the attack
- **WHEN** a candidate matches expected source/beat coordinates but fails an original acoustic gate
- **THEN** the diagnostic reports coordinate success and acoustic failure separately
- **AND** it does not select that result as a validated live synchronization policy

### Requirement: Stem Warp Evidence Separates Timing And Audio Coherence
The system SHALL compare full mix and prepared stems with one parent map and shared source-time
trajectory, while measuring audio-coherence limitations independently from source alignment.

Equal output lengths or channels-together options SHALL NOT count as proof of multistem phase
coherence. Separator residuals, warp differences and source addressing errors SHALL be reported
separately. No stem SHALL receive an independently detected musical grid in this diagnostic.

#### Scenario: Independent stem processors share positions but differ spectrally
- **WHEN** component stems have aligned positions but their processed sum differs from full-mix warp
- **THEN** the report retains both timing and audio-difference measurements
- **AND** it does not claim null-sum equality from the shared map alone

### Requirement: Diagnostic Rendering Does Not Activate Live Behavior
The system SHALL run variable-map rendering diagnostics outside the audio callback without
changing production Quantize, SYNC, source addressing, preparation ownership or scheduler policy.

Offline results SHALL NOT be labeled device-latency, realtime-allocation, deadline or listening
acceptance. Existing failed evidence and production playback SHALL remain intact.

#### Scenario: A diagnostic completes while production code remains unchanged
- **THEN** its report identifies the render assumptions and untested live conditions
- **AND** no callback inference, file I/O, locking, GIL access or experimental construction occurs

### Requirement: Render Comparison Includes Independent Transposition
The system SHALL evaluate independent per-pad semitone intent in the offline variable-map
diagnostic before choosing the production renderer for continuous SYNC.

Canonical source/output coordinates SHALL remain identical under different pitch trajectories
for the same time intent. The diagnostic SHALL measure perceived-pitch evidence separately from
acoustic timing, including nonzero transposition at unit rate, unity native-pitch crossings at
nonunit rate, joint range extremes, prepared-pitch revision changes and multi-pad/stem playback.
Unsupported combinations SHALL be explicit failures rather than silently clipped success.

#### Scenario: A pitch-only change does not move the musical timeline
- **GIVEN** exact maps and identical master/source timing for two renders
- **WHEN** one render receives a different diagnostic semitone trajectory
- **THEN** canonical source positions and scheduled output ranges remain identical
- **AND** pitch accuracy and audible timing are separately reported for the changed render

#### Scenario: A combined pitch requirement exceeds the current compensation range
- **GIVEN** a joint tempo/transposition request not supported by the candidate renderer
- **WHEN** the diagnostic evaluates that request
- **THEN** it records the unsupported combination
- **AND** it does not modify master tempo, source phase or requested pitch to manufacture a pass
