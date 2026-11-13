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

### Requirement: Reuse Unchanged Stereo Resampler Controls Within One Call
The pinned Windows dependency build SHALL use the source-owned libsamplerate
overlay to reuse stereo increment, scale and input-step calculations only while
the exact processing ratio remains unchanged within the same call. It SHALL
preserve converter quality, coefficient and tap order, per-frame positions,
history, delay, frame counts and the existing control and ownership boundaries.

#### Scenario: Constant and changing ratios share the original processing path
- **GIVEN** the original stereo SINC converter processes a valid input block
- **WHEN** its processing ratio stays unchanged or changes during that call
- **THEN** invariant controls SHALL be calculated lazily after the original input and termination checks and recalculated on an exact ratio change
- **AND** the complete PCM and consumed/generated frame trajectory SHALL match the original converter, including reset, clone and end-of-input behavior
- **AND** there SHALL be no unity-ratio bypass, persistent cache, new allocation or fast-math policy

#### Scenario: Packaging receives a different dependency overlay
- **GIVEN** a Native binary is attributed to an earlier source commit
- **WHEN** the selected source changes or adds an overlay input
- **THEN** Portable packaging SHALL refuse that stale Native attribution
- **AND** installation SHALL select the overlay from the exact checkout and preserve its pinned upstream source and BSD-2-Clause notice
- **AND** software timing observations SHALL remain separate from device, deadline, quality and platform acceptance

### Requirement: Independent Action Preparation Uses Bounded Concurrent Owners
Captured pitch and ordinary-start preparation SHALL use a bounded pool of at
most two independent off-realtime workers, selected according to host parallelism.
Their aggregate request,
ready and retirement cells SHALL retain the existing action budget. Worker
completion SHALL NOT replace original input order, scheduler fences, atomic
group readiness, common E, source authority or final effect claims.
The single selected-endpoint preparation owner SHALL remain below accepted
action work across the whole pool. Every started worker SHALL be joined after
callback shutdown, including partial worker-start failure. Concurrent Native
construction SHALL preserve independent mutable state and avoid per-constructor
writes to process-global backend defaults; each instance SHALL retain quiet
processing configuration.

#### Scenario: An independent later worker finishes first
- **GIVEN** two original actions or atomic group members retain independent Native and source Work
- **WHEN** their workers finish in a different order
- **THEN** existing original-order and whole-group checks still govern the actual effect and common E
- **AND** failed, cancelled or stale Work returns intact through existing retirement and own acknowledgement paths

#### Scenario: Aggregate preparation capacity is available
- **GIVEN** a whole group passes the existing aggregate capacity check
- **WHEN** one bounded shard has no request cells available
- **THEN** publication uses another available shard without changing total capacity or partially applying the group
- **AND** accepted work on either shard delays a not-yet-started endpoint preparation

#### Scenario: A later worker cannot be started
- **GIVEN** one pool worker has actually started before another worker-start attempt fails
- **WHEN** pool construction returns its error
- **THEN** every already-started worker has been signalled and joined
- **AND** callback publication has not acquired a partially initialized pool

### Requirement: Prepared Starts Preserve Their Own Beginning and Authority
Low-latency start resources SHALL retain current content/source/timing/pitch/stem
authority and independent mutable DSP state. Already-due starts SHALL preserve
the selected physical beginning at actual E. Atomic group members SHALL share E.
Preparation, resets, file reads, blocking and heavy allocation SHALL remain
outside the callback. Capacity or stale work SHALL retain truthful terminal
outcomes rather than mutate replacement content or report success prematurely.

#### Scenario: Rapid repeated and unselected starts
- **GIVEN** a nonunity PAD is retriggered rapidly or is not the selected editor PAD
- **WHEN** ready start material is consumed or preparation is unavailable
- **THEN** each accepted attack uses its own guarded mutable state and original capture
- **AND** readiness misses, deferral and rejection are observable in the validation result
- **AND** no missing source beginning or dry substitute is used to conceal latency

#### Scenario: Exact physical readiness predates the original launch
- **GIVEN** a selected Normal physical-birth reserve with no retained feed offset
- **WHEN** a fresh ordinary action has matching source, render and read trajectory
- **THEN** the readiness comparison permits different launch coordinates only in its comparison copy
- **AND** the transferred request retains the original action's fresh phase, source permit and T
- **AND** a past T executes no earlier than the current callback without consuming the source beginning
- **AND** a group uses this immediate path only when all members are ready with independent mutable state
- **AND** future T, AttackAtGrid source measurement and stale/cancel/capacity guards remain enforced

#### Scenario: Quantization controls intentional waiting, not an unquantized rate limit
- **GIVEN** quantization is enabled with a performer-selected step
- **WHEN** a prepared start becomes ready before its original quantized T
- **THEN** it remains scheduled at T rather than starting early
- **AND** when quantization is disabled, distinct original triggers may arrive faster than 1/64 notes without an artificial musical minimum interval or coalescing
- **AND** actual preparation/resource limits remain observable while Key Lock, stems and other concurrent functions are optimized together

#### Scenario: A completed physical birth precedes its admitted fallback deadline
- **GIVEN** an ordinary or constant independent Piano SetRetrigger FirstSoundAtGrid physical birth, or playback/recording atomic group, has fully completed worker preparation before its fallback E
- **WHEN** a current complete action request, permit, retained physical origin and source/DSP controls validate at E=max(original T, callback)
- **THEN** the same queued event may move earlier with its original input order and capacity
- **AND** every group member receives the same fresh E-bound request before any audible effect
- **AND** the old request remains owned until off-callback retirement, without Native reset or source consumption on the callback
- **AND** an earlier pending original cannot be overtaken, and unfinished workers do not occupy the deferred-due cell through this optimization
- **AND** Piano completion uses its exact retained physical origin, current action-owned permit and DSP intent, without live/history-bound feed
- **AND** recording validates the original pending capture/session, cancellation, geometry and immutable admitted STOP before any member request or E changes
- **AND** after all recording members bind, pending capture S moves to their common E in the same callback, preserving the original event order and reserved STOP lane
- **AND** a pre-start cancelled recording cannot revive, and whole-group capacity failure cannot record or play a partial prefix
- **AND** live SET, gliding/history-bound Piano and AttackAtGrid retain their existing guarded paths

#### Scenario: A late original completes genuine legacy reprepare
- **GIVEN** an independent FirstSound physical birth or complete atomic playback/recording group misses its fallback and follows the existing legacy reprepare path
- **WHEN** every required worker genuinely rebuilds its original physical beginning and successfully publishes preparation for the exact new E
- **THEN** a receipt bound to that E permits one fresh completion bind before the new fallback, using the same full request/source/permit/DSP and final effect fences
- **AND** only the successful off-RT worker releases a previous displaced request; failed or cancelled work retains normal custody and retirement
- **AND** a new reprepare or successful completion consumes that receipt, without resetting replan count or maximum launch-delay bounds
- **AND** all group members share the header E and validate before any binding or pending capture S changes, preserving immutable STOP and original order
- **AND** live earlier originals, SET, AttackAtGrid and history-bound births keep their existing guarded paths

#### Scenario: A wholly held deferred group reuses an unconsumed physical birth
- **GIVEN** the same due event retries with every member already retained in group Work custody from a previous attempt
- **WHEN** no possible earlier original remains and complete fresh later-E requests prove every unconsumed FirstSound physical beginning, exact owner/source/content/epoch metadata and current permit/DSP controls
- **THEN** all member plans and capture/session/cancellation/geometry/immutable STOP fences validate before any bind
- **AND** each prepared or scheduled owner binds the exact expected E to the common actual later E through its guarded CAS, without Native reset or callback preparation
- **AND** only all-member bind success moves common E and pending capture S, consuming each member's existing replan allowance without resetting delay limits
- **AND** a lost bind rejects the whole group before audio or spool stores, retaining all displaced and failed request pins for normal off-RT retirement
- **AND** the same popped event keeps original order and capacity without a new queued-completion receipt, and final all-member/capture claims remain authoritative
- **AND** occupied displaced-request custody, unavailable proof, live predecessors and unsupported birth/feed states retain genuine legacy reprepare rather than weakening their guards

#### Scenario: Terminal rejection releases ordering without releasing custody
- **GIVEN** an earlier same-content original is conclusively rejected while its slot, queued event or worker is still retained
- **WHEN** an independently ready later original or complete playback group reaches its guarded effect
- **THEN** the rejected owner does not delay that effect until its old cleanup deadline
- **AND** live or unobserved earlier owners still preserve original input ordering
- **AND** the old worker, slot, queued event and heavy source/Native owners retain their existing capacity and last-reader retirement
- **AND** no rejected original or partial rejected group can later effect, and fresh source/DSP/OwnACK, original T and recording S/E guards remain enforced

#### Scenario: An in-place SET misses its exact interior ASIO effect
- **GIVEN** an in-place SET has a frozen E strictly inside a qualified stereo 96 kHz ASIO request of 96 frames
- **WHEN** the scheduler reaches E and the original preparation Work has yet to return
- **THEN** the SET is rejected at E and the due pass continues in its original order
- **AND** independently due effects use their original frame boundaries in the same request
- **AND** the reserved slot retains custody until the exact Work returns and can retire off the callback
- **AND** delivered Work without prepared Wet follows its existing capacity and placement rules, and independent births and atomic groups keep their existing retry policy

#### Scenario: One prepared shift fits inside the qualified interior ASIO request
- **GIVEN** the original in-place SET requires Native preparation for its frozen interior E
- **WHEN** the worker's actual input FIFO needs N frames to shift, and all N frames fit before the end of the predicted request
- **THEN** that original Native MAY prepare exactly those N frames and retain the exact callback origin separately from E
- **AND** an insufficient request remainder SHALL return the ordinary unshifted preparation for its existing exact-E rules
- **AND** the callback SHALL verify its actual request origin, E, full Wet source trajectory and remaining ordered effects before transferring the original Native
- **AND** a bound effect on the same voice or its source or rate in [E, E+N) SHALL retain its own frame, with the Wet SET refused before transfer if its continuation would conflict
- **AND** independent effects and effects at E+N SHALL keep their existing timing and authority
- **AND** only the matching request SHALL consume the prepared Wet slice, with its suffix continuing on the same Native and recording confirmed through normal backend submission

#### Scenario: Fast timing parameters meet an aligned ASIO Wet SET
- **GIVEN** an in-place SET carries an already shifted Wet prefix for its frozen E at the start of a qualified stereo 96 kHz ASIO request of 96 frames
- **WHEN** ordered controls reach the SET before the ordinary fast-parameter pass
- **THEN** the SET SHALL claim the worker Native only if the bounded FIFO prefix contains no source-timing parameter and no already staged scalar rate effect can conflict in the request
- **AND** a refused Wet claim SHALL leave the incumbent playable and release the due event at E for following STOP and capture effects
- **AND** when an early claim still owes Wet, only the inspected fast-parameter prefix SHALL drain in that request, with later unbound messages retaining their queued request status
- **AND** a later claim after the parameter pass SHALL validate the actual source trajectory and exclude staged conflicting scalar rate work before transfer
- **AND** the admitted original Native SHALL supply only its exact Wet slice, with recording confirmed through normal backend submission

#### Scenario: A staged source-qualified BPM Lock changes the rate of an aligned Wet SET
- **GIVEN** the mixer has already staged a current source-qualified anchored BPM Lock ON request with no bound history transition
- **WHEN** a qualified aligned ASIO Wet SET reaches its original E and the qualified BPM tuple would change the voice's prepared scalar target in the same request
- **THEN** the SET SHALL be refused at E before Native transfer and the incumbent SHALL remain playable
- **AND** the ordinary BPM service SHALL own application and acknowledgement, with independent STOP, source acknowledgements and recording retaining their timing
- **AND** a BPM tuple that preserves the voice's exact scalar target SHALL leave the SET eligible for its existing exact Wet checks

#### Scenario: A terminal scalar releases staged unanchored BPM Lock ON at an aligned ASIO Wet SET
- **GIVEN** BPM Lock is off and its current staged unanchored ON request waits on a scalar bound beyond the qualified aligned ASIO request
- **WHEN** the original SET reaches E and the scalar fence can become terminal at that boundary before ordinary BPM service
- **THEN** a projected BPM tuple that would change the SET voice's exact scalar target SHALL refuse the SET at E before Native transfer
- **AND** an unchanged scalar target SHALL leave the SET eligible for its existing exact Wet checks
- **AND** ordinary rate service SHALL own the scalar and BPM receipts, with the independent STOP, source acknowledgements and recording retaining their timing

#### Scenario: An anchored BPM Lock follows STOP that transferred an aligned ASIO Wet SET
- **GIVEN** an independent STOP has already pulled the due SET into its original Native during a qualified stereo 96 kHz ASIO request of 96 frames
- **WHEN** the ordinary rate service can immediately apply the current source-qualified BPM Lock ON and its exact scalar target would change a voice that still owes a claimed Wet slice
- **THEN** the service SHALL retry the same request at the exclusive end of the last affected Wet slice and recheck ordinary application authority there
- **AND** an unchanged scalar target or already bound BPM history SHALL keep its ordinary timing
- **AND** independent STOP, source acknowledgements, output submission and recording SHALL keep their existing timing

#### Scenario: Turning off BPM Lock would change the rate of a claimed aligned ASIO Wet slice
- **GIVEN** BPM Lock is effective and an independent STOP has pulled the due SET into its original Native during a qualified stereo 96 kHz ASIO request of 96 frames
- **WHEN** the ordinary rate service can immediately apply the current unanchored BPM Lock OFF
- **THEN** a changed scalar target on a voice that still owes Wet SHALL defer the same OFF request to the exclusive end of the last affected Wet slice, with ordinary application authority checked there
- **AND** an unchanged scalar target SHALL keep its ordinary service frame
- **AND** OFF SHALL clear the effective master period and provenance when it applies
- **AND** independent STOP, source acknowledgements, output submission and recording SHALL keep their existing timing

#### Scenario: A derived loop-only refresh reaches admission after an aligned ASIO Wet claim
- **GIVEN** a qualified stereo 96 kHz ASIO request of 96 frames owes an already claimed Wet slice for the live normal-loop voice of the current source
- **WHEN** the control drain first admits a still-unbound accepted-timing refresh that changes that source's effective loop region without a master effect
- **THEN** the scheduler SHALL bind it at the later of the usual next frame and the exclusive end of that voice's actual claimed Wet slice
- **AND** exact original Native and trajectory checks SHALL govern the owed slice before the refresh can apply
- **AND** source, authority and retirement checks SHALL still govern the refresh and its actual acknowledgement
- **AND** the refresh SHALL leave the control drain available for independent source acknowledgements and preserve already assigned STOP and recording frames
- **AND** already scheduled effects and refreshes without an affected Wet obligation SHALL keep their existing timing

#### Scenario: An accepted master refresh reaches admission after an aligned ASIO Wet claim
- **GIVEN** a qualified stereo 96 kHz ASIO request of 96 frames owes an already claimed Wet slice
- **WHEN** the control drain first admits an unbound accepted-timing refresh whose qualified master would change a voice's exact scalar target or whose coupled loop edit would change the refreshed voice's effective loop region
- **THEN** the scheduler SHALL bind it at the later of the usual next frame and the exclusive end of the last affected claimed Wet slice
- **AND** voices whose exact scalar target and effective loop region are preserved SHALL impose no additional delay
- **AND** the ordinary execution checks SHALL govern the coupled effect and acknowledgement at the assigned frame
- **AND** already assigned frames and independent STOP, source acknowledgements and recording SHALL keep their existing timing

#### Scenario: A later qualified master refresh supersedes an older foreign Wet-bound refresh
- **GIVEN** an older accepted master refresh would first bind at an affected claimed Wet slice's exclusive end
- **WHEN** a later master-bearing refresh is admitted from another current qualified source, including an equal master value
- **THEN** the later admission SHALL supersede the older coupled refresh through the existing input order before either effect can execute
- **AND** the older refresh SHALL have no partial loop, master, provenance or later effect
- **AND** the latest refresh SHALL retain its own normal frame and execution checks, with complete request PCM and recording matching its corresponding standalone reference
- **AND** the global timing fence and completed-drain checks SHALL remain unchanged, preserving an independently bound original rate or BPM history owner and unread-publication checks
- **AND** source-only refreshes SHALL retain their independent per-source authority without revoking a foreign master intent
- **AND** failed admission SHALL preserve prior global and source authority and queued commands

#### Scenario: A source-revoked bound refresh overlaps another original Wet SET
- **GIVEN** an accepted master refresh is already bound at one original voice's Wet end inside the same qualified 96 kHz stereo request of 96 frames as another original interior SET
- **WHEN** a public source-timing edit conclusively revokes that refresh's original source or accepted binding before the interior SET, without changing the global timing fence or master-refresh order
- **THEN** the revoked refresh SHALL no longer veto the other SET's otherwise valid original Wet continuation
- **AND** a still-current refresh whose effect conflicts inside that continuation SHALL retain its original barrier and frame
- **AND** the revoked refresh SHALL reject at its original assigned frame without a partial loop, master, provenance or later effect
- **AND** both original SET targets, Work and Native owners, complete request PCM and recording SHALL match the corresponding reference without the revoked refresh
- **AND** independent STOP, source acknowledgements, output submission and last-reader retirement SHALL retain their existing authority and timing

#### Scenario: A bound master refresh still lacks a dry peer's Key Lock history
- **GIVEN** an accepted master refresh has reached its original assigned Wet-end frame but a live dry Key Lock peer cannot support the requested nonunity target
- **WHEN** ordinary execution retains that same refresh pending and the final mix continues through a later already admitted STOP for an active recording
- **THEN** the refresh SHALL retain its original identity and assigned frame without a partial loop or master effect
- **AND** the final-mix capture tap SHALL stop the original session at its own admitted STOP and copy no later frames, even while the unrelated event remains deferred
- **AND** pending capture starts and uncommitted recording groups SHALL retain their ordinary guards, and equality with the segment end SHALL remain eligible for the next ordinary due pass
- **AND** output submission SHALL still gate capture success, while source acknowledgement, original Native ownership and last-reader retirement keep their existing paths

#### Scenario: A terminal member proves its exact old atomic group impossible
- **GIVEN** a retained atomic group has a member conclusively marked rejected
- **WHEN** the runtime polls its exact reserved member owners before readiness acceleration
- **THEN** it attempts rejection of the remaining peers through their normal guarded CAS
- **AND** only peers actually observed rejected cease being ordering barriers; a lost CAS remains live until a later successful retry
- **AND** missing owners, preparation epochs, deadlines or suspicion do not prove impossibility
- **AND** applying/applied effects remain protected, and all slots, events, worker/Native/source owners and capacity remain in normal custody until cleanup
- **AND** recording failure, immutable STOP and last-reader retirement continue through the existing capture lifecycle without an audio or spool prefix

#### Scenario: Ready live originals retain their captured spacing within a callback
- **GIVEN** each earlier live independent original has successfully bound a whole fresh completion request and retargeted its exact queued event in this pass
- **WHEN** a later independently ready original or complete atomic group validates its own fresh completion
- **THEN** its E preserves every captured predecessor gap using checked predecessor E plus the original T difference
- **AND** the actual retained predecessor header event sorts before the proposed later event, including equal-T event order
- **AND** every original retains an independent effect and mutable DSP owner, without a musical minimum interval or coalescing
- **AND** unknown, busy, unbound, partially bound and deferred predecessors remain barriers
- **AND** final execution still waits for all possible earlier effects; predecessor failure or replan cannot be overtaken
- **AND** group members share E and recording S with the original immutable STOP and all fresh source/permit/DSP/cancellation/last-reader fences
