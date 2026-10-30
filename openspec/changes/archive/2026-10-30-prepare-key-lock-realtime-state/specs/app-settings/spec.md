## ADDED Requirements

### Requirement: Global Key Lock Start Behavior Changes Only When Fully Stopped
The system SHALL provide one coherent persisted app-wide Settings choice labelled
"Key Lock start behavior" with understandable policies "First sound at grid"
and "Attack at grid". These SHALL control genuinely different start contracts
in the existing single authoritative audio path: no audible output before the
selected grid target, or the perceived principal musical attack aligned to that
target with only causally available necessary quiet lead permitted, respectively.

The system SHALL permit a change only while audio playback is completely stopped.
The authoritative admission condition SHALL safely include still-held or paused
resumable voices and their retained histories, and admitted pending playback
that could resume or start using the current policy. UI appearance alone or zero
instantaneous output SHALL NOT establish the stopped condition. An intervening
playback admission SHALL NOT race a policy change into active retained history.

While the stopped condition is false the choice SHALL be disabled with the hint
"Stop playback to change". A refused change SHALL leave the effective and
persisted policy unchanged and SHALL NOT retain a queued or next-start choice.
The system SHALL NOT implement live switching, deferred application or additional
transition machinery for changing this setting. Both policies SHALL remain
available for the Human listening comparison; neither SHALL be automatically
removed based on the earlier attack preference.

#### Scenario: Playing, paused or held audio prevents a policy change
- **GIVEN** playback owns an active, paused or held resumable voice or admitted pending start/resume
- **WHEN** Settings displays or attempts to change the global policy
- **THEN** the control is disabled with the stop-playback hint and authoritative admission rejects a bypassed attempt
- **AND** no effective, persisted or deferred selection changes and retained source/DSP history stays unchanged

#### Scenario: A fully stopped change applies as one global persisted policy
- **GIVEN** the actual authoritative audio state satisfies the completely stopped condition
- **WHEN** the performer selects the other policy
- **THEN** one global effective policy is accepted and persisted through the existing settings/control path
- **AND** all pads use that accepted policy without changing markers, BPM, pitch or musical assignments

#### Scenario: A captured start awaiting publication still owns its policy
- **GIVEN** Native accepted an original ordinary start or pitch retrigger capture that is waiting for residency, preparation or queue admission
- **WHEN** Settings attempts to change the global start policy before that same request applies or terminates
- **THEN** the authoritative pending owner SHALL refuse the policy change even before the request reaches the callback queue or scheduler
- **AND** the original request SHALL retain the policy accepted at its capture, while rejection, cancellation or final owner retirement SHALL release that pending ownership exactly once

#### Scenario: No available lead or a late start remains explicit
- **WHEN** the chosen policy lacks necessary causal lead or receives a late start
- **THEN** existing target/source/quality requirements govern an explicit correct outcome
- **AND** the implementation does not silently defer the target, move content, invent recovered audio or claim Human acceptance
