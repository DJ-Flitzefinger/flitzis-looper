## ADDED Requirements

### Requirement: Rust Input Modules Stay Outside Audio Callback
The system SHALL allow Rust input/control modules outside the audio callback while keeping the
audio callback protected.

Rust modules outside the callback MAY own MIDI ports, timestamping, normalization, in-memory
mapping snapshots, bounded queues, dispatcher threads, and command bridging. The audio callback
SHALL NOT perform MIDI port handling, keyboard polling, JSON access, Python/GIL access, blocking
locks, logging, neural inference, unbounded allocation, or long-running work.

#### Scenario: MIDI callback queues control work outside audio callback
- **WHEN** the MIDI backend callback receives a supported mapped input
- **THEN** Rust normalizes and queues the event outside the audio callback
- **AND** the audio callback only observes any resulting bounded control message later

#### Scenario: Audio callback remains free of Python and JSON
- **WHEN** mapped input playback is active
- **THEN** the audio callback does not call Python
- **AND** it does not read or write mapping JSON
- **AND** it does not own MIDI port connections

### Requirement: MIDI Never Targets Audio Callback Directly
The system SHALL forbid direct MIDI-to-audio-callback execution paths.

MIDI input SHALL pass through the Rust input layer, mapping resolver, and existing bounded
control-command bridge before it can affect audio playback. MIDI input SHALL NOT call audio
callback functions directly and SHALL NOT bypass the established command queue.

#### Scenario: Mapped MIDI trigger uses command bridge
- **GIVEN** a MIDI input is mapped to pad trigger
- **WHEN** the performer sends that MIDI input
- **THEN** the Rust input layer resolves the mapping outside the callback
- **AND** it requests playback through the bounded control-command bridge
- **AND** the audio callback performs only its normal queued-message processing
