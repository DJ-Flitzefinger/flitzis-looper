## ADDED Requirements

### Requirement: Source metadata and two numeric shifts remain separate
The system SHALL represent detected or corrected source key, numeric audible base_shift and numeric extra_shift as separate per-content state and SHALL derive base and result labels without conflating them.

#### Scenario: Base and additional zero use the prepared tuning
- **GIVEN** E minor source and base C minor with base_shift=-4
- **WHEN** extra_shift is0,+2 or+12
- **THEN** nominal result is C minor,D minor or C minor one octave higher respectively
- **AND** the base menu stays C minor; extra0 means neutral to base, not original file

### Requirement: Correction changes labels without changing sound
The system SHALL persist an independent source metadata correction and SHALL preserve numeric base/extra, playback position and accepted events when correction is set or removed.

Correction MAY fix major/minor metadata. It SHALL NOT retune/retrigger or recompute
already chosen numerical shifts. Remove correction, reset base0 and reset extra0
SHALL be three independent actions.

#### Scenario: Correct a wrong source label while playing
- **WHEN** a source correction is set or consciously removed on playing A
- **THEN** source/base/result labels update consistently without sound/cursor/retrigger change
- **AND** copies' corrections and both numeric shifts remain independent

### Requirement: Analysis epochs preserve newer correction intent
The system SHALL remove only a preexisting correction at successful deliberate new analysis admission and SHALL fence late analysis metadata by source/lifetime/analysis and correction epochs.

#### Scenario: New correction after analysis admission wins
- **GIVEN** old correction C1 and successful new analysis epochE
- **WHEN** C1 is removed at admission, C2 is set afterward and E's result arrives late
- **THEN** C2 remains authoritative and saved numeric shifts stay unchanged

#### Scenario: Failed analysis admission or project reopen
- **WHEN** analysis cannot be admitted or the same verified project reopens
- **THEN** existing correction and shifts remain; reopen is not a new analysis
- **AND** late results for removed/reassigned lifetimes cannot affect a replacement

#### Scenario: Real admission and metadata-only version publication
- **GIVEN** the current content instance and native waveform source generation, digest and shape
- **WHEN** deliberate analysis returns a real integer request ID after successful native admission
- **THEN** analysis epoch advances and only its preexisting correction is removed
- **AND** the result may update a source key version only while that captured content, native source, request and analysis epoch still match
- **AND** missing or boolean request IDs and duplicated, older or equal-byte replacement results grant no new key metadata authority
- **AND** a later correction is retained, independently of whether a newer timing edit has made the analysis timing projection stale

### Requirement: Durable key policy remains neutral before audible activation
The system SHALL keep persisted key correction, base, extra and retrigger policy separate from native audio until the required pitch application gates are implemented and accepted.

#### Scenario: Neutral policy edit during ordinary playback
- **WHEN** correction is changed or removed, absolute base intent is chosen, either numeric shift is reset or extra/retrigger intent changes in K-META
- **THEN** no native pitch, trigger, stop, cursor, Key Lock or timing command is issued
- **AND** immutable source versions and independent per-content settings persist without restoring live tokens

### Requirement: Absolute base key selection has deterministic octave and mode
The system SHALL set base_shift from the corrected known source root to a same-mode target using d=(target-source) mod12 and d>6 then d-=12, with a fixed +6 tie and no accumulation.

#### Scenario: Repeated absolute key selection is stable
- **WHEN** any of12 source roots selects any of12 same-mode targets repeatedly
- **THEN** base remains the same integer -5..+6 and extra remains unchanged
- **AND** a tritone always selects+6 without octave switching

#### Scenario: Unknown source or unreachable major minor conversion
- **WHEN** GUI or MIDI requests an absolute target without known/corrected source or with different mode
- **THEN** admission explicitly rejects that target without guessing or audible mode conversion
- **AND** relative semitone choices remain usable with unknown source metadata

### Requirement: Additional transposition sets every supported signed value
The system SHALL offer all37 desired fixed integer extra_shift values -18..+18 including0 without scale filtering or accumulation and SHALL validate the whole resulting base+extra against one shared supported domain.

#### Scenario: Repeated semitone and independent resets
- **WHEN** +2 is selected repeatedly or extra is reset0
- **THEN** extra remains+2 or becomes0 respectively without changing base or source correction
- **AND** base reset0 independently preserves extra and correction

### Requirement: One pitch route preserves tempo and explicit lock behavior
The system SHALL use one integrated pitch route with explicit lock behavior and proven support.

#### Scenario: One route retains the complete pitch and readiness contract
- **WHEN** transposition is prepared, admitted or applied
- **THEN** the following complete route, domain and failure contracts SHALL apply:

The system SHALL apply k=base_shift+extra_shift through one integrated pitch route while preserving source time, tempo, M/B/S/phi, loop/launch and all other pads.

The desired total SHALL cover -23..+24 plus actual r(n). Equal-rate varispeed route
SHALL use h=2^(k/12), lockON p=h/r and lockOFF p=h with existing additional speed
coupling r*h. Quality/RT/readiness/latency/finite-history/unity proof SHALL precede
support claims; current clamp/dry/finite API checks SHALL NOT certify support.
Concrete demonstrated limitations SHALL be explained and consistently rejected
by UI/MIDI/storage/audio instead of hidden clamping or tempo change. Transposition
SHALL NOT create complete source PCM or two serial pitch shifters.

#### Scenario: Extreme total shift under variable rate
- **WHEN** total k=-23 or+24 and actual accepted r(n) extrema are evaluated
- **THEN** B5 proves the combined range or records a concrete unsupported limit
- **AND** no partial retune, time change or accepted-looking clamp conceals failure

#### Scenario: Native pitch unity differs from source rate unity
- **WHEN** r=1 with k nonzero or h/r=1 with r nonunit
- **THEN** prepared processing/transition preserves intended pitch and matched output history/latency
- **AND** rate-only/dry bypass cannot silently omit transposition or reset voices

### Requirement: A live Key Lock change owns a prospective numeric tuple
For a live nonzero applied transposition, the system SHALL retain the previously
effective Key Lock mode and its productive Native/FIFO history until the original
mode request can atomically adopt source-specific prepared owners for every
participating voice at one exact output frame E. Preparation SHALL use each
voice's actual applied numeric pitch intent with the requested Key Lock mode,
and SHALL retain the original request, content/source generation, actual window,
accepted timing/ACK, voice generation and preparation epoch fences. An old
effective-mode candidate SHALL NOT certify the prospective tuple.

The system SHALL use its existing bounded source-preparation/recycle lanes and
render boundaries. Callback work SHALL NOT construct Native state, allocate
owners, wait, perform I/O, or partially change a pad when any participant fails
preflight. Preparation failure or unavailable history SHALL reject only that
request visibly, preserve the old effective mode, and leave worker retirement
and later requests usable. Existing strong Native history permits SHALL remain
required at actual adoption. A constant endpoint SHALL NOT stand in for missing
variable source history. This requirement does not claim unimplemented variable
history, paused endpoint, acoustic, device or full B5 acceptance.

#### Scenario: Complete source changes Lock in either direction with nonzero pitch
- **GIVEN** a playing complete source with positive or negative numeric transposition and actual r=1 or a nonunit rate
- **WHEN** its current own mode request changes Key Lock in either direction
- **THEN** real source preparation targets the prospective tuple while uninterrupted old-mode PCM remains effective before E
- **AND** all participating prepared Native/FIFO owners and the same own mode request adopt together at E without a source restart, numeric pitch change or tempo change
- **AND** the first resulting PCM uses the prospective tuple and its matched prepared history

#### Scenario: Finite window storage precedes the same own mode acknowledgement
- **GIVEN** a finite source whose nonzero live transposition needs a new Key Lock tuple
- **WHEN** the original mode-only resident transaction claims actual storage
- **THEN** its original ticket SHALL remain adopting and its resident marker SHALL remain owned while the old mode is audible
- **AND** preparation SHALL capture a fresh strong permit from the actual installed window rather than fabricate authority for a future window
- **AND** ordinary or GLOBAL starts depending on that ticket SHALL remain pending until that same request's complete prospective adoption at E
- **AND** only complete adoption may accept the original ticket and publish its own mode acknowledgement

#### Scenario: Stale or unavailable prospective work preserves old effective audio
- **WHEN** Stop, source replacement, window/timing/ACK change, an earlier pitch effect, a newer mode request or a changed participant invalidates prospective work
- **THEN** no participant SHALL consume that candidate or change the shared effective mode
- **AND** the original resident marker and owner SHALL settle exactly once through bounded off-thread retirement
- **AND** an unavailable real source-history trajectory SHALL remain a request-local visible refusal without terminating the preparation worker or certifying unsupported history

#### Scenario: Saturated retirement retains the original finite mode owner
- **GIVEN** an adopting resident mode transaction and zero available retirement queue or backlog slots
- **WHEN** its exact E, cancellation or a newer mode request is observed
- **THEN** the system SHALL retain the original transaction, PCM/stem pins and pending state without consuming, overwriting, forgetting or destroying that owner on the callback
- **AND** only available retirement capacity may settle that owner exactly once
- **AND** if E has already passed, later capacity SHALL permit refusal and retirement, never late mode adoption

#### Scenario: Refused mode reconciles its own already adopted physical window
- **GIVEN** a mode-only transaction whose real storage claim installed a different finite window or context before E
- **WHEN** prospective mode adoption is refused by Stop, timing, unavailable work or missed E
- **THEN** its original mode ticket SHALL remain rejected and dependent starts SHALL remain unable to launch
- **AND** control reconciliation SHALL preserve the actual claimed storage using only that original transaction's separate callback storage observation and current native content/source-generation/unique-window fences
- **AND** a fresh capture and later own mode request SHALL use that real window and remain functional without fabricating an accepted predecessor or replacement receipt
