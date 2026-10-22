## ADDED Requirements

### Requirement: Realtime candidate consumes the existing independent canonical transcript
The test-only R3 realtime Standard/HighConsistency candidate MUST reuse the exact
retained44.1kHz/.5-rate original kick and independent canonical transcript under
q=1,p=2. It SHALL preserve every existing offline lifecycle assertion and negative
result. A separately owned realtime handle SHALL use the actual target pitch and
pre-signal padding/delay observations, never unity warm-retarget or borrowed Live
compensation. No product source clock or public admission SHALL change.

#### Scenario: The retained interior hard-negative input is staged
- **WHEN** both outer caller patterns submit the canonical source transcript
- **THEN** the original Live input prefix remains sample-exact
- **AND** identical Native512-frame submissions and remainder retain every input sample once

### Requirement: Realtime candidate capture retains complete bounded raw output
The candidate MUST feed its queried P zeros, process each real transcript sample
once, finalize once, and drain all actual output to terminal availability within
explicit finite bounds. It SHALL NOT use study, offline duration guarantees,
fabricated output padding, truncation or an altered physical source signal.

#### Scenario: No output is yet available before finalization
- **WHEN** available returns zero during source ingress
- **THEN** ingress continues under the declared bound rather than claiming EOF
- **AND** final completion requires actual terminal available==-1

### Requirement: Candidate evidence preserves original acoustic gates and truthful rejection
Before signal measurement the candidate MUST declare nominal raw_n-N coordinates,
fixed output extent and unchanged timing, original-peak, original-energy and tail
budgets. Both complete nominal output and the retained strict prerequisite SHALL
be evaluated, with full raw evidence and separate end-window availability. The
fixed final-window energy fraction SHALL use the actual nominal-output denominator
as well as the disclosed full-original-energy denominator. Missing
response, incomplete output or a failed unity coordinate control MUST NOT pass.
Acoustic rejection SHALL remain visible and SHALL NOT change criteria or select a
fitted delay. Positive prerequisite evidence SHALL NOT grant whole-domain,
productive ownership, callback, musical or Human acceptance.

#### Scenario: A realtime candidate still has an empty timing intersection
- **WHEN** actual q10/q50 errors admit no common fixed-budget translation
- **THEN** the negative result is retained without a constant-delay workaround
- **AND** no productive backend/profile/guard or effective-SYNC claim is introduced
