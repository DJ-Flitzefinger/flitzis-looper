## MODIFIED Requirements

### Requirement: Key Lock Preserves Pitch During Tempo Changes
The system SHALL make Key Lock the master-tempo playback mode so tempo changes do not intentionally
change the perceived musical pitch of playing audio.

When Key Lock is disabled, the same tempo ratio SHALL be rendered as varispeed playback, where the
source advances faster or slower and perceived pitch changes with the playback rate. When Key Lock
is enabled, the source SHALL still advance by the active tempo ratio, but the Rust audio path SHALL
apply bounded pitch compensation so the perceived pitch remains approximately stable without
stopping or retriggering active voices.

This behavior SHALL apply to normal speed changes, BPM-lock tempo ratios, full-mix playback, and
prepared-stem playback through the same voice timing path.

#### Scenario: Tempo increase with Key Lock enabled keeps pitch stable
- **GIVEN** a pad is playing a tonal loop
- **AND** Key Lock is enabled
- **WHEN** the performer increases global Pitch/Speed above 1.00x
- **THEN** the loop tempo increases
- **AND** the perceived pitch remains approximately near the original tonal pitch
- **AND** the pad does not need to be stopped or retriggered

#### Scenario: Tempo increase with Key Lock disabled repitches playback
- **GIVEN** a pad is playing a tonal loop
- **AND** Key Lock is disabled
- **WHEN** the performer increases global Pitch/Speed above 1.00x
- **THEN** the loop tempo increases
- **AND** the perceived pitch rises with the playback rate

#### Scenario: BPM Lock tempo ratio uses the same Key Lock mode
- **GIVEN** BPM Lock is enabled with a valid master BPM
- **AND** a playing pad has valid BPM metadata
- **WHEN** the pad's tempo ratio differs from 1.00x
- **THEN** Key Lock enabled preserves pitch approximately while matching tempo
- **AND** Key Lock disabled renders the same tempo ratio as varispeed repitch

### Requirement: Manual Key Lock DSP Parameters
The system SHALL expose bounded manual Key Lock DSP parameters instead of relying only on fixed
quality presets.

The persisted and Rust-published Key Lock parameter set SHALL include delay minimum in samples,
delay range in samples, delay head count, delay interpolation mode, delay-head window shape,
tempo-ratio smoothing step, and output gain. Supported values SHALL be constrained to:
delay minimum `16..512` samples, delay range `256..1984` samples, combined delay minimum plus
range at most `2032` samples, head count `1..4`, interpolation `linear` or `cubic`, window
`triangle` or `hann`, smoothing step `0.01..0.099`, and output gain `0.25..2.0`.

Changing any parameter SHALL NOT stop, reload, retrigger, regenerate stems for, or reanalyze active
pads. All parameter updates MUST stay inside the same bounded callback-safe processing contract.
Legacy Key Lock quality preset values MAY remain accepted as compatibility aliases, but the
Settings page SHALL publish the concrete bounded parameter set.

#### Scenario: Default Key Lock parameters use the former High baseline
- **GIVEN** a new project is created
- **WHEN** the Key Lock DSP settings are inspected
- **THEN** delay minimum is `64` samples
- **AND** delay range is `1536` samples
- **AND** head count is `2`
- **AND** interpolation is `cubic`
- **AND** window is `hann`
- **AND** smoothing step is `0.05`
- **AND** output gain is `1.0`

#### Scenario: Performer changes Key Lock parameters while audio is active
- **GIVEN** a pad is playing through full-mix or prepared-stem audio
- **AND** Key Lock is enabled
- **WHEN** the performer changes delay range, head count, interpolation, window, smoothing, or output gain
- **THEN** Rust updates only bounded scalar parameter state
- **AND** playback continues without stopping or retriggering the active voice

#### Scenario: Out-of-range Key Lock parameters are rejected before callback use
- **GIVEN** a control-plane caller provides a Key Lock parameter outside the documented range
- **WHEN** the parameter update is validated
- **THEN** the update is rejected or clamped before it can violate delay-buffer bounds
- **AND** the audio callback continues rendering with bounded already-owned state

#### Scenario: Minimum Key Lock DSP values remain bounded
- **GIVEN** a control-plane caller provides head count `1`
- **AND** smoothing step `0.01`
- **WHEN** the parameter update is validated
- **THEN** the update is accepted as the minimum supported Key Lock DSP setting
- **AND** the audio callback continues rendering with bounded already-owned state

### Requirement: No Heap Allocations In Audio Callback
The system SHALL preallocate time-stretch, pitch-compensation, and scratch buffers outside the
audio callback.

The audio callback SHALL process Key Lock, Key Lock parameter changes, BPM Lock, speed
changes, full-mix playback, and prepared-stem playback using bounded per-voice state. It SHALL NOT
allocate heap memory, perform disk I/O, decode audio, log, block, acquire the Python GIL, run
neural inference, load plugins, or resize processing buffers while rendering.

#### Scenario: Callback uses preallocated Key Lock buffers
- **GIVEN** a voice slot has been constructed before the audio stream callback renders
- **WHEN** the performer enables Key Lock and changes Pitch/Speed during playback
- **THEN** the audio callback reuses that voice's preallocated DSP buffers
- **AND** no heap allocation or buffer resize is required in the callback

#### Scenario: Rapid Pitch increases remain bounded with prepared stems
- **GIVEN** a pad is playing through prepared stems
- **AND** Key Lock is enabled
- **WHEN** the performer repeatedly increases Pitch/Speed during playback
- **THEN** pitch-compensation delay-line reads remain within the preallocated buffer bounds
- **AND** the audio callback does not panic or drop playback because of delay-index wrapping
