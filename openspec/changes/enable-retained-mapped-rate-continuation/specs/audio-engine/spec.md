## ADDED Requirements

### Requirement: Claim mapped Native rate continuations with original source custody
The engine SHALL stage and revalidate every participant and mapped replacement
before any global rate effect, using existing source, lifetime and claim owners.

#### Scenario: A retained source outlives its bank replacement
- **GIVEN** productive mapped source A remains pinned after the bank publishes source B
- **WHEN** a global rate request captures that voice
- **THEN** explicit retained authority SHALL prove A's own admission, effective Native owner and voice lifetime
- **AND** B's slot or ACK SHALL NOT authorize A, and failed Current authority SHALL NOT fall back to Retained
- **AND** shared scalar-policy Current capture SHALL preserve its existing semantics

#### Scenario: A last participant refuses the complete claim
- **WHEN** any participant lacks source/reader/currentness proof, counter capacity or supported direct continuation
- **THEN** the master, every preceding voice, Native/FIFO state and effective tuple SHALL remain unchanged
- **AND** a successful complete claim SHALL install only prechecked fixed data without callback allocation, reset, wait or owner destruction
- **AND** tempo acceptance SHALL NOT assert musical output phase or Human acceptance

#### Scenario: Repeated pure numeric projection uses bounded original-owner storage
- **GIVEN** repeated canonical projection of the same owned Birth or Current origin and target
- **WHEN** a successful strict-factory result is reused from two fixed slots for that role
- **THEN** the engine SHALL preserve every original source, permit, lifetime, Native stamp and exact checkpoint comparison
- **AND** an actual claimed continuation SHALL invalidate Current keys without changing Birth results
- **AND** no failure, eligibility or permission result SHALL be cached, and a failed claim SHALL preserve semantic/audio/ownership state even if numeric entries were warmed

#### Scenario: Bounded mapped reader reuses fully validated exact taps
- **WHEN** a mapped or phase chunk of at most1024 frames uses a fixed exact tap batch
- **THEN** it SHALL preserve the original guard/error order and exact position, seam, gain and interpolation results
- **AND** it SHALL validate every required left/right tap, channel and current/outgoing reader for the whole chunk before changing any output buffer
- **AND** scalar, zero-frame, longer and invalid-domain requests SHALL retain their prior path and behavior
- **AND** scratch SHALL remain bounded at32768 bytes with no callback allocation or persistent reader authority

## MODIFIED Requirements

### Requirement: Adapt a qualified all-dry mapped cohort through the existing global rate claim
A changed global speed/master-period tuple with active mapped voices MAY apply through the existing global rate coordinator only when every active participant is admitted to the supported dry cohort or the bounded productive Native continuation below. It SHALL capture and validate every participant's actual source/reader permit, placement or explicit retained-voice authority, generation, continuity and effective tuple, including each mapped voice's original qualified geometry. It SHALL NOT replace those owners with a later selected-map request. It SHALL require BPM Lock on or that actual mapped voice's frozen explicit global SYNC ON choice under add-global-tempo-sync-policy and a valid actual master period with supported whole-map local rates. The explicit choice SHALL NOT create a master tempo or overwrite requested BPM Lock. Legacy global-null policy SHALL retain the BPM Lock requirement. Dry mapped participants SHALL retain zero mapped pitch and Key Lock off. A productive mapped Native participant MAY instead use only the equal-current-endpoint, source-bound first-mix and private-owner continuation specified by enable-retained-mapped-rate-continuation. An already productive scalar Native participant MAY join that same direct claim only with its unchanged owned pitch/Key-Lock tuple, private actual-feed continuity and effective source contract, no new history work, and the existing scalar retained-rate preflight and claimed installation. Paused mapped voices, paused or Hold-active Native members, Native phase programs, unsupported transitions, unavailable readers, required new Native history and all other unqualified Native paths SHALL refuse the entire direct effect without partial mutation. Existing scalar-only history and qualified dry phase recovery behavior SHALL remain unchanged.

#### Scenario: Coupled fader changes a supported mixed cohort
- **WHEN** the existing public speed/master-period operation reaches a settled callback boundary with qualified mapped and scalar dry or explicitly admitted productive Native participants
- **THEN** the system SHALL preflight every mapped replacement, scalar retained-rate update and source participant before claiming the effect
- **AND** it SHALL install the global tuple, transport and mapped playback replacements through the existing atomic claim at that one actual boundary
- **AND** it SHALL retain original voice generations, map/source owners, launch targets, preceding content and every retained Native/FIFO owner

#### Scenario: A participant cannot join the direct effect
- **WHEN** a participant is stale, has unavailable reader coverage, requires an unsupported Native/transition/history path, is an excluded paused or Hold-active Native member, or any whole-map local rate is out of range
- **THEN** the system SHALL refuse the complete effect with a truthful unavailable result
- **AND** preceding audio, the effective global tuple, master timing and every participant SHALL remain intact
- **AND** it SHALL NOT clip individual map segments, silently select scalar playback or borrow a later source's admission
