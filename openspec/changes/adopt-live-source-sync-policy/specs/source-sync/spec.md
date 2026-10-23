## ADDED Requirements

### Requirement: Live policy adoption preserves exact source phase and established master
The system SHALL adopt a supported changed SYNC/Realtime BPM policy or qualified map for living voices through the existing Native runtime and atomic audio claim. It SHALL preserve the exact integer/fraction source cursor at actual execution E, original physical loop intent, content lifetime, voice generation and launch target. It SHALL leave the actual master clock and requested BPM Lock/Key Lock controls unchanged. Unsupported transitions SHALL retain preceding effective audio with truthful pending/refusal state rather than a fabricated adoption ACK.

#### Scenario: A qualified dry loop enables variable synchronization while playing
- **WHEN** a supported complete runtime request and original qualified source delivery reach actual E with a valid current master
- **THEN** the system SHALL preflight exact fractional conversion before mutation and commit the whole admitted cohort at that E
- **AND** future source positions SHALL use the same immutable anchor and original piecewise map independently of callback partitions
- **AND** no seek, restart, master rewrite, fitted offset or extra scalar BPM factor SHALL conceal phase error

#### Scenario: Mapped playback changes to fixed or disabled synchronization
- **WHEN** a supported live request disables variable synchronization
- **THEN** the scalar continuation SHALL start from the exact current source pair and applied local source rate
- **AND** it SHALL use the existing scalar rate glide toward the resolved unclamped target without fabricating prior history
- **AND** fixed matching SHALL remain unsettled until the actual rate target is reached

### Requirement: Live adoption retains original custody and validates the whole cohort
The system SHALL retain qualified effective immutable map/source custody separately from the voice's exact original launch owner and its own retirement lifetime. Adoption SHALL capture and recheck all relevant participants, raw global/PAD/map revisions, source/content/lifetime/placement and timing/master/reader/pitch fences under the existing fixed atomic claim. It MUST reserve owning allocation and last-reader retirement off realtime. No participant SHALL mutate when any required participant is stale, unsupported or lacks capacity.

#### Scenario: One participant cannot join the effect
- **WHEN** one required lane is paused, held, stale, unsupported, outside qualified coverage or lacks original reader/retirement permission
- **THEN** the complete effect SHALL leave every preceding voice, policy, master epoch and pause/Hold cause unchanged
- **AND** no unchanged-looking value or old equal-looking ACK SHALL bypass the validation

#### Scenario: A mapped voice changes effective map custody
- **WHEN** a newer original qualified map is actually adopted
- **THEN** all current rendering and feedback SHALL use that effective custody
- **AND** original launch ownership SHALL remain retained for its original lifetime
- **AND** displaced map and reader owners SHALL retire only after their actual last reader through the existing off-realtime path

#### Scenario: An existing mapped voice stops before live adoption is enabled
- **WHEN** a currently supported mapped launch stops or is replaced after its original successful admission
- **THEN** its exact original launch owner and effective immutable map reference SHALL move together through one existing reserved retirement entry
- **AND** capacity refusal SHALL retain both references and preceding audio without a callback-side final destruction
- **AND** this custody separation alone SHALL NOT enable conversion, map replacement or policy adoption

### Requirement: Successful publication cannot strand retryable adoption
The system SHALL keep adoption idempotent and retryable after successful runtime publication, including unchanged-signature refreshes or Native-owned pending service. It SHALL capture the current original Native runtime snapshot and preserve distinct raw revisions for away/back changes. A newly qualified map SHALL be eligible without requiring another global toggle. Publication, readiness and matching booleans SHALL NOT count as effective audio acknowledgement.

#### Scenario: Admission temporarily lacks capacity after publication
- **WHEN** the runtime request publishes successfully but actual adoption temporarily cannot reserve its required work or retirement
- **THEN** a later unchanged refresh or existing Native service SHALL retry the current request
- **AND** successful repeated service SHALL NOT reapply, restart or reanchor an already adopted voice
- **AND** feedback SHALL distinguish the actual preceding voice from the pending/refused request

#### Scenario: Variable map becomes ready while the global choices remain unchanged
- **WHEN** an original qualified delivery replaces the unavailable map state for the same living content
- **THEN** the system SHALL consider that exact current delivery for adoption without another toggle
- **AND** cancellation or replacement before the claim SHALL preserve old audio rather than adopt stale custody

### Requirement: Physical scalar coordinates preserve the separate source pair
A coordinates-only physical scalar continuation SHALL retain an already canonical Normal source frame and fraction separately and SHALL use an explicit projection mode distinct from legacy scalar trajectories. It SHALL derive constant-rate displacement from the exact u64 elapsed-frame and binary64 rate product, reduce integer displacement before fractional addition, and keep circular rounding error at most 2^-53 source frame for the complete u64 epoch. Generic zero fractional displacement SHALL preserve fraction bits on every exact whole-loop revisit. This construction SHALL NOT authorize live adoption, source ownership or pre-conversion history, and SHALL NOT alter legacy arithmetic or reinterpret a musical domain as physical.

#### Scenario: A physical cursor has a tiny fraction and revisits after a long epoch
- **WHEN** a valid physical continuation is queried at its anchor or an exact whole-loop displacement, including elapsed values beyond binary64 integer precision
- **THEN** its separate fraction bits SHALL remain unchanged and integer addressing SHALL remain within the admitted physical loop
- **AND** copied and partitioned evaluation SHALL use the same immutable epoch and existing glide boundaries

#### Scenario: A continuation cannot retain its declared trajectory contract
- **WHEN** its domain or cursor is malformed, outside, non-Normal or musical, or its elapsed/offset addition exceeds u64
- **THEN** the operation SHALL refuse before changing the preceding trajectory
- **AND** cross-mode exact/rate-target comparisons and unsupported earlier/shifted history SHALL NOT claim equivalence or reconstruct unobserved reads

### Requirement: Anchored conversion has explicit numerical and realtime limits
Before enabling a conversion, the system SHALL define and enforce its numerical domain, analytic error bound and original-knot/wrap classification. It SHALL retain the exact separate source anchor and frozen actual master projection, evaluate from immutable anchors rather than callback integration, and keep existing bitwise continuity guards. It SHALL keep copied mapped spans at most 240 bytes and a 32-lane SourcePlayback copy at most 16 KiB. Heavy geometry, allocation and final retirement SHALL remain off realtime; actual-E freshness and phase checks SHALL remain bounded inside the original callback claim.

#### Scenario: A cursor cannot be converted within the admitted numerical domain
- **WHEN** exact phase, original geometry or the declared bound cannot be established
- **THEN** conversion SHALL refuse before any effective mutation
- **AND** it SHALL NOT substitute a one-frame cursor correction, snapping, nextafter search, relaxed comparison or silent scalar fallback
