# time-stretch-pitch-shift Specification

## Purpose
To define Rust-side realtime playback-rate and Key Lock behavior for active voices, including bounded tempo-ratio updates and callback-safe processing.
## Requirements
### Requirement: Real-time Time-stretch And Pitch-shift In Mixer
The system SHALL perform playback-rate and Key Lock processing in real time inside the Rust mixer for each active voice, using bounded per-voice Rust processor state.

#### Scenario: Mixer produces output using stretch processing
- **GIVEN** a pad is playing a looping sample
- **WHEN** the audio callback renders output
- **THEN** the voice’s contribution is generated through the time-stretch/pitch-shift processor
- **AND** the callback remains real-time safe (no blocking)

### Requirement: Global Speed Updates Affect All Active Voices In Real-time
The system SHALL apply changes to the global speed control to all currently active voices without requiring retriggering.

#### Scenario: Slider drag updates tempo for active voices
- **GIVEN** two pads are currently playing
- **WHEN** the performer changes global speed from 1.0× to 1.25×
- **THEN** both pads’ playback tempo changes audibly
- **AND** no pad requires retriggering to adopt the new tempo

### Requirement: Key Lock Preserves Pitch During Tempo Changes
When Key lock is enabled, the system SHALL prevent tempo changes from changing the perceived musical pitch of playing audio.

#### Scenario: Tempo increases without pitch increase
- **GIVEN** a pad is playing
- **AND** Key lock is enabled
- **WHEN** global speed increases
- **THEN** the pad’s perceived pitch remains approximately constant

### Requirement: BPM Lock Tempo-matches Pads Using BPM Metadata
When BPM lock is enabled and a master BPM has been selected, the system SHALL tempo-match pads using their effective BPM metadata.

#### Scenario: Pads with different BPM follow a common global BPM
- **GIVEN** Pad A has effective BPM 120
- **AND** Pad B has effective BPM 90
- **AND** BPM lock is enabled with master BPM 120
- **WHEN** global speed is 1.0×
- **THEN** Pad A uses a tempo ratio near 1.0×
- **AND** Pad B uses a tempo ratio near 120/90

### Requirement: BPM Lock Degrades Gracefully Without BPM Metadata
If BPM lock is enabled but a pad lacks BPM metadata, the system SHALL continue playback and fall back to non-BPM-matched behavior.

#### Scenario: Missing BPM falls back to global speed
- **GIVEN** BPM lock is enabled
- **AND** a pad is playing with unknown BPM
- **WHEN** global speed changes
- **THEN** the pad continues playing
- **AND** the pad follows the global speed multiplier as a fallback

### Requirement: No Heap Allocations In Audio Callback
The system SHALL NOT perform heap allocations during the audio callback while applying time-stretch and pitch-shift.

#### Scenario: Callback remains allocation-free
- **WHEN** the mixer renders audio with time-stretch enabled
- **THEN** no heap allocations occur


<!-- Added from add-phase-aware-playback-sync -->
### Requirement: BPM Lock Master Tempo Shares Transport Tempo Without Implicit Pad Phase Sync
The system SHALL use accepted BPM-lock performance master BPM updates as the shared Rust tempo for
both BPM-ratio matching and transport-grid timing.

When BPM lock is enabled, existing BPM-ratio tempo matching SHALL remain the active voice behavior.
When a valid performance master BPM is recomputed and accepted by the audio thread, Rust SHALL
apply that BPM to both mixer tempo matching and the permanent transport grid while preserving the
transport's current bar phase at the current output frame. This update SHALL NOT reset the
transport output-frame clock, stop, restart, retrigger, or time-slip active voices.

Setting per-pad BPM, enabling Key Lock, or changing pitch/speed controls SHALL NOT anchor the
transport downbeat to a pad by side effect. Qualified BPMLOCK setup and acknowledged
accepted-timing refreshes MAY arm the internal one-time session bootstrap through existing
source/content/ownership checks. No unqualified or repeatable phase-anchor command SHALL remain.

#### Scenario: BPM lock master tempo updates transport tempo without pad phase sync
- **GIVEN** BPM lock is enabled
- **AND** the permanent Rust transport has an existing master BPM and downbeat anchor
- **WHEN** the valid performance master BPM is accepted by the audio thread
- **THEN** existing active voices continue using BPM-ratio tempo matching
- **AND** the transport grid uses the same master BPM
- **AND** the transport preserves its current bar phase without anchoring to a pad

<!-- Added from add-phase-aware-playback-sync -->
### Requirement: Unavailable Bootstrap Reference Preserves Tempo Matching
The system SHALL preserve ordinary BPM-lock tempo matching while its qualified internal
session-bootstrap reference is inactive, paused, missing or lacks valid BPM/timing metadata.
Rust SHALL leave master phase unchanged until that same qualified reference is available.

#### Scenario: Missing reference metadata preserves tempo matching
- **GIVEN** BPM lock is enabled and its internal bootstrap reference lacks valid timing metadata
- **WHEN** Rust services the pending bootstrap at an ordinary callback boundary
- **THEN** Rust does not update master phase from that pad
- **AND** ordinary BPM-ratio tempo matching continues where metadata is available


<!-- Added from add-per-pad-key-lock -->
### Requirement: Per-Pad Key Lock State Drives Active Voices
The system SHALL resolve each loaded pad's requested Key Lock mode from one global boolean default
and that pad's nullable override: an explicit boolean SHALL take precedence, and `null` SHALL
inherit the current global default. One authoritative control-intent resolver SHALL serve UI,
persistence restore, new-source completion and live publication.

Global Key Lock changes SHALL update the default and publish changed effective requests only for
inheriting loaded pads. They SHALL NOT rewrite explicit overrides. Per-pad ON, OFF and inherit
requests SHALL change only the addressed loaded pad's override. Unloaded pads SHALL retain no
override and SHALL use disabled neutral live state until a successful source load is published.

Requested intent and confirmed Native live state SHALL remain separate. Failed enqueue, preparation
or Native acceptance SHALL retain the requested origin and previous confirmed Native state, and
SHALL expose failure rather than replacing an override with an observed audio boolean. Acceptance
and pending/error projections SHALL be associated with the matching pad/source request so stale
results cannot confirm newer intent. Updating intent SHALL NOT stop, retrigger, reload, regenerate
stems, reanalyze pads, time-slip active voices, or move source loop positions.

#### Scenario: Different pads use different Key Lock modes
- **GIVEN** Pad 1 and Pad 3 are playing
- **AND** Pad 1 has an accepted explicit ON override
- **AND** Pad 3 has an accepted explicit OFF override
- **WHEN** the performer changes global speed or BPM Lock creates a non-neutral tempo ratio
- **THEN** Pad 1 uses Key Lock pitch-preservation processing
- **AND** Pad 3 uses varispeed playback
- **AND** neither pad is retriggered by the mode difference

#### Scenario: Global update changes only inherited pad requests
- **GIVEN** loaded Pad 1 inherits the disabled global default
- **AND** loaded Pad 3 has explicit OFF and loaded Pad 4 has explicit ON
- **WHEN** the performer enables global Key Lock
- **THEN** Pad 1 requests enabled Key Lock while retaining a `null` override
- **AND** Pad 3 remains explicitly OFF and Pad 4 remains explicitly ON
- **AND** only changed inherited effective requests are published
- **AND** accepted active voices adopt the change without stopping or retriggering

#### Scenario: Per-pad update affects only one active pad
- **GIVEN** global Key Lock has been enabled
- **AND** Pad 3 and Pad 4 are playing and inherit that default
- **WHEN** the performer disables Key Lock for Pad 3
- **THEN** Pad 3 retains explicit OFF and uses varispeed after Native acceptance
- **AND** Pad 4 continues using Key Lock pitch-preservation processing
- **AND** the update uses bounded audio-engine state

#### Scenario: Clearing an override restores the current default
- **GIVEN** global Key Lock is enabled and loaded Pad 3 has explicit OFF
- **WHEN** the performer clears Pad 3's override
- **THEN** Pad 3 retains `null` intent and requests enabled Key Lock
- **AND** later global changes affect Pad 3 without recreating an explicit override
- **AND** the global default and every other pad's override remain unchanged

#### Scenario: Native rejection preserves requested origin
- **GIVEN** Pad 3's confirmed Native state is OFF
- **WHEN** the performer requests explicit ON and publication or preparation fails
- **THEN** Pad 3 retains explicit ON intent and the reported failure
- **AND** its confirmed Native state remains OFF until a matching acceptance is confirmed
- **AND** failure or telemetry does not rewrite the override to OFF or inheritance

#### Scenario: Per-pad update ignores unloaded pad
- **GIVEN** Pad 3 has no loaded audio
- **WHEN** the performer or controller requests Key Lock for Pad 3
- **THEN** Pad 3 retains a `null` override and disabled neutral live state
- **AND** no active voice can inherit stale Key Lock state from the unloaded pad


<!-- Added from add-per-pad-key-lock -->
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


<!-- Added from repair-key-lock-master-tempo -->
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


<!-- Added from replace-key-lock-with-rubberband -->
### Requirement: Rubber Band Processing Is Bounded In The Callback
The system SHALL use preallocated per-voice Rubber Band state and bounded callback work for Key Lock processing.

The audio callback SHALL NOT allocate heap memory, resize buffers, perform disk I/O, decode audio, log, block, acquire the Python GIL, run neural inference, load plugins, or spin waiting for Rubber Band output. If Rubber Band shifted output is unavailable for part of a callback block, the system SHALL use a deterministic finite fallback and continue rendering without unbounded refill or retrieve loops.

#### Scenario: Callback uses preallocated Rubber Band buffers
- **GIVEN** a voice slot has been constructed before callback rendering
- **AND** Key Lock is enabled
- **WHEN** the performer changes Pitch/Speed during playback
- **THEN** the audio callback reuses preallocated Rubber Band staging and output buffers
- **AND** no heap allocation or buffer resize is required in the callback

#### Scenario: Missing shifted output does not spin
- **GIVEN** a Rubber Band voice has not produced enough shifted frames for the current callback block
- **WHEN** the callback renders the block
- **THEN** the callback fills the missing frames using the documented bounded fallback
- **AND** the callback does not loop unboundedly waiting for Rubber Band output


<!-- Added from replace-key-lock-with-rubberband -->
### Requirement: Rubber Band Latency And Playhead Semantics
The system SHALL document and account for the Rubber Band backend's fixed block size and start delay before declaring the backend ready for user testing.

Playhead telemetry SHALL remain source-frame based. Rubber Band output latency SHALL NOT change loop-region ownership, source-frame wrapping, waveform-editor playhead reporting, trigger quantization, or transport scheduling semantics unless a later focused spec explicitly changes those contracts.

#### Scenario: Latency is documented for the selected backend
- **GIVEN** the Rubber Band backend is initialized for the app output sample rate and channel count
- **WHEN** the backend reports its fixed block size and start delay
- **THEN** the values are recorded in implementation documentation or tests
- **AND** the branch explains whether the first implementation compensates or accepts the output delay

#### Scenario: Playhead remains source-frame based
- **GIVEN** a pad is playing with Key Lock enabled
- **WHEN** the waveform editor receives playhead telemetry
- **THEN** the reported playhead follows the voice's source-frame position
- **AND** Rubber Band output latency does not shift the reported loop position


<!-- Added from repair-multi-loop-bpm-sync -->
### Requirement: BPM Lock Active Voice Timing Avoids Cumulative Rounding Drift
The system SHALL render BPM-locked active voice loop phase from the Rust master output timeline, or from a mathematically equivalent non-cumulative timing model, so per-callback integer rounding cannot accumulate audible inter-pad drift.

The repaired timing model SHALL keep source-frame progression, Key Lock processing, and BPM-ratio tempo matching consistent for full-mix and prepared-stem playback. It SHALL NOT require stopping, retriggering, or resetting Rubber Band state during ordinary loop wrapping.

#### Scenario: Callback segmentation does not change BPM-locked phase
- **GIVEN** BPM Lock is enabled with valid per-pad BPM metadata
- **AND** the same output duration is rendered once using fixed callback segments and once using variable callback segments
- **WHEN** active voices reach the same absolute output-frame position
- **THEN** their BPM-locked source loop phase is equivalent in both renders
- **AND** the result does not depend on how prior callback chunks were split

#### Scenario: Key Lock does not introduce a second timing path
- **GIVEN** BPM Lock is enabled
- **AND** Key Lock is enabled
- **WHEN** active voices wrap their loop regions repeatedly
- **THEN** Rubber Band pitch compensation consumes the same repaired source-frame sequence as Key Lock disabled playback
- **AND** Rubber Band output latency does not redefine source loop phase, trigger quantization, or the Rust master output timeline

