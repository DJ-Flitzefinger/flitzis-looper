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

#### Scenario: Only a desired raw policy changes during an existing caused Hold
- **WHEN** a new raw SYNC/PAD/global policy publishes while every source, content, window, accepted timing, loop, pitch, multi-loop, ACK owner and selected-map delivery/geometry projection remains unchanged
- **THEN** publication SHALL invalidate old captured future starts through the original launch/runtime fences while preserving the existing caused Hold transport identity
- **AND** scalar or mapped Hold release SHALL retain its original source/custody/phase guards and resume the preceding effective voice before a later qualified policy retry
- **AND** source/window/timing/loop/pitch/multi-loop or selected-map changes and genuine transport commands SHALL retain their existing supersession rules
- **AND** original counter exhaustion SHALL refuse atomically before publication without changing preceding Hold or audio state

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

#### Scenario: A callback closes with cached scalar adoption still pending
- **WHEN** a scalar policy request remains queued or scheduled as the callback closes while its immutable requested snapshot is still cached
- **THEN** the existing off-realtime close/retirement path SHALL release every dispatch owner and make that unexecuted attempt retryable
- **AND** a later unchanged refresh SHALL reuse the current snapshot without a stranded pending flag or a callback-side final owner destruction

#### Scenario: Source acknowledgement becomes ready without a runtime row edit
- **WHEN** an original source fence or acknowledgement changes independently after a scalar snapshot was cached
- **THEN** unchanged request service SHALL detect that actual Native fence transition and recapture the current original authority without another control toggle
- **AND** stable retries SHALL reuse the cache without repeated owning allocation
- **AND** stale or unavailable sources SHALL still fail the complete actual-E qualification before any effective mutation

#### Scenario: A final old dispatch owner retires while unchanged service retries
- **WHEN** the existing retirement worker is finalizing an earlier scalar attempt while control service retries its unchanged snapshot
- **THEN** the final old dispatch owner SHALL remain reserved until its matching-attempt cleanup completes
- **AND** no newer attempt SHALL begin inside that finalization or be overwritten by the old destructor
- **AND** callback-side owner release SHALL remain analytically bounded without locks or owning allocation

### Requirement: Scalar policy adoption preserves the current scalar trajectory
An independently qualified scalar-to-scalar policy effect SHALL capture the complete current original Native runtime row and full raw request revisions bound to actual source/content/lifetime, physical window, pitch projection and launch/runtime freshness. It SHALL reuse the existing fixed 32-lane capture/preflight/original-claim authority. Every active participant SHALL have supported scalar current and desired state with unchanged source/window/domain and qualified readers. One unsupported, stale, paused/Hold, Native/pitch/Key-Lock, conflicting-work, missing-master/period, unsupported-rate or capacity participant SHALL refuse the whole effect before any effective mutation. Master time/tempo, source, placement, generation, projection and reader semantics SHALL remain unchanged.

#### Scenario: The raw policy changes while a scalar glide already targets the same rate
- **WHEN** a complete current scalar-only cohort has new raw request revisions but equal scalar targets
- **THEN** actual adoption SHALL update requests atomically while preserving every complete pending scalar trajectory
- **AND** it SHALL NOT consume an old glide step, restart, reanchor or treat matching booleans as acknowledgement

#### Scenario: A supported scalar policy resolves a different target
- **WHEN** every participant passes the final actual-E preflight with a newly resolved scalar target
- **THEN** all request/target changes SHALL commit atomically with exact E source position and unchanged reader/PCM
- **AND** the existing .05/512 glide boundary SHALL occur exactly once and settlement SHALL remain pending until actual target convergence
- **AND** publication/cache-hit retry SHALL be idempotent and bounded without another Python cohort queue or repeated owning-work floods

#### Scenario: A scalar attempt awaits execution or its applied rate is still settling
- **WHEN** current qualified attempt feedback is pending, refused or applied while its actual scalar glide remains unsettled
- **THEN** the status SHALL retain preceding actual audio state and distinguish those conditions with bounded human guidance
- **AND** an applied attempt SHALL report its actual execution frame without claiming settled matching until the actual callback confirms the complete current raw request and target
- **AND** a matching settled callback acknowledgement SHALL take precedence over older attempt diagnostics

#### Scenario: An already current scalar policy is undergoing an ordinary tempo glide
- **WHEN** an active callback confirms the complete current scalar raw request and current resolved target while its actual rate remains unsettled
- **THEN** feedback SHALL report tempo settling even if an independent policy attempt was refused
- **AND** this observation SHALL NOT fabricate settled acknowledgement, effective SYNC or an applied attempt
- **AND** absent, malformed, stale or nonmatching target/request evidence SHALL NOT bypass attempt diagnostics or mapped geometry qualification

This independent effect SHALL NOT open representation conversion, mapped admission or musical reader changes. The mathematical conversion contract remains mandatory before every such conversion.

### Requirement: Master-independent scalar policy and idle service remain quiet
Live scalar policy adoption SHALL omit the mixer matching-master/Transport period equality prerequisite for Off and Legacy with matching disabled. Those modes SHALL retain their established speed-only behavior without initializing, clearing or rewriting either master. Fixed and Legacy with matching enabled SHALL retain their original master qualification, including missing/mismatched-master refusal, and all final cohort, timeline, rate, source, phase and reader checks.

#### Scenario: Default REALTIME BPM is on while SYNC and BPM Lock are off
- **WHEN** a scalar Off or matching-disabled Legacy target is serviced with differing or absent mixer matching-master and a valid Transport
- **THEN** the irrelevant master difference SHALL NOT produce missing_master
- **AND** supported adoption SHALL preserve the exact source cursor, preceding PCM, original master states and existing target/glide semantics

#### Scenario: Unchanged service observes a stopped cohort or a later current launch
- **WHEN** the previous attempt reports no_active_voices and complete current Native callback evidence still proves all captured loaded participants inactive, with no active participant elsewhere
- **THEN** unchanged service SHALL retain that attempt without allocating or queuing another owning work item
- **AND** missing, stale, overlapping or mismatched evidence SHALL conservatively allow service rather than invent inactivity
- **AND** subsequent active callback evidence SHALL permit retry or suppress obsolete work when the complete current raw target is already callback-confirmed
- **AND** stopped/off feedback SHALL NOT present no_active_voices as a failed live change or fabricate an applied acknowledgement
- **AND** Busy, Unsupported, capacity, Hold, renderer-configuration and independent source-acknowledgement recovery SHALL remain retryable through the existing service

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

#### Scenario: Accepted scalar timing and a qualified map retain different reader periods
- **WHEN** the same immutable source and physical loop have accepted scalar timing and an independently qualified source map with different musical reader periods
- **THEN** equality of their separate canonical source frame/fraction SHALL NOT stand in for equality of their actual SourceReadPlan, interpolation gains or first PCM sample
- **AND** conversion SHALL retain the existing exact first-PCM guard and refuse an unsupported reader-domain change before effective mutation
- **AND** it SHALL NOT silently replace the accepted scalar period, reinterpret the original map geometry or change the legacy interpolation to make the comparison pass

### Requirement: Musical scalar coordinates preserve the original reader domain
A coordinates-only musical scalar continuation SHALL use an explicit projection mode distinct from legacy and physical-pair trajectories. It SHALL retain the complete original Some(P) loop domain and an already canonical separate Normal source frame/fraction. Its numerical domain SHALL require finite binary64 P in [0.5, 2^64], exact |P minus physical length| at most one frame, a valid physical region, finite initial/target rates in [0.5, 2], and separate canonical pair comparison against P. Construction SHALL reject incompatible, noncanonical or malformed input rather than wrapping it at the anchor. It SHALL derive full-u64 constant-epoch displacement and period reduction exactly on a bounded integer dyadic grid, then round only the canonical fractional remainder. Circular source error SHALL be at most 2^-54 frame per constant epoch. Generic zero fractional displacement SHALL preserve fraction bits, including signed zero, on exact whole-period revisits.

#### Scenario: A valid musical pair begins or exactly revisits its source phase
- **WHEN** a qualified numeric continuation is queried at its anchor or at an exact whole-period displacement
- **THEN** the separate canonical source pair and fraction bits SHALL be preserved
- **AND** the unchanged SourceReadPlan and same immutable readers SHALL produce identical executed taps, gain bits and PCM at that pair
- **AND** the existing musical interpolation, including its legacy phase recombination, SHALL remain unchanged

#### Scenario: A musical period includes a virtual endpoint cursor
- **WHEN** a canonical musical pair is at the physical end inside a longer admitted musical seam
- **THEN** validation SHALL compare the separate pair against the actual period rather than applying the physical-pair frame-before-end rule
- **AND** source ownership and actual tap coverage SHALL still require their original independent qualification

#### Scenario: A copied musical continuation changes target or exhausts its epoch
- **WHEN** the existing scalar glide, copied checkpoint, history query or bounded output advance services the musical pair mode
- **THEN** it SHALL retain the existing .05-per-512-active-frame glide boundary, exact rebases, mode-aware comparisons and immutable constant-epoch addressing
- **AND** unavailable pre-anchor/shifted history and u64 overflow SHALL refuse before mutation or output
- **AND** no source, reader, mapped-conversion, adoption or realtime preparation permission SHALL follow from coordinates alone

The musical coordinate prerequisite SHALL NOT change legacy/physical arithmetic, source timing ownership or reader-domain selection. Its per-epoch bound SHALL NOT be presented as a global ideal unrounded-glide, whole-process resource or music/device acceptance bound.

### Requirement: Supporting exact arithmetic has explicit fixed capacity and rounding
Supporting source arithmetic SHALL use checked fixed-capacity values with at most 6400 magnitude bits and no heap owner. Finite binary64 inputs SHALL decode exactly onto the integer lattice G=2^1074; nonfinite input, lost high bits, arithmetic capacity exhaustion and zero divisors SHALL refuse without changing preceding state. A checked shift SHALL reject discarded high bits, not merely an excessive shift count. Signed Euclidean division SHALL retain the exact quotient and nonnegative remainder without a u64 cycle narrowing.

Exact arithmetic optimizations SHALL preserve the admitted values and refusals of the checked 6400-bit algebra. Zero, unit and symmetric power-of-two multiplication MAY use lossless shifts. Additional products MAY factor exact trailing binary zeros and multiply odd coefficients in checked u128 only when both coefficients and their product fit losslessly; reconstruction SHALL retain the original lossless 6400-bit shift bound. Other products, including native coefficient overflow, SHALL retain the original checked fixed-capacity multiplication fallback rather than newly refuse. A fused binary64-grid/u32 product SHALL decode finite input before multiplying the integer significand, without a floating product or intermediate rounding. Nonfinite input SHALL refuse even with factor zero. Phase-only Euclidean remainder SHALL require a positive nonzero denominator and retain the same nonnegative remainder as exact signed division.

#### Scenario: Optimized operations retain exact admission and edge decisions
- **WHEN** optimized support evaluates signed zero, subnormal or maximum finite inputs, full-width products, negative phases or a decimal knot whose legacy product rounds
- **THEN** canonical numeric zero, overflow refusal, exact phase and single fraction rounding SHALL remain unchanged
- **AND** the original geometry intersection SHALL still compare the exact product against the unchanged legacy-rounded product, refusing any mismatch
- **AND** descriptive timing and observed thread allocation counts SHALL NOT grant stack, callback, cohort or realtime permission

#### Scenario: Native odd-coefficient overflow falls back without narrowing admission
- **WHEN** binary scales are removed exactly but either odd coefficient or its checked native product exceeds u128
- **THEN** multiplication SHALL use the unchanged 6400-bit fallback and retain every preceding accepted product and overflow refusal
- **AND** a native coefficient product that fits SHALL still refuse if its reconstructed full magnitude exceeds 6400 bits
- **AND** signs, canonical numeric zero, raw fraction identity and original rounding/domain guards SHALL remain unchanged

#### Scenario: An exact source ratio rounds a separate fractional frame
- **WHEN** nonnegative numerator N is below 2^6363, positive denominator D is below 2^5304 and the exact source coordinate is at most 2^64
- **THEN** support arithmetic SHALL retain the integer quotient separately and round only its exact fractional remainder once to binary64 using round-to-nearest ties-to-even
- **AND** normal ties, subnormals and a rounded fractional one SHALL be handled exactly, allowing an integer result of 2^64 for subsequent checked loop normalization before any physical address cast
- **AND** converting the full numerator/denominator to floating point SHALL NOT replace exact quotient/remainder rounding

#### Scenario: A numeric zero is distinct from the original raw source fraction
- **WHEN** supporting integer arithmetic canonicalizes zero
- **THEN** it SHALL NOT claim preservation of an original negative-zero fraction or grant source-domain, reader, conversion or history permission
- **AND** the future source converter SHALL still retain raw anchor bits and use the established generic zero-displacement identity

### Requirement: Frozen master horizons use the actual finite projection
Before anchored conversion relies on a frozen master horizon, it SHALL certify ordered frame bounds, finite positive frames-per-beat, a finite anchor and finite endpoint evaluations of the existing nondecreasing TransportBeatProjection. The original evaluation order and u64-to-binary64 plateaus SHALL remain unchanged. An invalid or nonfinite horizon SHALL refuse before effective mutation and SHALL NOT be classified as a genuine source knot or wrap.

#### Scenario: Individually valid master values overflow at a later address
- **WHEN** the existing period is valid but the actual frozen projection becomes nonfinite within the proposed horizon
- **THEN** horizon certification SHALL refuse
- **AND** a full-u64 conversion SHALL require a certified endpoint at u64::MAX rather than infer safety from current E alone

These numerical prerequisites SHALL add no productive conversion caller, second clock, source ownership, oversized copied span or realtime throughput approval. Geometry, reader-domain, original-knot, stack and actual callback-cost qualification SHALL remain separate mandatory gates.

#### Scenario: Cost evidence uses the copied reader path and identifies its limits
- **WHEN** supporting anchored coordinates are profiled before productive conversion
- **THEN** evidence SHALL distinguish isolated queries from complete copied-reader blocks with endpoint preflight, span refresh and advancement, including realistic two/four/six participants, twenty as primary stress and ten as fallback when stress failure may be performance-related
- **AND** historical thirty-two-reader timings SHALL remain diagnostic evidence rather than a throughput release hurdle; internal thirty-two-lane capacity, correctness, layout and ownership bounds SHALL remain unchanged
- **AND** full-reader and checked-reader routes, original knot/wrap bursts, virtual periods, raw fraction edges and large epochs SHALL retain their actual work and report host timings against an explicitly stated frame-count/sample-rate interval
- **AND** compiled unwind evidence SHALL identify its exact executable/PDB and distinguish individual fixed stack adjustments from a complete peak-stack bound
- **AND** component timings SHALL identify their prepared inputs and non-additive observation costs; a counted boundary-search replay SHALL match actual span endpoints and SHALL NOT be presented as an instrumented production call trace
- **AND** a successful software test or observed allocation count SHALL NOT discharge callback/DSP/device timing, maximum stack, source-custody, first-PCM or Human acceptance gates

### Requirement: Qualified anchored boundary probes preserve the exact search
For an already admitted anchored span, local boundary probes MAY precede the unchanged general binary search only after the original geometry, shifted finite-horizon, phase and target-range gates. The concrete unfolded predicate SHALL be total and nondecreasing over that admitted interval, preserving actual master evaluation and cast plateaus. The left predecessor and at most two right successors SHALL remain inside the original search interval; the explicit terminal MAX probe and all subsequent endpoint, reader/domain, rounding and local-rate checks SHALL remain. The original general fallible first_true helper SHALL retain its semantics for arbitrary fallible predicates. Additional probe errors SHALL NOT be ignored or interpreted as boundaries. No original admitted value or refusal SHALL be narrowed or rescued by changing an intermediate overflow; total anchored arithmetic SHALL retain the original 6400-bit bound.

#### Scenario: Qualified nearby boundaries retain the original exact search result
- **WHEN** the original admitted unfolded predicate reaches a source knot or wrap at the left predecessor or one of the first two right addresses
- **THEN** local probes SHALL select the same first true frame as the original binary search, including actual master plateaus and shifted history
- **AND** a boundary farther away SHALL use the unchanged binary search on a bracket with the same first true result
- **AND** each binary search SHALL remain bounded by64 predicates, with at most three additional local probes and the unchanged explicit MAX probe per refresh, for a conservative total at most132
- **AND** terminal MAX-1 requests, invalid intervals, failure, exact pairs, original endpoint normalization and no productive RT permit SHALL remain unchanged

### Requirement: Prepared exact geometry preserves the original loop and reader

An independently prepared initial exact-continuation geometry SHALL certify an explicit intersection domain without changing existing legacy map construction or projection. Preparation SHALL be opt-in and off realtime, retaining the same original immutable map, binary64 seconds/beat anchors, captured B/Q, physical markers, full source extent and original Some(P) reader domain. Every original segment with positive-length overlap with the admitted beat interval SHALL retain trusted coverage, and both original knot products of each such segment SHALL equal their existing binary64-rounded products exactly. Endpoint-only contact SHALL retain the existing interval-open coverage policy rather than include an unused neighboring segment. Exact B plus Q SHALL equal the captured legacy endpoint beat. The original spline SHALL close exactly at the integer physical start and at physical start plus original P. Failure SHALL refuse this new qualification without clipping, moving markers, rescaling, replacing knots or rewriting the original reader period.

#### Scenario: Original variable geometry belongs to the certified intersection
- **WHEN** original trusted knots, exact loop closure and the unchanged bounded musical domain meet the intersection requirements
- **THEN** opt-in preparation SHALL retain the original geometry with an immutable derived certificate
- **AND** later reader compatibility SHALL require the complete original domain to match, including Some(P) bits, source extent and physical range
- **AND** an exact local-rate check SHALL compare original rational slope extrema with the actual frozen frames-per-beat, preserving supported bounds without a clamped approximation

#### Scenario: Legacy geometry is valid but does not meet exact closure
- **WHEN** a used knot product, captured B/Q sum or endpoint correspondence fails the exact intersection proof
- **THEN** existing legacy construction and projection SHALL remain available with their preceding semantics
- **AND** the new exact-continuation qualification SHALL refuse rather than move a canonical source anchor through an unproved modulo
- **AND** unused geometry outside the admitted loop SHALL NOT invalidate an otherwise supported interval

#### Scenario: A numerically similar reader is not the original domain
- **WHEN** a caller supplies a different accepted period, physical extent, source length or None in place of the original Some(P)
- **THEN** the prepared qualification SHALL NOT authorize that reader domain
- **AND** coincident cursor or PCM values SHALL NOT replace original-domain, source-custody or first-PCM guards

This geometry prerequisite SHALL NOT enable an anchored trajectory, live adoption, new clock or source/ACK authority. It SHALL add no hidden wide scan to existing legacy construction. General geometry outside this intersection, actual anchor conversion, copied representation, stack/callback cost and all remaining acceptance gates SHALL remain open.

### Requirement: Anchored mapped coordinates retain the immutable separate source anchor
A coordinates-only anchored mapped mode SHALL be distinct from legacy mapped arithmetic. It SHALL require the original opt-in exact geometry qualification, complete bitwise original Some(P) domain equality, an already canonical Normal separate source frame/fraction, a nonzero marker, supported original nominal and exact local rates, actual E below u64::MAX and the unchanged actual master projection finite through its admitted u64 horizon. It SHALL invert the original source knots exactly at the raw source pair, prove the initial phase lies in [0,Q), retain the original inverse segment and raw fraction bits, and evaluate later positions from that immutable anchor and the exact difference of actual binary64 master evaluations. It SHALL NOT use callback integration, rounded inverse beats, a fitted offset, changed knots or an additional BPM factor.

#### Scenario: A tiny fraction or master cast plateau crosses original geometry
- **WHEN** the anchored mode evaluates an anchor, integer source displacement, original knot, wrap or distant u64 frame
- **THEN** it SHALL classify the original segment and loop side with exact phase comparisons before separately rounding only the source fraction once
- **AND** circular source error SHALL be at most 2^-54 frame relative to the exact original anchored spline for each admitted frozen master epoch
- **AND** every exact source fractional identity SHALL retain the original raw fraction bits, including negative zero and subnormals, without an E-only cursor override
- **AND** only a rounded coordinate exactly equal to the retained exclusive endpoint SHALL normalize to the original loop start before any physical address cast

#### Scenario: Copies and refreshes reach a knot or the terminal output frame
- **WHEN** copied or unequally partitioned evaluation refreshes derived segment bounds
- **THEN** it SHALL preserve the original source/master anchor and produce the same canonical pair as direct geometry evaluation
- **AND** source-knot and wrap searches SHALL use bounded fallible predicates without classifying arithmetic failure as a boundary
- **AND** a finite horizon without another boundary SHALL end at u64::MAX; observing that endpoint SHALL remain possible while constructing or refreshing a renderable span there SHALL refuse without mutation
- **AND** pre-anchor history, noncanonical input, invalid coverage, incompatible domain, nonfinite master and checked arithmetic exhaustion SHALL refuse without replacing preceding state

#### Scenario: Pause or a supported master replacement retains latent exact phase
- **WHEN** an anchored coordinate copy resumes under its unchanged actual master
- **THEN** checked output-address translation SHALL repeat its preceding exact phase while retaining the original source pair, inverse segment and cached original master evaluation
- **AND** a master replacement SHALL require bitwise equality of the old and new actual master at the addressed execution frame and a finite new admitted horizon before replacing the projection
- **AND** replacement SHALL restrict earlier history and retain the original unrounded anchor rather than reanchor at a rounded source pair
- **AND** replacing a copied epoch SHALL reject a different mode or original raw source/master anchor even when the current rounded cursor happens to match
- **AND** the replacement API itself SHALL verify addressed actual master-bit continuity and SHALL NOT reopen history before the preceding permitted frame
- **AND** a changed projection SHALL set the first permitted frame to the actual replacement frame rather than retain an earlier anchor's history floor
- **AND** unsupported paused-derived or otherwise discontinuous master changes SHALL refuse without mutation

The copied mode SHALL remain Copy with no destructor, mapped spans at most 240 bytes and 32 copied SourcePlayback values at most 16 KiB. Wide scratch SHALL remain outside stored spans and lanes. Legacy projection, original SourceReadPlan/interpolation, scalar glide and source/custody/ACK authorities SHALL remain unchanged. This coordinates-only mode SHALL add no productive adoption caller or callback-cost permission: actual first-PCM/custody conversion, stack/throughput, cohort integration and all final acceptance gates SHALL remain required.
