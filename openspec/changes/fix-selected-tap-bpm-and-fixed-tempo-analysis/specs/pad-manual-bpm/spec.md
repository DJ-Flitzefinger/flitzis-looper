## MODIFIED Requirements

### Requirement: Tap BPM Computes And Sets Manual BPM
The system SHALL provide a Tap BPM workflow that computes BPM from an explicit performer tap
measurement and sets it as the target pad's manual BPM.

The Tap BPM workflow SHALL support both pad-specific activation and selected-pad mapped activation.
When Tap BPM is activated from the selected-pad sidebar or a selected-pad Tap BPM input mapping, the
target pad SHALL be the pad selected at execution time. Existing pad-specific Tap BPM mappings SHALL
continue to target their stored pad id.

Tap BPM measurement state SHALL remain scoped to the target pad. If the selected-pad Tap BPM mapping
is executed after the performer selects a different pad, the Tap BPM measurement series SHALL switch
to the newly selected pad using the same target-switch behavior as explicit pad-specific Tap BPM.

Tap BPM measurement state remains Python/control-plane UI state and SHALL NOT add disk I/O,
Python/GIL access, logging, blocking work, heavy allocation, or neural inference to the Rust audio
callback.

#### Scenario: Selected-pad Tap BPM mapping follows current selection
- **GIVEN** a keyboard or MIDI input is mapped to selected-pad Tap BPM
- **AND** pad 1 is selected
- **WHEN** the performer activates the mapped input
- **THEN** the system records a Tap BPM event for pad 1
- **WHEN** the performer selects pad 2 and activates the same mapped input
- **THEN** the system records a Tap BPM event for pad 2

#### Scenario: Existing pad-specific Tap BPM mapping remains pad-bound
- **GIVEN** a keyboard or MIDI input is mapped to pad 1 Tap BPM
- **AND** pad 2 is selected
- **WHEN** the performer activates the mapped input
- **THEN** the system records a Tap BPM event for pad 1
