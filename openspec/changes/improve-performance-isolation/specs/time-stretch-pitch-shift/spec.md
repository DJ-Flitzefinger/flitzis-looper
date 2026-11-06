## ADDED Requirements

### Requirement: Production Trigger Response Has Separate Timing Evidence
The system SHALL measure original input capture, requested target T, actual
effect E and rendered significant onset separately for performance validation.
It SHALL report principal-attack displacement against the source/dry reference,
intentional quantization wait, rejected starts and unavailable preparation.
Nominal Native delay, warmed-handle readiness or queue admission SHALL NOT be
presented as proof of audible attack or device latency.
The suggested 30 ms reference SHALL be advisory rather than a mandatory
acceptance gate. Optimization SHALL seek the shortest practical latency while
preserving quality and functions, without disproportionate complexity.
One millisecond per block or a particular device buffer size SHALL NOT be a
universal acceptance threshold for Key Lock. Architectural and engine work SHALL
continue to reduce avoidable processing bottlenecks while preserving quality,
functionality and realtime safety. The user selects the lowest supported buffer
that remains free of audible crackles for their device, backend, computer and
workload; the application SHALL NOT silently change device or system settings to
claim that threshold.

#### Scenario: Slowed non-master PAD is compared with controls
- **GIVEN** the declared ready 2/4/6-PAD reference workload
- **WHEN** ordinary, Piano or group starts run with quantization disabled
- **THEN** the report includes slowed, unity-master and faster rates with Key Lock ON/OFF
- **AND** it compares software input-to-significant-output and principal-attack timing against declared authored onset/reference timing and the advisory 30 ms reference
- **AND** earlier pre-ringing cannot stand in for principal-attack timing, and failed or ambiguous measurements remain explicit
- **AND** device/driver and physical listening acceptance remain separate

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
