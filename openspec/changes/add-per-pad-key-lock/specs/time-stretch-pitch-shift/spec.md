## MODIFIED Requirements

### Requirement: Key Lock Preserves Pitch During Tempo Changes
When a pad's effective Key Lock state is enabled, the system SHALL prevent tempo changes from changing that pad's perceived musical pitch.

When a pad's effective Key Lock state is disabled, the system SHALL render that pad with varispeed behavior so pitch follows playback speed. The effective Key Lock state SHALL be resolved per pad and SHALL apply independently to active voices during normal speed changes, BPM-lock tempo ratios, full-mix playback, prepared-stem playback, loop wrapping, active mode toggles, and Multi Loop mixing.

#### Scenario: Tempo increases without pitch increase
- **GIVEN** a pad is playing
- **AND** that pad's effective Key Lock state is enabled
- **WHEN** global speed increases
- **THEN** the pad's perceived pitch remains approximately constant

#### Scenario: Key Lock disabled pad remains varispeed
- **GIVEN** a pad is playing
- **AND** that pad's effective Key Lock state is disabled
- **WHEN** global speed increases
- **THEN** the pad's tempo increases
- **AND** the pad's perceived pitch rises with the playback rate

## ADDED Requirements

### Requirement: Per-Pad Key Lock State Drives Active Voices
The system SHALL choose Key Lock processing for each active voice from that voice's loaded pad-specific Key Lock state.

Global Key Lock updates SHALL overwrite only currently loaded pads' Key Lock state and SHALL leave unloaded pads at disabled Key Lock state. Per-pad Key Lock updates SHALL change only the addressed loaded pad. Updating either state SHALL NOT stop, retrigger, reload, regenerate stems, reanalyze pads, time-slip active voices, or move source loop positions.

#### Scenario: Different pads use different Key Lock modes
- **GIVEN** Pad 1 and Pad 3 are playing
- **AND** Pad 1's per-pad Key Lock state is enabled
- **AND** Pad 3's per-pad Key Lock state is disabled
- **WHEN** the performer changes global speed or BPM Lock creates a non-neutral tempo ratio
- **THEN** Pad 1 uses Key Lock pitch-preservation processing
- **AND** Pad 3 uses varispeed playback
- **AND** neither pad is retriggered by the mode difference

#### Scenario: Global update overwrites loaded live pad states
- **GIVEN** active pads have mixed per-pad Key Lock states
- **WHEN** the performer enables global Key Lock
- **THEN** every loaded pad's effective Key Lock state becomes enabled
- **AND** unloaded pads retain disabled Key Lock state
- **AND** active voices adopt the new state without stopping or retriggering

#### Scenario: Per-pad update affects only one active pad
- **GIVEN** global Key Lock has been enabled
- **AND** Pad 3 and Pad 4 are playing
- **WHEN** the performer disables Key Lock for Pad 3
- **THEN** Pad 3 uses varispeed playback
- **AND** Pad 4 continues using Key Lock pitch-preservation processing
- **AND** the update uses bounded audio-engine state

#### Scenario: Per-pad update ignores unloaded pad
- **GIVEN** Pad 3 has no loaded audio
- **WHEN** the performer or controller requests Key Lock for Pad 3
- **THEN** Pad 3's effective Key Lock state remains disabled
- **AND** no active voice can inherit stale Key Lock state from the unloaded pad

### Requirement: Per-Pad Key Lock Realtime Safety
The system SHALL store realtime per-pad Key Lock state as bounded scalar audio-engine state.

The audio callback SHALL NOT allocate, resize buffers, perform disk I/O, read or write JSON, call Python, acquire the GIL, call UI code, block on locks or waits, log, scan or load plugins, run neural inference, or execute unbounded loops while applying per-pad Key Lock state.

#### Scenario: Callback reads bounded pad state
- **GIVEN** a voice is active for Pad 3
- **WHEN** the audio callback renders that voice
- **THEN** the callback reads Pad 3's Key Lock state from bounded audio-engine state
- **AND** no Python object, project JSON, file path, plugin scan, neural model, or unbounded collection is accessed from the callback

#### Scenario: Per-pad update message is bounded
- **GIVEN** the performer toggles Key Lock for one pad
- **WHEN** the controller publishes the update to Rust
- **THEN** the update contains only bounded scalar data such as pad id and enabled state
- **AND** the audio callback applies the accepted state without allocation-heavy or blocking work
