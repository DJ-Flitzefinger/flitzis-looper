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
