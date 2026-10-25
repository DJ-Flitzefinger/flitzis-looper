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
- **AND** Busy, capacity and independent source-acknowledgement recovery SHALL remain retryable through the existing service
- **AND** Unsupported SHALL retain deliberate retry and automatic recovery after a relevant actual condition changes, including Hold release, Resume and renderer configuration, under the qualified observation requirement below

### Requirement: Automatic Unsupported service uses actual rejection evidence
The system SHALL distinguish ordinary automatic service from deliberate retry through the existing Native adoption method. `request_input_runtime_policy_adoption(*, service=False)` SHALL retain deliberate behavior by default; ordinary Python publication and cache-hit service SHALL use `service=True`. Automatic service MAY suppress an unchanged Unsupported refusal only after the original pending, dispatch-owner, cleanup, current-target and qualified-idle checks and with a complete current observation correlated to the predicates actually used for that refusal. It SHALL NOT infer eligibility from elapsed time, cursor, render frame, transition progress or a continuously changing telemetry sequence. All original source, claim, reader, PCM and atomic cohort guards SHALL remain authoritative. Other refusal reasons SHALL retain their existing retry behavior.

#### Scenario: Unchanged automatic service observes an unsupported cohort
- **WHEN** 1500 automatic polls and ordinary rendering retain the same qualified Unsupported conditions for the complete original active voice cohort
- **THEN** the refusal attempt SHALL remain unchanged without new owning queue or scheduler work
- **AND** previous effective audio, cursor, readers and actual status SHALL remain unchanged
- **AND** the observation SHALL include all actual voice slots, including older active voices sharing a PAD, rather than only the newest per-PAD feedback

#### Scenario: A relevant condition changes or the caller deliberately retries
- **WHEN** Hold release, Resume, renderer/domain configuration, lane retirement or a qualified source/map/ACK/context change makes the prior complete observation different, or the caller requests deliberate retry
- **THEN** the original service SHALL permit a new attempt after its existing pending and cleanup guards allow it
- **AND** deliberate retry SHALL NOT bypass source, claim, cohort, capacity, actual target or pending-owner checks
- **AND** MissingMaster, ReaderUnavailable, PhaseChanged, Busy and pressure failures SHALL NOT be permanently latched by the Unsupported suppression mechanism

#### Scenario: An asynchronous blocker changes around its actual eligibility read
- **WHEN** a blocker becomes busy only at the actual Unsupported guard and becomes free before the following observation, or an actually busy blocker becomes free before refusal publication
- **THEN** a mismatched refusal observation SHALL NOT suppress the next automatic attempt
- **AND** equality of outer before/after observations alone SHALL NOT establish that the observed state was actually refused
- **AND** the guard SHALL retain its once-read actual predicate inputs as evidence, including the original early blockers and later loop/domain/transition/feed conditions

### Requirement: Retry observations preserve callback and ownership lifetimes
Retry observations SHALL be fixed bounded data with shape storage prepared off realtime. Each callback mixer SHALL publish only to its own observation slot. The existing input-runtime ownership SHALL select the current slot off realtime using a checked nonreused birth and condition token. Successful complete mixer construction and completed explicit ownership rebinding SHALL establish a new observation birth; failed worker construction or invalid epoch-vector construction SHALL preserve the previous slot exactly. Observation registration, registry access, allocation and final destruction SHALL remain off realtime. This observation SHALL NOT own or authorize audio, source, map, Work or scheduler effects.

#### Scenario: An old callback publishes after its replacement
- **WHEN** callback B is successfully constructed after callback A and A subsequently publishes or invalidates its own retry observation
- **THEN** A SHALL NOT overwrite, invalidate or reactivate B's current observation
- **AND** the displaced registry reference and final mixer reference SHALL follow the existing control and CallbackOwned worker retirement paths

#### Scenario: Retry evidence is unavailable or exhausted
- **WHEN** observation identity is unknown, overlapping, stale, poisoned or exhausted, or a bounded checked token publication cannot complete
- **THEN** Unsupported suppression SHALL be disabled and original retry SHALL remain available without wrapping a token or creating a new audio failure
- **AND** no callback SHALL acquire a registry lock, clone or replace the registry owner, allocate shape storage or release the last observation owner

### Requirement: Map readiness feedback preserves preceding actual audio
After original source/binding and feedback qualification, requested Mapped status SHALL use one coherent existing selector observation for geometry, readiness and errors. Concrete missing, preparing or failed-map guidance SHALL precede a generic adoption refusal while retaining its technical details and the preceding actual active, effective and realtime state. Readiness SHALL NOT certify adoption or grid phase. An explicitly qualified ordinary pending birth MAY preserve Play availability as specified below; admitted mapped trajectories, live conversions and failed trigger qualifications SHALL NOT silently select a replacement route.

#### Scenario: A requested map is unavailable and later becomes current
- **WHEN** requested Mapped timing has no qualified current map while preceding audio remains active
- **THEN** feedback SHALL identify the actual map condition without hiding preceding effective audio or inventing an applied acknowledgement
- **AND** a mapped trigger SHALL remain unavailable while an explicitly qualified ordinary pending birth MAY use the original ordinary path
- **AND** a later original current delivery SHALL permit the existing qualified retrigger path without treating that start as continuous phase acceptance
- **AND** feedback SHALL NOT obtain a second geometry snapshot through a separate readiness query

### Requirement: Ordinary Play remains available while variable synchronization is pending

The system SHALL distinguish ordinary Play availability from requested Mapped
timing. A fresh complete runtime row MAY explicitly permit an ordinary pending birth
when the same selector observation proves no effective map geometry, requested
Mapped timing has no usable selected delivery, and the ordinary source binding
and launch region are valid. Delivery withheld while existing geometry awaits
assignment publication SHALL NOT alone qualify this permission. It SHALL be strict
typed data, default false for historical rows, and SHALL be cleared by selection
stripping after stale publication or mismatched binding. Withheld invalid mapped
geometry SHALL NOT receive it. An explicitly supplied stale delivery or a supplied
selected map whose Native launch-geometry preparation fails SHALL retain its
original refusal path. A failed analysis with no effective geometry MAY recover
ordinary Play through a new coherent explicit row while its map error remains
visible; failure SHALL NOT be caught and recaptured within an admitted trajectory.
Permission combined with a selected delivery or without a current source binding
SHALL reject as contradictory input rather than silently discarding permission.

Explicit qualification SHALL capture the existing source generation and source
request incarnation as immutable permission provenance. Omitted map rows SHALL
preserve permission only for the same loaded source projection and ACK owner,
with the stored published runtime revision still current and both captured
counters current. A fresh control capture's zero runtime-revision placeholder
SHALL NOT substitute for or be compared as the published revision. Publication and launch admission
SHALL repeat those source checks. A request-only ABA SHALL NOT inherit permission
even when the binding and PCM address are unchanged.
Publication under its own acquired placement marker SHALL use the existing fixed
placement/source fields; a pending-sensitive new slot capture SHALL NOT reject
the publication merely because its own marker is active. Source, window, ACK and
captured counter changes SHALL still refuse atomically.

The captured owner SHALL retain the raw desired SourceSyncRequest. Only an
explicitly permitted unadmitted birth with no map owner MAY derive actual Legacy
timing before preparation. Ordinary source/ACK, cancellation, parent, timestamp,
residency, Native, first-attack and final-claim rules SHALL remain. Actual Legacy
playback SHALL NOT acknowledge requested Mapped timing, effective SYNC or grid
phase. Existing original policy service MAY later adopt a compatible current map
without retoggling. It SHALL retain the actual physical region and own-Q guards;
an incompatible map extent SHALL remain pending until a genuine guarded region
edit or new launch, never silently moving loop markers or cropping audio.

Missing-map scalar region resolution SHALL be confined to ordinary launch
preparation. Requested mapped loop editing, seek/mode transactions and retained
mapped audio SHALL retain their existing geometry and actual-mode fences. GUI,
direct MIDI/keyboard and global ordinary starts SHALL capture the same launch
classification. Raw nullable choices and persisted preferences SHALL not change.
The Native ordinary-transport API SHALL identify version2 before this row contract
is used; launch preflight SHALL reject an older Native before App construction.

#### Scenario: No-map Play uses ordinary audio and keeps SYNC pending
- **GIVEN** a current loaded source with SYNC requested, Realtime BPM on or untouched, and no usable selected map
- **WHEN** a fresh complete row explicitly permits ordinary pending Play and the user starts the PAD
- **THEN** the original ordinary pipeline SHALL emit its complete beginning with actual Legacy timing under the current BPM Lock, Key Lock, pitch and start policy
- **AND** desired Mapped timing SHALL remain pending with actual playback separately observable and no phase acknowledgement
- **AND** a later compatible current map MAY enter the original atomic policy service without a retoggle

#### Scenario: Missing selection does not erase a refusal fence
- **WHEN** a selected delivery is withheld for invalid mapped extent, or Native strips selection after source/binding mismatch or stale omitted publication
- **THEN** that row SHALL NOT permit ordinary pending Play until a fresh coherent complete projection explicitly does so
- **AND** existing source, ACK, cancellation and whole-cohort failure SHALL retain prior audio and owners

#### Scenario: Map arrival cannot replace the actual loop extent
- **GIVEN** an ordinary pending birth using its captured physical loop region
- **WHEN** a later accepted map requests a different automatic musical end
- **THEN** policy retry SHALL retain the actual region and refuse an incompatible conversion without silently changing markers, Q, source pair or first content
- **AND** editing an actual retained mapped voice SHALL continue to require its original geometry even when the requested selector is unavailable

#### Scenario: A stale Native API cannot distinguish service from deliberate retry
- **WHEN** the Native method lacks the exact optional keyword-only `service=False` contract
- **THEN** the existing capability preflight SHALL reject it before application or audio-device construction
- **AND** automatic service SHALL NOT silently fall back to the old no-argument method

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

### Requirement: Compact borrowed queries preserve the exact anchored reference
An explicit compact-only anchored query MAY use lossless smaller integer grids while retaining the unchanged general exact reference and all of its admissions and refusals. Optional constant-size grid/headroom metadata SHALL be prepared during the existing off-realtime exact qualification scan, retaining both original endpoints of every used segment. An unavailable compact class SHALL NOT invalidate previously accepted exact geometry. No per-segment table, additional map owner, raw pointer or stored-span/voice expansion SHALL be introduced.

At the actual original anchor and after any supported projection/address translation, the compact class SHALL require source and beat grid exponents in [-126,0] and exact coefficient magnitudes below 2^126. Source refinement SHALL include the separate raw source fraction without recombining the pair in floating point. Beat refinement SHALL include the original cached M0 and a conservative grid for EVERY evaluation of the unchanged actual master projection over the complete shifted horizon. For positive finite actual frames-per-beat F with f=floor(log2(F)), the quotient grid SHALL be no coarser than 2^max(-1074,-f-53), further refined by the exact original transport anchor beat. Finite ordered endpoints SHALL bound magnitudes only; endpoint trailing-zero observations SHALL NOT certify the interior grid. Actual master operation order and u64 cast plateaus SHALL remain unchanged.

A temporary borrowed view MAY prepare constants once per reader chunk using at most 256-bit phase and 512-bit coordinate scratch. It SHALL retain the original inverse segment, source pair, M0, complete domain and exact forward segment. The compact factorization SHALL prove the original 6400-bit intermediate and 6363/5304-bit split limits, rather than rescue an excluded original intermediate through cancellation. The separate whole/fraction split, ties-to-even, generic raw-fraction identity including negative zero, fractional carry and virtual 2^64 endpoint SHALL remain exact. Newly rounded result fractions MAY need a finer grid than the raw source anchor; normalization SHALL compare them exactly against the original integer start plus original Some(P) before any physical cast.

#### Scenario: A finer interior master value cannot be represented by a compact grid
- **WHEN** finite master horizon endpoints are individually representable but the whole evaluator requires an exponent below -126 or exceeds coefficient headroom
- **THEN** compact-only preparation SHALL refuse before mutation or reader output
- **AND** the unchanged general reference SHALL remain available through its explicit preceding API, without hidden wide fallback in the compact path

#### Scenario: A reader borrows a prepared compact context
- **WHEN** compact copied playback, endpoint preflight, advancement, refresh or actual reader fill uses a borrowed geometry
- **THEN** it SHALL preserve domain/B/Q/rate, original segment membership and cached endpoint association with that geometry
- **AND** numeric equality SHALL NOT establish original source/map custody, admission, history or ACK permission
- **AND** full and checked fills SHALL share the existing interpolation/tap loop with compact coordinate queries throughout, without a second reader pipeline or hidden wide work
- **AND** failure SHALL preserve the preceding copied playback and checked-reader output as required by their existing contracts

#### Scenario: Compact refresh crosses an original knot, wrap or distant endpoint
- **WHEN** a compact span refresh classifies the original forward segment or computes its exclusive endpoint
- **THEN** exact beat ordering SHALL classify unused remote search probes without requiring their representation on the loop's compact grid
- **AND** the original fallible bounded search, local-probe/MAX rules, endpoint normalization, nominal/exact local-rate guards and terminal refusal SHALL remain unchanged
- **AND** a skipped endpoint SHALL be evaluated from its own original segment rather than extrapolated from the current one

This prerequisite SHALL NOT add a productive mixer/Native feed/adoption caller or establish callback throughput. Evidence SHALL cover complete consuming reader/endpoint/advance/refresh workloads at 2/4/6 active lanes, unconditional synthetic stress 10 as the maximum performance workload, unchanged exact results/first PCM, allocation/layout and explicit 2-MiB compiled-stack gates. Extreme synthetic seam costs SHALL be identified separately. All productive conversion, common-phase, Native, musical, Human and FULL-Goal gates remain required.

### Requirement: Initial dry conversion uses a complete compact callback path
A supported running dry cohort MAY adopt the prepared original exact-intersection map at actual E through the existing runtime publication, scheduler and fixed 32-lane claim. Its compact actual-E constructor SHALL preserve the original separate source pair, inverse segment, M0, full original Some(P) domain, finite shifted master horizon, nominal and exact rate guards and first reader taps/gain/PCM. It SHALL use bounded original-map binary searches and small exact arithmetic without a wide constructor, wide validation scan or hidden wide fallback on realtime. A distinct compact anchored mode SHALL retain the original copied layouts and route generic reader, endpoint, advance, journal slice/merge/trim and continuation operations through that compact representation. Reader constants SHALL be prepared once per full or checked chunk using the same interpolation loops. Original geometry SHALL remain separately retained and borrowed for global refresh and endpoint association.

#### Scenario: A scalar-born dry voice adopts an original prepared map
- **WHEN** a genuine current runtime row, source admission, accepted timing acknowledgement and original selected delivery qualify at actual E
- **THEN** conversion SHALL retain the effective map without fabricating an original launch
- **AND** every active participant SHALL pass the final original claim before any request, trajectory or owner changes
- **AND** unsupported preparation SHALL preserve ordinary legacy launch availability and preceding effective audio

#### Scenario: A compact mapped voice returns to scalar timing
- **WHEN** the complete dry cohort qualifies for a scalar target with the identical original reader domain
- **THEN** scalar continuation SHALL preserve the exact current pair and begin the existing glide from the applied local rate
- **AND** renderer domain selection SHALL follow effective playback mode while historical map custody remains safely retained
- **AND** a genuine original launch SHALL remain retained across effective map replacement

### Requirement: Dry adoption orders global intent and owner retirement
The final dry policy effect SHALL acquire the existing nonblocking global timing lease before the engine-wide placement claim, revalidate the original raw timing/request/source/master fences under that lease and commit the complete cohort infallibly. Retry observations SHALL occur outside its own lease. Only lease CAS contention SHALL retain the exact scheduled Work as Pending; existing unsettled rate/drain work SHALL retain its established retryable refusal rather than block its own completion. Work and all displaced ownership SHALL have retirement capacity before the claim; last-owner destruction SHALL remain on the existing worker.

#### Scenario: A publisher wins before the adoption claim
- **WHEN** a newer global timing or hierarchy publication wins before the final leased claim
- **THEN** the preceding capture SHALL NOT change any participant
- **AND** lease contention SHALL retain the same scheduled owner for a fresh actual-E retry without a partial effect

#### Scenario: Adoption wins the global lease first
- **WHEN** the complete original cohort passes the final placement predicate under the existing global lease
- **THEN** all trajectory/request/custody changes SHALL complete before a competing global publication can succeed
- **AND** retry observation SHALL resume only after releasing that lease

#### Scenario: Source-map cancellation overlaps final admission
- **WHEN** the last original delivery-currentness check succeeds inside the final claim predicate and independent job cancellation overlaps afterward
- **THEN** that successful check MAY linearize admission before cancellation
- **AND** an already effective map SHALL retain genuine delivery-owner provenance and actual source admission without requiring the old analysis job to remain current
- **AND** cancellation visible before that final check SHALL prevent adoption; the system SHALL NOT claim the placement marker serializes the independent cancellation atomic

### Requirement: Conversion retry distinguishes immutable preparation from advancing eligibility
Stable original prepared-map and reader-domain unavailability MAY suppress automatic Unsupported retry only with the actual once-read immutable snapshot/guard evidence. A whole-map nominal or exact slope-rate refusal MAY use that suppression only when the original once-read inputs and complete actual master projection match both retry observations; an actual master or map change SHALL invalidate it. A refusal depending on advancing cursor, raw fraction, actual E, finite-horizon eligibility or first PCM SHALL NOT receive that stable suppression token. Existing source/ACK/raw revision, stale, ABA, unreadable and exhausted observation guards SHALL remain.

#### Scenario: Playback progress makes conversion eligible
- **WHEN** a cursor-dependent refusal is followed by actual playback progress without a runtime policy edit
- **THEN** automatic service SHALL remain eligible to retry the same current request
- **AND** immutable unavailable-map refusals SHALL remain quiet without repeated owning work

#### Scenario: A selected map job is cancelled after preparation but before adoption
- **WHEN** a cached ready derivative loses its original pending delivery currentness without a runtime row edit
- **THEN** unchanged off-realtime service SHALL detect that original availability transition and recapture once through the existing policy revision path
- **AND** the resulting immutable unavailable class SHALL remain quiet until genuine recovery rather than repeat owning SourceChanged work
- **AND** source and acknowledgement fences SHALL always remain mandatory; only additional map-availability recapture MAY be skipped for completely current effective audio that already retained the immutable map

#### Scenario: Mode adoption and completed phase retain distinct cancellation behavior
- **GIVEN** an explicit common-phase request has adopted its mapped mode but has not installed its phase programme
- **WHEN** the original selected delivery is cancelled
- **THEN** service SHALL recapture the newly unavailable preparation once and retain the preceding effective immutable pin and actual audio
- **AND** it SHALL NOT certify or suppress missing phase completion from the earlier MODE acknowledgement
- **GIVEN** the same original request instead completed the actual same-snapshot PHASE claim before cancellation
- **THEN** complete current service SHALL retain that snapshot, programme and effective pin without redundant queued work, while source/ACK fences remain mandatory

This initial productive intersection SHALL NOT certify general decimal-map coverage, common-grid phase convergence, unequal-loop or one-shot EOF completion, Native/pitch adoption, arbitrary mapped history projection, device/hearing acceptance or the complete SYNC feature. Existing adoption refusals for paused/Hold, Native, pitch, Key Lock, residency conflicts and transitions SHALL remain until their actual contracts are implemented. Later pause/Hold/resume/retrigger/rate/history operations on an admitted dry voice SHALL retain their established source and continuation guards.

### Requirement: Declared-Q general maps have an explicit exact reader domain
A separate productive general mode MAY qualify the same original immutable source map for a declared positive binary64 quarter extent Q. It SHALL preserve every stored knot, the integer loaded sample rate, physical markers, source/content/lifetime/ACK custody and the original scalar reader domain. Loaded knot coordinates SHALL mean exact stored seconds multiplied by integer sample rate before rounding. With physical start A, the mode SHALL use the exact original inverse B*=B(A), exact rational B*+Q and exact Pmap=S(B*+Q)-A. It SHALL NOT replace these values with a displayed inverse, a rounded B+Q, the original scalar period or a fitted curve. The initial exact-intersection mode and ordinary legacy launch contracts SHALL retain their preceding admissions and refusals.

General qualification SHALL prove trusted original coverage over the exact positive loop interior, required meter metadata, nominal and exact local-rate bounds, positive Pmap>=0.5, exact |Pmap-H|<=1 for physical length H, and physical-marker closure by nearest integer with positive ties away from zero. Exact A+Pmap SHALL be checked before a physical cast, including a virtual 2^64 endpoint. No extrapolation, clipping, marker movement or new source permission is granted. Unavailable preparation SHALL preserve preceding effective audio and ordinary launch availability.

#### Scenario: A decimal map differs from legacy rounded loaded coordinates
- **WHEN** a genuine current original map with declared Q passes the separate general qualification although its exact seconds-times-rate products or loop boundary differ from the old intersection
- **THEN** the actual-E candidate MAY use the explicit rational mapped reader domain
- **AND** it SHALL preserve the raw separate source pair and first actual tap addresses, gain bits and all-channel PCM against the original reader plan
- **AND** the original scalar period SHALL remain separately retained for a later reverse conversion

### Requirement: Rational mapped addressing shares checked reader semantics
The effective reader domain SHALL explicitly distinguish Physical, scalar musical and rational mapped periods, including complete exact certificate identity in copied playback, read plans, checkpoints and source-feed journals. A rational period certificate SHALL contain a positive reduced numerator and denominator of at most256 bits each. This is an additional admission bound, not a claim that every finite map fits. An existing raw pair SHALL already be canonical in both conversion domains; it SHALL NOT be recombined through a rounded binary64 q+f. Before conversion, both actual reader plans SHALL prove coverage and bitwise first taps, gains and PCM. General-to-scalar conversion SHALL restore the retained original scalar domain, require its current fenced timing compatibility and use the existing glide from the applied local rate. An unavailable original scalar witness SHALL NOT imply a physical domain.

Each general query SHALL prove its exact rational source coordinate is inside the exact loop before splitting whole and ties-to-even binary64 fraction. Exact fractional identity SHALL retain raw fraction bits, including negative zero. Only a rounding carry whose resulting pair reaches or exceeds the exact rational exclusive upper boundary MAY normalize to the original start and positive zero. Arbitrary out-of-domain input SHALL refuse. For Pmap=N/D, the final source interval SHALL use last=min(ceil(Pmap)-1,H-1), original last/first physical knots and exact alpha=(phase-last)/(Pmap-last), rounded once to binary32 with ties to even. Exact Pmap=H SHALL use the equivalent physical tap descriptor, while retaining distinct domain identity. Other intervals SHALL retain ordinary source taps and fraction conversion.

#### Scenario: A different seam is temporarily ineligible
- **WHEN** the current pair is canonical in only one domain or its actual seam taps or gain differ
- **THEN** the complete adoption cohort SHALL remain unchanged
- **AND** that cursor-dependent refusal SHALL receive no stable Unsupported suppression token
- **AND** ordinary playback progress with the same immutable request SHALL remain eligible to recover

#### Scenario: Repeated mapped replacements preserve the independent scalar witness
- **WHEN** an already mapped captured voice replaces its effective map with an initial-intersection target whose reader period differs from the retained original scalar period
- **THEN** a separate numeric replacement helper MAY construct unchanged old compact coordinates under that target's own certified domain
- **AND** it SHALL require identical physical/sample geometry and an available valid nonrational retained original scalar witness, copying only that witness from the previous captured playback
- **AND** original timing freshness, raw target canonicality, actual original-versus-target taps/gain/PCM and the complete final claim SHALL remain mandatory
- **AND** later mapped replacements and reverse conversion SHALL retain that same original witness, including general-to-intersection-to-intersection chains
- **AND** ordinary scalar birth into the old intersection SHALL retain its preceding full-domain equality; missing witnesses or changed physical geometry SHALL refuse without mutation

#### Scenario: Rational arithmetic or resident taps are unavailable
- **WHEN** a full or checked rational read cannot validate every requested coordinate and actual tap
- **THEN** it SHALL refuse before writing reader buffers or publishing a successful source-read receipt
- **AND** no scalar tap fallback, fabricated silent PCM, partial Native/FIFO feed or source-advance permit SHALL replace that failure
- **AND** availability, full reads, checked reads and conversion preflight SHALL use the same checked tap and sample authority

### Requirement: General compact programs preserve original master and bounded ownership
The distinct general epoch SHALL retain the original raw pair, original inverse segment, M0 and unchanged actual master evaluator. It SHALL evaluate U=B(raw)-B*+(M(n)-M0), k=floor(U/Q) and b=B(raw)+(M(n)-M0)-kQ exactly, including the original binary64 master operation order and cast plateaus. Original geometry SHALL remain separately retained; a copied span MAY contain a tagged exact dyadic cycle product and use the copied reader certificate for local queries. Program bytes SHALL NOT be interpreted as legacy float fields. Generic endpoint, advance, refresh, pause/resume, master replacement and journal operations SHALL preserve the complete domain and original scalar witness without wide realtime inverse work or a new owner, pointer, scheduler or reader pipeline.

The general class SHALL separately admit complete shifted-horizon source/beat grids in [-126,0] and coefficient magnitudes below2^126. A rational U coefficient bound below2^128 SHALL prove the nonnegative exact dyadic cycle coefficient fits128 bits. New checked512-bit phase/forward scratch SHALL enforce forward numerator below2^386 and denominator below2^380; separate checked1024-bit rational normalization and binary32 seam arithmetic SHALL enforce their own bounds. These new proofs SHALL NOT broaden the existing compact intersection's256/512-bit theorem or hide a6400-bit fallback. Mapped spans SHALL remain at most240 bytes and32 optional copied playback values at most16KiB, with Copy/no destructor. Actual compiled2MiB stack, zero observed Rust allocation requests and complete consuming2/4/6 plus unconditional stress10 evidence SHALL remain required, with negative extreme seam costs retained honestly.

#### Scenario: A general map is adopted or replaced at the actual execution frame
- **WHEN** the original runtime snapshot and complete fixed32 capture pass source/ACK/request/master freshness, first-reader checks and the existing global-before-placement claim
- **THEN** the complete cohort SHALL commit atomically using existing reserved worker retirement capacity
- **AND** an identical effective original map with new intent witness SHALL retain its complete trajectory
- **AND** map replacement, reverse conversion and history retirement SHALL preserve any genuine original launch and retained original scalar witness
- **AND** source cancellation, stale/ABA, lease contention, capacity pressure and failed participants SHALL retain their preceding ownership and failure contracts

This declared-Q milestone SHALL NOT certify physical-marker-only rational Q, common-grid phase, unequal-loop/OneShot EOF, Native/pitch/Key Lock adoption, arbitrary mapped history projection, music/device/Human acceptance or complete SYNC. Those remain required subsequent FULL-Goal work. Existing GUI/MIDI/persistence hierarchy and requested/pending/effective/phase distinctions SHALL remain; successful map or tempo adoption SHALL NOT certify unproved common phase.

### Requirement: Physical-marker general maps retain exact quarter extent separately from reader period
A separate physical-marker general branch MAY qualify the original immutable map when the original requested extent is None rather than declared Q. For existing integer markers A and Z it SHALL derive exact original loaded-coordinate inverses B(A), B(Z), positive rational Q=B(Z)-B(A), and exact source-frame reader period Pmap=H=Z-A. Original stored binary64 seconds times integer loaded rate SHALL remain exact before rounding. A displayed inverse difference, saved bars, rounded Q, fitted curve, marker movement, original scalar P or scalar physical fallback SHALL NOT replace that contract. The old legacy/intersection attempt, declared-Q admissions and ordinary launch availability SHALL remain intact.

Physical manual extent SHALL retain the existing require_four_four=false contract; declared auto-bars SHALL retain true. Preparation SHALL prove trusted original positive-interior coverage and original local-rate bounds without extrapolation. At an exact start knot the right interior segment SHALL apply; at an exact end knot the left interior segment SHALL apply. An unused exterior gap or meter interval SHALL NOT contaminate the interior, while a real interior gap or absent endpoint SHALL refuse. An absent physical end SHALL retain existing loaded-region interpretation; insufficient map coverage to original EOF SHALL refuse without clipping or implying finite playback.

#### Scenario: Both physical endpoints have rational inverse coordinates
- **WHEN** a genuine current source/map and physical None row pass preparation for ordinary decimal knots whose exact inverse difference has more than64 denominator bits
- **THEN** the productive branch SHALL retain that independently derived Q within its separately admitted bounds, original markers/source custody and independent original scalar P
- **AND** the same actual-E complete claim SHALL prove raw pair, tap addresses, gain bits and all-channel first PCM before any effect
- **AND** no fabricated numeric permit, rewritten control row or saved-bar substitution SHALL establish that evidence

### Requirement: Copied physical-map domains distinguish beat Q from source-frame H
The physical branch SHALL carry a distinct MappedPhysicalQuarters identity with an opaque positive reduced quarter-note N/D certificate of at most256 bits per part,64 bytes total. The complete domain SHALL combine that Q identity with the existing physical/sample region. Its effective source-frame reader period SHALL be exactly H/1, constructed directly without runtime GCD. Q SHALL NOT be interpreted as source-frame period, rate, original scalar timing or a new master factor. Physical-equivalent taps SHALL NOT collapse this domain into Physical, ScalarMusical(H) or the declared mapped-period tag.

Both mapped tags SHALL participate in full plan/history/retry equality, layout/domain validation, full and checked reader prepasses, Native/history refusals and continuation guards. Failure to resolve a mapped reader period SHALL be a terminal numeric refusal, never a legacy physical fallback. Actual physical taps SHALL require Normal mode and a canonical raw pair in[A,Z), and use the shared checked physical-H descriptor. Invalid raw coordinates SHALL NOT be wrapped into eligibility. Generic mapped construction SHALL retain Unavailable original scalar state; explicit continuation SHALL preserve the separately validated original scalar witness through physical/declared/intersection replacements and reverse conversion.

#### Scenario: Numerically equal circles have different extent identities
- **WHEN** a physical-Q playback is paired with a Physical, ScalarMusical(H), declared mapped-period or different-Q plan
- **THEN** full plan/domain equality SHALL refuse before output mutation or a successful source-read receipt
- **AND** the standalone numeric span API SHALL retain its existing private-bound-domain precondition rather than claim it can establish arbitrary certificate association

### Requirement: Physical-quarter programs have a separate bounded numerical proof
A copied physical span SHALL retain an exact nonnegative128-bit wrap count k and receive Q from its already bound copied domain, without a new owner, pointer, clock, scheduler or reader pipeline. It SHALL use unchanged actual M(n) evaluation/order/cast plateaus. With beatgrid g, B(A)=A0/a, B(raw)=R0/r, delta=M(n)-M0, u=(R0+delta*r)*a-A0*r, v=a*r, Q=N/D and qN=N<<(-g), it SHALL evaluate k=floor(u*D/(v*qN)) exactly. Complete shifted-horizon actual-E admission SHALL prove k(MAX)<2^128; subsequent relevant master/resume reconstruction SHALL requalify before mutation.

Original source/beat grids SHALL remain in[-126,0] with input coefficient magnitudes below2^126. Endpoint inverses bound physical Q coefficient below2^127; canonical raw and actual master delta bound U below2^128. Local reconstruction SHALL explicitly check Cnum=k*N<<(-g) < D<<128 before relying on Cnum width below384. Separate fixed1024-bit physical scratch SHALL enforce phase numerator/denominator bounds640/636, local beat bounds513/383, affine offset514, and source numerator/denominator642/636. The declared512-bit386/380 proof and old compact intersection theorems SHALL remain unchanged, with no6400-bit fallback.

The separate physical splitter SHALL prove A*d<=n<Z*d exactly before a single RN-even whole/fraction split, preserve generic raw-fraction identity including signed zero, and normalize only a rounding carry reaching integer Z to A/+0. Integer boundary products require at most700 bits, raw identity762, and the actual RN-scaled remainder689 by correlated numerator/denominator widths; produced fraction LSB SHALL be at least-688. No invalid-coordinate clipping, per-E correction or new rational f32 seam SHALL be introduced. Refresh SHALL retain bounded original-master searches and canonical global endpoints across skipped knots/cycles.

#### Scenario: Physical numeric admission or actual-E conversion fails
- **WHEN** Q reduction, complete horizon, checked scratch, canonical raw, first-reader or original source/ACK/cohort claim cannot be proved
- **THEN** the complete cohort SHALL preserve preceding playback, plans, owners and output
- **AND** immutable preparation failure SHALL remain distinct from progress-retryable raw/seam/E/horizon failure
- **AND** runtime Q reduction, owning allocation, final-owner destruction and unbounded callback work SHALL remain forbidden

Physical spans SHALL remain at most240 bytes, epochs128 bytes and32 optional copied playbacks at most16KiB, Copy with no destructor. Actual compiled Debug/Release2MiB, observed Rust allocation requests and complete consuming2/4/6 plus unconditional stress10 evidence remain required. Earlier declared20/10, tiny20 and physical measurements SHALL remain unchanged historical evidence. The current resource gates concern2/4/6 and maximum stress10; historical20 results SHALL NOT require optimization for20. This physical extent branch grants no common phase/bar commensurability, finite OneShot/EOF, Native/pitch/Key Lock, arbitrary mapped history permission, musical/device/Human acceptance or complete SYNC/REALTIME acceptance. All FULL-Goal obligations remain.

### Requirement: Qualified dry static SYNC converges without changing its declared grid
After the original Fixed request has adopted its ordinary rate and completed its existing active-frame glide, a later original all-participant policy claim MAY install an exact static correction. It SHALL retain the effective source's original signed scalar origin O and frames per quarter F as geometry provenance. The target SHALL preserve the actual intentional selected-A launch at original T: A+F*(M(n)-M(T)). M(T) SHALL be captured from the sole scheduler before preparation, never inferred from the activation cursor or reevaluated using a later tempo projection. This launch reference SHALL NOT certify a new grid/downbeat or turn arbitrary activation drift into an intentional offset.

The admitted first class SHALL retain its actual Physical or already authorized ScalarMusical reader domain. The exact decoded product F*Q SHALL equal that domain's source-frame period P, with the existing compatible-cycle Q, P in [.5,2^64] and positive Q below2^64. A rounded compatibility product, manual/TAP metadata or ready variable map SHALL NOT manufacture accepted timing, a new reader period or bar-phase authority. Nonclosing cases SHALL keep their preceding ordinary audio and unconfirmed phase.

Let u=(M(n)-M(E))*F and D be a feasible signed representative of A+F*(M(E)-M(T))-s0 modulo P. The copied clock SHALL evaluate s0+u+sign(D)*min(u/2^k,abs(D)) from immutable anchors before its own source modulo. It SHALL select the nearest feasible winding, positive on equal distance, and try the other winding when necessary. It SHALL require k from6 through64, exact analytic source rate and corrected rate in [.5,2], analytic correction at most.03125 source frame per output frame, and a finite capture within the admitted output horizon. Actual master cast plateaus SHALL remain plateaus. These are bounds on the analytic rate; they SHALL NOT falsely require every discrete binary64 master increment to be positive. The unwrapped phase SHALL remain continuous, with bounded rate steps; no seek, restart, stop, master rewrite or hidden phase clipping is permitted.

#### Scenario: An ordinary Fixed glide precedes phase adoption
- **WHEN** a new raw Fixed request is applied, including one whose target already matches
- **THEN** the original rate/glide SHALL be adopted first without installing phase
- **AND** later service of the same original snapshot SHALL retain rate-target-current and tempo settlement separately from static-program-current
- **AND** a phase attempt while any required Fixed glide is unsettled SHALL leave the whole cohort unchanged and remain retryable
- **AND** one unsupported Fixed phase participant SHALL refuse the complete phase attempt without undoing already adopted ordinary timing

#### Scenario: Unequal static loops use the same unwrapped master
- **GIVEN** independently qualified eight- and sixty-four-bar source loops
- **WHEN** both actual dry voices render through repeated short-loop wraps and finite capture
- **THEN** each SHALL use its own exact period while retaining the same unwrapped master
- **AND** actual-E raw pair, first taps, gains and all-channel PCM SHALL remain bitwise unchanged
- **AND** copied partitioning SHALL not introduce a new rounding anchor or change the trajectory

### Requirement: Static phase arithmetic and copied authority remain bounded
The static class SHALL separately check a common dyadic grid g in [-126,0]. Every signed integer coefficient AFTER exact alignment to2^g for F, O, raw fraction and every actual master value over the complete horizon SHALL have magnitude below2^126; raw whole/physical period retain at most64 bits. Unscaled f64 magnitude SHALL NOT substitute for this coefficient bound. With k<=64 the finest grid SHALL be at least-316; static-specific products, signed sums, residues and final numerators SHALL remain below384 bits in checked512-bit scratch. Final exact dyadic splitting SHALL use one ties-to-even binary64 rounding, at most2^-54 source-frame error, canonical seam carry and original raw fraction bits whenever exact fractional identity recurs, including negative zero. Out-of-certificate input SHALL refuse without mutation or a cursor-independent retry token.

The static clock SHALL share the existing mutually exclusive copied playback storage with mapped clocks, preserve complete program/domain/request identity, and retain Copy/no destructor and the existing copied-layout limits. Actual source/map/ACK/content/lifetime custody, reader checks, global-before-placement all32 claim, retirement and scheduler remain authoritative. Static scalar reader tags SHALL NOT bypass complete layout/domain and full-chunk reader preflight before output writes.

#### Scenario: Static history is not relabelled constant after capture
- **WHEN** a static clock has reached capture, including nominal rate one
- **THEN** scalar constant-history, Native future/preparation/FIFO retention and constant source-control proofs SHALL remain unavailable for that clock
- **AND** actual source-feed checkpoints SHALL retain its complete copied trajectory identity
- **AND** public grid phase SHALL remain unconfirmed until the separate unified current source/master/request/actual-read witness is implemented

#### Scenario: Pause or source controls invalidate the effective static program
- **WHEN** an actual pause/Hold/resume, changed source grid/domain, source timing revision or successful master/rate effect changes the effective trajectory
- **THEN** the current source pair SHALL be preserved and the static program SHALL be dissolved or freshly qualified through its existing owner
- **AND** resume SHALL not reuse an old addressed master as current live phase
- **AND** failed Hold/global/master claims SHALL leave the complete preceding clock unchanged
- **AND** identical scalar BPM/origin publication SHALL not restart a compatible program
- **AND** retained old-source voices SHALL keep their own frozen grid and prevent a new source's feedback from certifying the whole PAD

Native/pitch/Key Lock and late stem integration remain required separate work. Until then their unsupported static-history paths SHALL truthfully refuse through existing terminal owners before creating stranded work. A genuinely disabled SYNC/parent target MAY dissolve a dry static clock through the original all32 policy claim despite remembered requested pitch/KeyLock preferences; actual Native/retained/lane-busy, pending/queued/admitted work, residency, Hold, source, transition and reader guards SHALL remain. Busy or failed claims SHALL not speculatively invalidate or retire another actor. Ordinary scalar operations SHALL remain recoverable after the successful OFF effect.

This productive dry static milestone SHALL NOT certify variable-map phase, broader static reader-domain conversion, a positive public phase witness, finite OneShot/EOF, Native/pitch/stem integration, practical settling quality, device/music/Human acceptance or complete SYNC/REALTIME. All original FULL-Goal gates and preserved negative resource evidence remain required.

#### Scenario: Equal-tempo accepted refresh preserves other static master projections
- **GIVEN** an active static source and an original accepted refresh for another PAD
- **WHEN** the requested master-period bits equal the actual candidate Transport period
- **THEN** the complete existing actual master projection SHALL remain unchanged, including its original binary64 evaluator
- **AND** accepted refresh SHALL NOT reanchor that equal-tempo projection at the refresh frame
- **AND** the original source/loop/ACK and bootstrap bookkeeping SHALL retain their existing authority
- **AND** a changed-period refresh SHALL still preserve the current master beat and dissolve affected static programs only inside its successful original claim before later fresh qualification

Internal static service custody SHALL correlate the complete successfully claimed original snapshot revision and original global timing fence. Equal-looking parent/intent OFF-to-ON publication before a callback SHALL require a new complete claim. That claim MAY retain a proved compatible numeric trajectory without restarting it, but SHALL NOT borrow an earlier raw request's service acknowledgement or imply actual-read/common-phase acceptance.

### Requirement: Ordinary policy adoption precedes whole-cohort phase installation
The original all32 capture SHALL choose one MODE or PHASE stage before constructing any lane, retain that stage in the capture and recompute it under the original final claim. A new raw request anywhere, or a needed original mapped source/witness/domain/projection replacement, SHALL use MODE for the complete cohort. MODE SHALL retain ordinary Legacy/Off/Fixed/Mapped effects and SHALL NOT construct new static phase clocks in any already-Fixed neighbor. A compatible existing static clock MAY remain; its actual E, projection, F/O/Q/domain, source/timing and readers SHALL still qualify, or it SHALL dissolve on the copy before ordinary behavior. No phase-unavailable neighbor SHALL block an otherwise qualified ordinary opt-out or mode conversion.

PHASE SHALL be considered only for the complete already-current cohort. Its original all-participant numeric, source, ACK, raw request, reader, global-before-placement, claim and retirement guards SHALL remain; one unsupported phase participant SHALL still refuse the complete phase attempt. Original map-current and exact effective target source/witness/domain/projection checks SHALL NOT be replaced by reported mode or Ready status. Stage changes, new participants and stale/cancelled source/map/request inputs at the final claim SHALL preserve every preceding clock and request.

#### Scenario: Unavailable phase cannot trap an ordinary neighboring control
- **GIVEN** a Fixed source whose loop cannot qualify static phase and another active PAD with a new ordinary SYNC opt-out
- **WHEN** the original complete policy claim qualifies its MODE stage
- **THEN** the ordinary requested effect SHALL apply without attempting either PAD's new phase clock
- **AND** actual source cursors, PCM and the master SHALL retain their existing transition contracts
- **AND** later service of the same snapshot SHALL still refuse its unsupported complete PHASE attempt without undoing the ordinary effect
- **AND** a real older Mapped voice converting to Fixed beside a newly born Fixed source SHALL follow the same separation

Nominal unsettled feedback SHALL NOT alone suppress phase service: invalid static custody can also be unsettled. Without a separately proved ordinary-glide observation the original bounded Busy retry behavior SHALL remain. No elapsed-time, rate-equality or phase-pending shortcut grants service or phase acknowledgement.

### Requirement: Recovery-closed dry variable common phase preserves original owners

A genuinely requested variable common-phase programme SHALL use the unchanged
actual Transport master and original trusted source map, own loop Q and reader
domain. It SHALL preserve the actual-E source pair, raw fraction bits, first taps,
gains and all-channel PCM. It SHALL NOT establish a new source map, read permit,
master, source ownership, timing ACK or public phase acknowledgement.

The useful initial class SHALL require exact integer1<=Q<2^48 and an off-realtime
qualification bound to the SAME prepared compact/general/physical geometry.
Existing compact preparation preference SHALL remain supported. Phase metadata
failure SHALL NOT make ordinary mapped preparation unavailable. Original knots,
P/Q, physical/musical/rational reader tags and original scalar witness SHALL remain.

The intentional target SHALL be B(A)+M(n)-M(T) modulo own Q, using the original
exact loop-start inverse and captured scheduler launch master. Canonical source
grid origin D SHALL remain independent provenance, not an unadjusted phase target.
Selected starts SHALL NOT thereby certify new grid/downbeat metadata. Quarter
qualification SHALL retain integer-Q lattice closure. A bar reference SHALL require
uniform known4/4, consistent original normalized downbeat phase and Q divisible by4.
Labels-3 and1 SHALL describe the same modulo4 origin, never different bar zeroes.
Original exact positive-interior metadata SHALL qualify the reference; rounded
Evaluation or saved wishes SHALL NOT. Unknown/inconsistent bar metadata MAY retain
quarter-only convergence and SHALL NOT claim bar proof. Arbitrary rational physical
Q SHALL remain legal ordinary geometry without being called quarter/bar aligned.

#### Scenario: Unequal loops retain one unwrapped master

- **GIVEN** qualified8-bar and64-bar maps on independent PAD/voice state
- **WHEN** their dry common-phase programmes advance
- **THEN** both SHALL evaluate the SAME actual unwrapped M and own Q32/Q256
- **AND** a shorter wrap SHALL NOT reset either master or longer source
- **AND** source knots, partitions, cast plateaus and reader topology SHALL retain their exact contracts
- **AND** numeric capture SHALL NOT itself certify actual-read or Human phase acceptance

### Requirement: Variable correction remains bounded and ordinarily recoverable

If a global candidate is selected, it SHALL evaluate the following exact law for
beta0=B(raw), actual M0=M(E) and t=M(n)-M0:
beta=beta0+t+sign(e)*min(t/2^k,abs(e)), where e is a feasible signed representative
of B(A)+M0-M(T)-beta0 modulo own Q. Nearest feasible winding SHALL be tried first with
positive ties, then the other direction if needed;6<=k<=64. Exact nominal and
corrected rates SHALL remain[.5,2], additional derivative at most.03125 source
frame/output frame, and actual finite-horizon capture SHALL be proved.

Mixed saturated.5/2 maps SHALL NOT use an invalid global correction. A bounded
candidate MAY use an original eligible dyadic seed and delayed full window of
width(2^k+sign)*delta, delta a power of two. It SHALL wait at ordinary rate until
its next full entry, preserve continuous unwrapped phase, and clamp cumulative
correction exactly on capture. Practical waiting/settling quality remains an open
acceptance gate. Legitimate map/control knots do not promise C1 continuity.

Every admitted programme SHALL prove that ALL its emitted RN source pairs remain
accepted by the SAME original geometry's matching typed ordinary recovery
capability. The initial Grid class SHALL retain its original source-grid admission
and SHALL certify exact dyadic forward frame slopes Fj=aj*2^fj, common odd
G=gcd(aj) off realtime, and runtime odd_den(beta0) dividing its numerator times G.
Exact source intercepts, actual master grid, correction/window/capture/wrap branches,
zero cases and canonical/virtual whole bounds SHALL participate in recovery Gout.
Require Gout>=GR=max(-126,Tsource_max-126) and actual old sourceBase.refine(GR).
New metadata SHALL NOT substitute a different subset grid for that old proof.

The Grid recovery/rate gate SHALL precede512-bit phase evaluation and prove r+k<=74,
forward N<2^461 and D<2^454. Source results SHALL be exactly normalized onto GR
with coefficient below2^126 BEFORE the unchanged original reader splitter receives
N126/D127. Old386/380,-432 and physical reader1024 contracts SHALL NOT be widened.
No unadmitted516/509 path, floating remainder, rounded inverse, productive6400-bit
fallback or discarded tiny residue SHALL be used. Raw-0 identity SHALL remain.

#### Scenario: Saturated maps require a qualified window or truthful refusal

- **GIVEN** a map with saturated nominal rates for which global correction is invalid
- **WHEN** an original eligible window candidate is considered
- **THEN** admission SHALL require the unchanged exact rate, ordinary recovery-grid and reader proofs for every emitted pair
- **AND** an admitted programme SHALL wait at ordinary rate until its next full window and clamp cumulative correction exactly on capture
- **AND** an unqualified candidate SHALL leave the original clock and ownership untouched without claiming practical settling

### Requirement: Copied phase state stays bounded and temporally truthful

A distinct phase64-byte payload SHALL retain the raw pair, original inverse segment,
exact Q and checked configuration. Only its frozen projection and immutable E MAY
reconstruct M0. ALL older anchored modes SHALL preserve their own fields and
semantics. Phase SHALL NOT use old shifted pause/reprojection or reinterpret an
old payload. Epoch<=128, span<=240 and32 copies<=16KiB SHALL remain. A typed
nonowning launch-reference payload MAY extend Playback from424 to488 bytes;
it SHALL retain original T/master, monotonic master-coordinate generation and
the actual target map's original start inverse. Map replacement at the same A
SHALL re-certify that inverse. Bare phase epochs/spans SHALL NOT infer a missing
reference or canonical-zero fallback. Full copy equality SHALL include reference
identity. Unadvanced-birth freshness SHALL belong to actual voice-effect custody,
not copied clocks; worker rewind/replay SHALL NOT change that actual-read fact.
Copied state SHALL NOT own a map/source/permit or allocate, lock, perform I/O/GIL,
scan unbounded data or destroy a final owner in the callback.

Discontinuous master coordinates SHALL change a checked generation without ABA;
continuous tempo changes SHALL retain generation and captured M(T). Clear/recreate
and direct anchor helpers SHALL preserve this rule. Only a genuinely same-T,
unadvanced DRY scalar birth with unchanged A/domain and no active/retained Native,
FIFO, source-lane or pending history ownership MAY bind directly to final bootstrap
M(T). A seek/domain edit, including same-frame away/back, SHALL revoke eligibility.
Stale pending scalar work MAY retain its ordinary attack; it SHALL NOT gain phase
permission. Ineligible voices SHALL retain immutable audio/history references and
truthfully unavailable phase. General reanchor and Native-owner-consistent rebinding
remain required milestone work, not accepted exclusions or complete SYNC.

The exact two-inverse debt SHALL use E=(J-mT*c)*a+M0*a*c-I*c and H=Q*a*c,
where beta0=I/a and B(A)=J/c on the original beat grid. Uncaptured evaluation
SHALL retain its original denominator. Saturation comparisons MAY multiply by c
within459 bits; captured evaluation SHALL cancel a symbolically and reconstruct
the original target with denominator c, never round a new anchor. Nonzero debt
SHALL separately prove target-start common-odd and recovery closure. Zero debt
SHALL retain its exact raw-based identity and closure. Original512-bit source
normalization/reader limits and all first-pair/PCM guards remain unchanged.

A safe32-byte span programme MAY store original-grid source cycle*Q128 plus window
seed and typed control information. Local position/derivative/history SHALL use
one programme region. Window entry is right-sided correcting, exit is outside,
exact capture is captured; knot/wrap rules remain original. Fixed NEXT boundary
predicates SHALL be strictly future and monotone even at actual master plateaus.
Repeating region equality SHALL NOT substitute for those predicates. Refresh SHALL
not emit zero-length spans or revisit the same edge indefinitely.

#### Scenario: Numeric currentness does not grant actual launch qualification

- **GIVEN** copied launch provenance whose numeric master reference is current but whose original T predates admission
- **WHEN** the ordinary start applies or a worker replays copied history
- **THEN** numeric currentness SHALL NOT make the actual voice phase-qualified or newly unadvanced
- **AND** changed master generations and same-frame seek/domain ABA SHALL revoke eligibility without rewriting immutable Native/history copies
- **AND** only the stated genuinely same-T unadvanced dry birth conditions MAY permit bootstrap rebinding

### Requirement: Variable phase uses the original whole policy claim

Common-phase eligibility SHALL be shared by stage, service and status. Fixed SHALL
retain its existing phase requirement. Mapped SHALL require phase only when its
real projected request has policy_sync true OR policy_realtime_bpm true. Legacy/
Off SHALL not. Parent/local request masks, actual BPM Lock/master/source/readers
remain separately qualified. Nullable None/None at ANY revision SHALL retain old
creative mapped behavior. Global revision alone or bare Mapped SHALL NOT enable it.

The original all32 MODE stage SHALL precede a later same-snapshot PHASE stage when
no compatible programme exists. PHASE SHALL repeat the original full source/map/
ACK/raw request/global-before-placement/retirement claim atomically. Mapped mode
ACK SHALL NOT suppress required later phase service. Equal qualifying publication
MAY retain numeric trajectory and refresh ONLY exact full-snapshot/global-fence
service custody. A qualifying phase-Mapped to nullable Mapped request SHALL first
ordinary-rebase at actual E; correction SHALL NOT remain hidden behind a new ACK.
Cursor-dependent raw-grid/common-odd/debt/window/recovery/horizon failures SHALL
remain progress-retryable with NO immutable Unsupported/MappedRate token. Existing
whole-map nominal-rate evidence SHALL retain its distinct proof.

#### Scenario: Mode acknowledgement cannot substitute for the phase claim

- **GIVEN** a genuinely requested qualifying common-phase cohort whose MODE effect applied
- **WHEN** the same snapshot is serviced for PHASE
- **THEN** the complete original source/map/ACK/request/global/retirement claim SHALL be checked again atomically
- **AND** a stale participant SHALL leave every preceding clock and request untouched
- **AND** a later nullable creative Mapped request SHALL ordinary-rebase at its actual E before acknowledgement rather than retain hidden correction

### Requirement: Pause and ordinary effects retain a real escape from phase

An actual mapped phase pause/Hold SHALL suspend its exact copied pair/programme
and clear service custody through original invalidation. A successful resume SHALL
preflight ALL changed original causes/source/ACK/readers and rebase the frozen pair
to ordinary mapping under LIVE M(R), without old virtual-M/address shifts. Failed
resume SHALL leave the cohort paused/untouched and retain original ticket ownership.
Duplicate/no-effect gestures SHALL not rebase unrelated lanes. Later original PHASE
service MAY freshly converge. Real global/master/request effects SHALL similarly
ordinary-recover on copies under their existing complete claim; true no-ops SHALL
preserve the exact programme/projection. Failed effects SHALL preserve preceding
clocks/owners/requests in full.

Recovery SHALL preserve ANY-already-mapped scalar witness semantics. Current
compact geometry P MAY differ from retained birth scalar P. Numerical recovery
SHALL use CURRENT effective geometry/domain and carry the PREVIOUS independently
validated scalar witness; it SHALL NOT fake a new scalar birth or weaken old pure
birth constructors. Current-E and suspended-live-R APIs SHALL retain their distinct
preconditions. Different target geometry and actual seam/domain/readers STILL
require their original compatibility checks; this recovery theorem waives none.

A genuine Legacy/Off effect MAY bypass ONLY requested pitch/Key Lock preferences
for productive actual dry mapped/static playback. Actual pitch/Key Lock, wide-mode,
pending/queued/Native/retained/resident/Hold/paused/source/ACK/readers/global/claim/
retirement guards SHALL remain. This SHALL admit no Native work. Requested wishes
SHALL remain stored and unacknowledged until their own operation applies. Fresh
supported Native/stem actions after OFF SHALL use their original owners.

#### Scenario: Pause and resume preserve the frozen source pair

- **GIVEN** an admitted mapped phase voice paused through its original successful claim
- **WHEN** output time advances while paused and an ordinary resume is requested
- **THEN** the paused exact source pair SHALL remain frozen and successful resume SHALL use live M(R) under the full original source/ACK/reader claim
- **AND** recovery SHALL retain the independently validated prior scalar witness even when current geometry P differs from its birth P
- **AND** a failed resume claim SHALL leave the whole affected cohort paused and untouched, preserving ticket ownership

### Requirement: This dry milestone retains broader acceptance gates

Installed/captured programme custody SHALL NOT set grid_phase_confirmed. New phase
history, including unity/post-capture, SHALL remain unavailable to unproved Native/
pitch/stem/preparation paths. Broader decimal/noncommon-odd/fine-input and rational
quarter contracts, unified qualified actual-read phase witness, finite OneShot/EOF,
complete Native/pitch/stem integration, practical settling, old/new resource misses,
device/music/Human and all original FULL-Goal gates SHALL remain explicitly open.

#### Scenario: Numeric phase capture is not musical or Human acceptance

- **GIVEN** a dry programme has captured its numerical target and its original policy claim has applied
- **WHEN** runtime feedback or milestone status is reported
- **THEN** programme custody or MODE/PHASE acknowledgements SHALL NOT set grid_phase_confirmed or certify unproved Native/history paths
- **AND** retained BeatThis B2, music/device/Human and FULL-Goal gates SHALL remain open until their own required evidence and acceptance exist

### Requirement: Productive performance targets six PADs with at most ten under stress

The current optimization target SHALL be productive playback with2/4/6 concurrent
PADs. Synthetic performance stress tests SHALL run at most10 concurrent PADs and
SHALL test10 directly and unconditionally. No20-PAD optimization target or20-to-
conditional10 recipe SHALL remain active. Helpers or architecture SHALL NOT be
added to pursue the retired20-PAD target.

Historical20 measurements SHALL remain unchanged as historical evidence and SHALL
NOT be relabeled passed or treated as a current20-PAD acceptance requirement.
Actual2/4/6 failures and relevant10 stress observations SHALL remain truthful.
User documentation SHALL state the six-PAD optimization target and warn that more
than10 simultaneously playing PADs may cause audio glitches or other performance
problems. This SHALL NOT imply a guarantee for7–10, hardware/music acceptance or a
new hard10-PAD runtime cap. Original32-voice atomicity/capacity/retirement/layout
and all other FULL-Goal obligations SHALL remain unchanged.

#### Scenario: Descriptive stress success preserves measured timing failures

- **GIVEN** productive cohorts of2/4/6 PADs and the unconditional10-PAD synthetic stress cohort
- **WHEN** the descriptive profiling process succeeds but a measured region exceeds its nominal interval
- **THEN** that timing observation SHALL remain negative even when the test process exits successfully
- **AND** no20-PAD workload or optimization SHALL be added, while internal32 correctness remains required
- **AND** the report SHALL NOT infer a7–10/device guarantee, Human acceptance or a hard10-PAD app cap

The Human explicitly accepted the measured FullFraction six-PAD maximum of
11.2481 ms for the first native PC test stand on 2026-10-24. This SHALL permit
that checked implementation to be delivered for real Human testing without
relabelling the above-nominal observation as a timing pass. Further FullFraction
speed work MAY be deferred as an optional optimization. The test stand SHALL
report possible glitches, preserve the unconditional10-PAD stress observation
and retain actual device/Human and all other functional gates.

### Requirement: Declared phase and ordinary recovery share a closed full-fraction capability

If selected, a separate declared-Q FullFraction capability SHALL use the
unchanged admitted original source/beat axes, each on its126-bit grid. It SHALL
decode each actual finite binary64 source pair separately, including fractions
through exponent-1074, and SHALL NOT refine every original axis to that fraction
grid. It SHALL retain the same map, semantic reader domain, original inverse,
selected-start inverse, launch T/M(T), actual master and source/ACK/claim owners.
It SHALL be available from BOTH declared general and compact-preferred geometry
before FullFraction phase is admitted. Metadata failure SHALL leave ordinary
readiness unchanged. This capability SHALL NOT broaden physical-Q admission.

For fraction f*2^g and original source grid s, it SHALL use d=max(0,s-g), H=2^d,
R=whole*2^(d-s)+f*2^(g-s+d), A=dx*H and
I=y0*dx*H+(R-x0*H)*dy. Preliminary arithmetic SHALL be wide and checked; exact
canonical membership and inverse containment SHALL precede the tighter valid-input
proof I<2^(255+d), A<2^(127+d). Start inverse denominator C SHALL remain on the
original grid. Debt and capture SHALL preserve the original whole-own-Q target
and symbolic cancellation of A; no rounded reanchor or denominator accumulation
across recovery SHALL be introduced.

FullFraction ordinary recovery SHALL use an explicit same-size anchored mode,
distinct from the old GeneralCompact theorem. Phase SHALL use a typed Grid or
FullFraction recovery descriptor. All validation, local views, endpoints, copied
advance, journal/address translation, pause/live-R resume, family/retry predicates
and MODE-to-PHASE claims SHALL recognize the correct capability. A new tag SHALL
NOT become permanently wide-anchored Unsupported or acquire scalar/constant
Native history permission. Installed FullFraction phase SHALL recover through its
matching capability even when compact preference left general preparation absent.
Original scalar P, current mapped P and launch qualification SHALL remain separate.

#### Scenario: Compact-preferred phase has a real ordinary escape

- **GIVEN** a declared geometry prepared through the existing compact preference and its matching FullFraction recovery capability
- **WHEN** FullFraction phase emits a fine source pair and a genuine Pause/Hold then live-time resume applies
- **THEN** the original full claim SHALL preserve the paused pair and recover through the same geometry's FullFraction ordinary mode
- **AND** successive recoveries SHALL decode the actual RN64 pair afresh without growing a hidden denominator or replacing the retained scalar witness
- **AND** invalid source, ACK, reader or final-claim inputs SHALL leave the entire preceding cohort and ownership unchanged

### Requirement: Full-fraction arithmetic and readers have separate bounded proofs

The declared FullFraction class SHALL use separately typed fixed2048-bit ephemeral
arithmetic. For d<=1074 and original window grammar r+k<=129, it SHALL enforce
phase source numerator/denominator bounds1590/1583 bits, ordinary declared bounds
1460/1454, source-cycle comparisons1588 and exact period256 cross-products1846/1839.
Initial start*denominator membership MAY require1647 bits. Raw-identity checks
SHALL handle zero and compare effective exponents before shifting; potentially
equal operands and RN64 scaled numerators SHALL fit1636 bits. Original512/1024
proofs and their old input guards SHALL remain intact. No6400-bit callback fallback
or unrelated physical-Q theorem SHALL be inferred from these bounds.

The shared rational reader SHALL retain its semantic period and existing finite,
range, canonical, interior and physical-seam checks. Only a finer nonphysical seam
fraction outside the old exponent-432 class SHALL select a separately typed
FullFraction seam kernel, with numerator1394, denominator1330 and RN32 intermediate
bound1543 bits. It SHALL NOT retry arbitrary old failures. Direct/preflight and
prepared chunk readers SHALL retain identical taps, gain bits, PCM and coverage
refusals. Exact-inside validation SHALL precede one canonical virtual-period carry;
signed zero, RN-even subnormal ties and source first-pair identity SHALL remain.

The old Grid512/u128 affine path SHALL remain available. A rational local view MAY
evaluate [N0+B*(J(n)-J0)]/D only inside its originally checked source segment,
cycle and control region, where J separately decodes actual binary64 master values
and N0 is unrounded. Complete endpoint and region proofs SHALL bound every interior
numerator. Width-qualified512-bit affine coefficients with a prepared1024-bit
splitter, or the Full2048 specialization, SHALL be selected once. The prepared
Small splitter SHALL retain exact cached L<=N<U containment and the original
post-rounding canonical pair/carry checks for every output. There SHALL be no
per-sample inverse reconstruction, GCD, map search or cycle search. Copied epoch128,
span240, Playback488 and32 optional copies within16KiB SHALL remain; larger scratch
SHALL stay ephemeral and satisfy actual2MiB and observed Rust-request checks.
New complete consuming2/4/6 and unconditional10 measurements SHALL be required.

#### Scenario: Decimal phase preserves exact readers and repeated recovery

- **GIVEN** a qualified declared decimal map such as seconds0,0.37,0.91 with beat labels0,1,2 at48kHz and an independently captured source pair
- **WHEN** the original MODE then same-snapshot PHASE claim qualifies the FullFraction path
- **THEN** knots, wraps, correction, capture and later ordinary recovery SHALL preserve independent exact coordinates and actual first taps/PCM
- **AND** nonzero selected starts, full-fraction rational seams and repeated real pause/live-R effects SHALL retain their own references and original ownership
- **AND** successful numeric or descriptive checks SHALL NOT certify public actual-read phase, practical settling, Native/finite playback, BeatThis B2 or Human acceptance
