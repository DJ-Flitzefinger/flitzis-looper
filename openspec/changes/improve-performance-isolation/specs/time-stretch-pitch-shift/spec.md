## ADDED Requirements

### Requirement: Production Trigger Response Has Separate Timing Evidence
The system SHALL measure original input capture, requested target T, actual
effect E and rendered significant onset separately for performance validation.
It SHALL report principal-attack displacement against the source/dry reference,
intentional quantization wait, rejected starts and unavailable preparation.
Nominal Native delay, warmed-handle readiness or queue admission SHALL NOT be
presented as proof of audible attack or device latency.

#### Scenario: Slowed non-master PAD is compared with controls
- **GIVEN** the declared ready 2/4/6-PAD reference workload
- **WHEN** ordinary, Piano or group starts run with quantization disabled
- **THEN** the report includes slowed, unity-master and faster rates with Key Lock ON/OFF
- **AND** it evaluates the 30 ms software input-to-significant-output goal and principal-attack goal against declared authored onset/reference timing
- **AND** late principal attacks cannot pass through earlier pre-ringing, and failed or ambiguous attacks remain explicit
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
- **AND** live earlier originals, group-held deferred Work, SET, AttackAtGrid and history-bound births keep their existing guarded paths

#### Scenario: Terminal rejection releases ordering without releasing custody
- **GIVEN** an earlier same-content original is conclusively rejected while its slot, queued event or worker is still retained
- **WHEN** an independently ready later original or complete playback group reaches its guarded effect
- **THEN** the rejected owner does not delay that effect until its old cleanup deadline
- **AND** live or unobserved earlier owners still preserve original input ordering
- **AND** the old worker, slot, queued event and heavy source/Native owners retain their existing capacity and last-reader retirement
- **AND** no rejected original or partial rejected group can later effect, and fresh source/DSP/OwnACK, original T and recording S/E guards remain enforced

#### Scenario: A terminal member proves its exact old atomic group impossible
- **GIVEN** a retained atomic group has a member conclusively marked rejected
- **WHEN** the runtime polls its exact reserved member owners before readiness acceleration
- **THEN** it attempts rejection of the remaining peers through their normal guarded CAS
- **AND** only peers actually observed rejected cease being ordering barriers; a lost CAS remains live until a later successful retry
- **AND** missing owners, preparation epochs, deadlines or suspicion do not prove impossibility
- **AND** applying/applied effects remain protected, and all slots, events, worker/Native/source owners and capacity remain in normal custody until cleanup
- **AND** recording failure, immutable STOP and last-reader retirement continue through the existing capture lifecycle without an audio or spool prefix
