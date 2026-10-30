## ADDED Requirements

### Requirement: One central three octave performance piano
The system SHALL provide one responsive three-octave piano above the central PAD grid, outside both sidebars. This requirement supersedes the older two-keyboard and popup-menu presentation. Opening the piano SHALL reserve approximately one responsive PAD row and reduce the grid below it; closing it SHALL restore that space. Its open state SHALL belong to the UI and remain stable across PAD changes. Notes, offsets, highlights and the two retrigger preferences SHALL remain independent per content.

The piano SHALL use actual white and overlaid shorter black key geometry with black-key hit priority, large pointer targets and all twelve chromatic pitches. The original source root SHALL appear in the middle octave. Each of the 36 displayed notes SHALL have its own signed octave-bearing identity; no modulo-twelve folding SHALL occur. Unknown source metadata SHALL retain explicitly relative semitone choices without inventing an original note. The source root is a key reference, not an estimate of the first melody note.

#### Scenario: A learned note lies beyond the displayed octaves
- **GIVEN** a valid exact piano shift outside the selected content's displayed three-octave range is chosen by a learned action, semitone step or restored state
- **WHEN** the piano renders, including after selecting another PAD and returning
- **THEN** it SHALL visibly identify the exact selected signed shift and, for a known source, its note and relative octave, together with the direction beyond the displayed range
- **AND** it SHALL retain the fixed 36-key geometry and exact chosen identity without highlighting an octave-folded substitute or claiming an absolute note for unknown metadata

#### Scenario: Repeated real mouse attacks and PAD changes
- **WHEN** a performer presses a white or black piano key, including the already selected key
- **THEN** that mouse-down SHALL submit one original SET or SET_RETRIGGER action through the existing content-bound controller, capture, worker and sole scheduler
- **AND** repeated equal notes SHALL be real repeated attacks, with fresh identities and timestamps
- **AND** PAD changes SHALL update the content's note, octave, offset and preferences without closing or flickering the piano or borrowing the previous PAD's settings
- **AND** admission failures SHALL be visible rather than silently represented as successful effective audio

### Requirement: Octave-bearing notes retain the supported total domain
The system SHALL retain legacy learned absolute-root actions with their existing nearest-root and fixed +6 tie semantics, while providing distinct octave-bearing piano actions. For a recognized root R, three C-based octaves indexed 0 through 35 SHALL map to base_shift=index-(12+R), putting the source root in the middle octave. The union of the new base domain SHALL be -23 through +23. Unknown-root relative choices SHALL use explicit signed values and no guessed note labels.

Additional transposition SHALL retain all 37 integer offsets -18 through +18. The shared validated combined intent SHALL remain -23 through +24, including the original -5..+6 base domain and every original base/extra combination as regression coverage. A new base/extra combination outside that total SHALL explicitly refuse without clamping or changing timing. Widening the numerical base representation SHALL NOT certify new DSP capability or close any existing quality, actual-r(n), finite-history, readiness, resource or Human gate.

#### Scenario: Octaves remain distinct and extreme combinations refuse
- **WHEN** three displayed C notes are chosen with known source C and offset zero
- **THEN** they SHALL retain base shifts -12, 0 and +12 as different saved and captured targets
- **AND** selecting base +23 with offset +18 SHALL refuse the unsupported total rather than silently accept +41 or clamp it
- **AND** legacy twelve-root actions and all original 37-offset/total -23..+24 cases SHALL retain their original meanings

### Requirement: Original Key and transposition reset have separate meanings
The system SHALL replace the Correction dropdown and both old sidebar keyboards with compact Piano, piano Retrigger and Original Key controls, and a separate transposition row ordered +1, Reset, -1 with a value and its own independent Retrigger preference. Half-step controls SHALL count every chromatic semitone; octave, note, reset and existing MIDI Note/CC/encoder Learn variants SHALL use the same semantic action pipeline and retain learned SET versus SET_RETRIGGER identity.

Original Key SHALL remove all pitch modifications in one original action: base_shift=0, extra_shift=0 and manual source-key correction removed, retaining the detected source reference. It SHALL preserve KEY LOCK, BPM, Learn bindings, unrelated PADs and the global piano open state. The transposition Reset SHALL change only extra_shift to zero, preserving the complete chosen piano note and octave and source metadata. Original Key SHALL not publish intermediate audible states or create multiple implicit retriggers.

#### Scenario: Independent reset paths
- **GIVEN** original source C, chosen piano D and offset +1
- **WHEN** transposition Reset is invoked
- **THEN** piano D SHALL remain selected with offset zero and unchanged correction metadata
- **WHEN** Original Key is invoked
- **THEN** piano C at the original octave, offset zero and no manual correction SHALL be the one resulting requested tuple
- **AND** an admission failure or stale content SHALL not separately clear another content's correction

### Requirement: Piano attacks preserve source timing and lock policy
Piano and transposition actions SHALL change only pitch intent h=2^(k/12), not actual source rate r, master BPM, beat grid, loop cycle or other PADs. KEY LOCK ON SHALL retain p=h/r and OFF p=h, with the audible OFF ratio r*h. The UI SHALL describe a key as nominal when speed changes its audible pitch, and SHALL not automatically enable KEY LOCK or change master tempo.

#### Scenario: Quantized retrigger and independent controls
- **WHEN** a piano or offset SET_RETRIGGER is captured under Quantize or SYNC
- **THEN** pitch adoption and restart at the regular loop beginning SHALL occur together at the existing original due frame
- **AND** audio before that frame SHALL retain the previous effective pitch without early retuning or a second scheduler
- **AND** the two GUI retrigger preferences SHALL remain independently persisted while existing learned variants keep their fixed behavior

#### Scenario: Piano retrigger during an unfinished accepted rate glide
- **GIVEN** an active PAD has a confirmed rate target while its actual source rate is still following the existing active-frame smoothing trajectory
- **WHEN** a piano SET_RETRIGGER prepares and takes effect before that trajectory reaches its target
- **THEN** the effect SHALL restart only the intended source phase at the normal loop beginning and SHALL preserve the actual rate, pending target and remaining active-frame smoothing interval at its original execution frame
- **AND** preparation and effect validation SHALL use the projected actual rate at that frame rather than substituting the bank's eventual rate target
- **AND** its own prepared Native and following PCM SHALL obey the existing Key Lock law on that preserved trajectory, with no early retuning, target-rate jump, peer restart or clock/master-period change
- **AND** unavailable or invalid original preparation SHALL retain the existing whole-action refusal and original T/E policy rather than partially applying pitch or restart
- **AND** an instantaneous ON pitch scale of one during an unfinished glide SHALL NOT certify a dry future trajectory; OFF with neutral pitch MAY retain its existing dry path while preserving the same source-rate clock
- **AND** unavailable reset-phase history SHALL NOT be replaced by constant history at the bank target or by relabelling another phase's prepared FIFO, and the original execution frame SHALL NOT be delayed merely to wait for the rate glide to settle
- **AND** a ready cached endpoint for the same note at the eventual target SHALL NOT qualify as ready for a different actual rate or smoothing interval, consume that unrelated prepared owner or shorten the existing preparation execution policy

#### Scenario: A complete Normal retrigger prepares its own reset phase during a glide
- **GIVEN** a running complete FullMix NormalLoop source has unchanged source, accepted timing and loop domain, and retained genuine rate epochs covering the required preparation
- **WHEN** its original wet SET_RETRIGGER is captured during the accepted rate glide or has E inside it, including a glide that settles between F and E
- **THEN** the existing worker SHALL prepare a new Native owner for the Normal beginning using a separately declared reset-phase history, preserving the original rate epochs and E
- **AND** the unmodified actual journal and frozen F-to-E projection SHALL first match their original nonreset endpoint exactly, and that endpoint's prescribed Normal reset SHALL exactly match the action's frozen reset endpoint
- **AND** derived Normal read phases SHALL translate the original epoch origins by one common phase shift modulo the actual physical or musical loop period, without replacing the rate, target or smoothing interval or relabelling actual past reads
- **AND** the new owner's preparation origin SHALL be E minus a whole Native-block count covering the maximum required pitch-dependent H across all included epochs, with its cold profile chosen from the epoch completing its first real input block
- **AND** every derived tap SHALL have current complete-source coverage, and the actual-history, source, voice-generation, timing and action guards SHALL remain valid before preparation, after replay and at the unchanged E
- **AND** preparation SHALL restore the exact frozen reset endpoint after its declared pre-E replay without weakening strict live adoption comparisons or claiming continuous-from-birth equivalence
- **AND** the first adopted sample, following FIFO blocks and remaining actual glide SHALL match independent original-PCM, rate-epoch, direct-Native and continuous-filter references while peers and master timing continue unchanged
- **AND** missing, gapped, evicted or insufficient epochs, a changed domain or source, paused projection, Intro/Tail, unsupported stem history or transitions, late work or unavailable custody SHALL retain whole-action refusal rather than fabricate history or delay E

#### Scenario: A stable complete selected StemSet retriggers during a glide
- **GIVEN** a running NormalLoop source has one current fully held source-bound StemSet, an unchanged component mask and accepted timing, no active selection transition, and sufficient genuine rate epochs using that exact physical owner and selection
- **WHEN** a piano or learned MIDI wet SET_RETRIGGER is captured while its actual rate is gliding and its original E remains inside that glide
- **THEN** the existing capture, worker, action permit and sole scheduler SHALL prepare and adopt that action's own Native at E using the same declared reset-phase history and unchanged actual rate, target and remaining smoothing interval
- **AND** original and shifted reads SHALL resolve every required component against the actual retained complete-set, publication and PCM identities, with current publication authority and complete physical source and component coverage
- **AND** source addresses and copied immutable identities SHALL NOT manufacture ownership, borrow a foreign or replacement publication, reinterpret FullMix as the selected signal, or replace the original actual journal
- **AND** the first adopted sample, following blocks, selected component sum and continuous filters SHALL match independent component-PCM, scalar rate-epoch and direct-Native references while the audible FullMix peer and master timing remain unchanged
- **AND** missing or foreign pins, invalidated publication, selection or source ABA, changed timing/domain, insufficient selected history, finite component windows without the acknowledged whole-loop proof below and active transitions SHALL retain whole-action refusal without partial pitch/restart or delayed E
- **AND** a fresh request after valid current ownership and sufficient selected history are established SHALL remain usable on the existing worker; this bounded complete stable-selection behavior SHALL NOT certify active transitions, finite readiness, continuous-from-birth equivalence, acoustic quality, performer deadlines or Human acceptance

#### Scenario: An acknowledged finite Normal window retains the original glide attack
- **GIVEN** an original source's own finite residency request has received its genuine current Native WindowACK before capture F, and its held physical source and component views cover the entire Normal-loop tap union including both seam taps
- **AND** its source identity, resident bindings, accepted timing, loop domain, stable selection and every required current StemSet publication remain unchanged through the original E with no active selection transition
- **WHEN** a piano or learned MIDI wet SET_RETRIGGER is captured during an actual-rate glide with sufficient genuine own history and E inside that glide
- **THEN** the existing worker SHALL prepare and adopt that action's own Native at E using the existing separately declared reset-phase history, without requiring unrelated source frames outside the proved Normal-loop union or replacing the original actual journal
- **AND** every original and phase-shifted history read SHALL retain exact source/window/reader identities and current authority, and SHALL stay within the actual held all-phase physical coverage with no missing or fabricated seam taps
- **AND** the finite request SHALL retain its own mandatory NormalLoopCoverage and exact source, ResidentBinding, WindowACK, timing and action guards before preparation, after replay and at the final effect claim
- **AND** preparation SHALL retain the original pitch-dependent history requirement and first-block profile on the actual rate epochs, and SHALL preserve original T/E, reset phase, actual rate, target and remaining active-frame smoothing interval
- **AND** first and subsequent output, Native/FIFO and filters SHALL match independent complete-original-PCM and scalar-rate references while peers and master timing continue unchanged
- **AND** missing, foreign, stale or wrong-window ACK, changed physical views or selection, unavailable seam coverage or history, invalid publication or late work SHALL refuse the whole original action rather than expand an unproved window, delay E or partially apply pitch/restart
- **AND** a fresh valid acknowledged request SHALL remain usable on the same existing worker; this bounded unchanged-window behavior SHALL NOT certify finite window handover, active transitions, continuous-from-birth equivalence, acoustic quality, performer deadlines, resource or Human acceptance

#### Scenario: A lower total piano and offset retrigger keeps the original glide
- **GIVEN** a supported complete FullMix source has an independently chosen transposition offset of -18 semitones established through ordinary stopped SET controls, and its playing voice has sufficient genuine source history for the pitch-dependent reset requirement
- **WHEN** a real piano note of -5 semitones or its learned MIDI SET_RETRIGGER is captured during an accepted actual-rate glide with its original E inside that glide
- **THEN** the captured action SHALL retain the distinct piano note -5 and offset -18, resolving total -23 without replacing the offset, folding the octave or borrowing another selected pad's content
- **AND** the action SHALL use its own existing worker, current history and Native authority to apply pitch plus one retrigger at its unchanged original E; a later pad selection SHALL NOT redirect it
- **AND** the reset history SHALL retain h=2^(-23/12), actual r(n), OFF p=h and ON p=h/r(n), the independently required maximum H and first-block cold profile without substituting the bank target for the actual glide
- **AND** the first adopted sample, continuing PCM, FIFO and filters SHALL match independent original-PCM, scalar-rate and direct-Native references while peer and master chronology remain unchanged
- **AND** this supported-domain interaction SHALL NOT certify the full domain matrix, finite handover, active selection transitions, continuous-from-birth equivalence, acoustic quality, Native allocation/logging, deadlines, resources or Human acceptance

#### Scenario: An upper total piano and offset retrigger retains its cold profile
- **GIVEN** a supported complete FullMix source has offset +18 established through ordinary stopped SET controls, and its genuine initial piano note -12 produces total +6 with independently verified pitch-dependent birth preparation
- **AND** sufficient genuine post-birth source history has been rendered for the independently required maximum reset H; birth preroll SHALL NOT count as actual voice history
- **WHEN** a real piano note +6 or its learned MIDI SET_RETRIGGER is captured during an accepted .5-to-2 actual-rate glide with its original E inside the glide
- **THEN** the captured action SHALL retain piano note +6 and offset +18 as distinct intent, resolving supported total +24 without treating +24 as a standalone piano-note representation
- **AND** its own existing worker and Native request SHALL retain the actual rate epochs, ON p=4/r(n) and OFF p=4, maximum included H and exact first-block cold profile, including ON p=8 during the earlier .5-rate history
- **AND** pitch plus one retrigger SHALL apply at the unchanged original E with independent first/continuing PCM, Native/FIFO/filter and scalar references while peer/master chronology continues unchanged and a later pad selection does not redirect the captured content
- **AND** unavailable genuine history or authority SHALL retain existing whole-action refusal rather than relabelling a birth preroll, using a bank-target cold profile or moving E
- **AND** this bounded upper-domain interaction SHALL NOT certify the full domain/rate matrix, finite handover, active selection transitions, continuous-from-birth equivalence, acoustic quality, Native allocation/logging, deadlines, resources or Human acceptance

### Requirement: Learned pitch steps distinguish CC buttons and encoder encoding
The system SHALL keep Note and keyboard step actions as explicit signed attacks and SHALL treat a newly learned fixed step on a MIDI CC/NRPN button as one high-edge attack, ignoring release and repeated held-high values. The compact MIDI controls SHALL expose an explicit encoding preference for newly learned CC/NRPN step bindings: button, absolute encoder, two's complement encoder or offset-binary encoder. The saved semantic action SHALL retain that encoding; changing the UI preference SHALL not reinterpret existing bindings. Existing fixed-note/base/extra bindings SHALL retain their established semantics.

The new octave-bearing piano-note and Original Key actions SHALL likewise use one high edge when learned to a CC/NRPN button, and SHALL retain repeated genuine NoteOn attacks. Legacy base/extra/base-reset CC actions SHALL retain their established behavior. Encoders SHALL use the explicitly encoded step actions rather than infer a direction from a fixed piano-note target.

Encoder resolution SHALL happen at the original Native input capture, retaining original content, timestamp and variant through the existing scheduler. Absolute encoding SHALL initialize on its first value, ignore equal values and resolve later modular 7-bit movement by direction. Two's complement SHALL use 1..63 as positive and 65..127 as negative; offset binary SHALL use 65..127 as positive and 1..63 as negative. Zero and 64 SHALL be neutral for both relative encodings. Each signed message SHALL apply one chosen semitone or octave step, with negative learned steps inverting direction. Capture state SHALL be scoped to the current learned mapping owner and retained off realtime.

NRPN Data Entry SHALL support these same seven-bit button/encoder values under
the selected full parameter identity, consuming the optional low byte without
another attack. This SHALL remain distinct from fourteen-bit value resolution.
Existing NRPN Increment/Decrement SHALL retain Looper's normalized values 65/63
and the matching offset-binary step encoding, without rewriting saved mappings
or guessing another encoding.

#### Scenario: Encoder reversal and button release preserve exact step count
- **WHEN** a two's complement encoder learned to offset +1 sends 1 and then 127
- **THEN** the offset SHALL advance one semitone and return one semitone, retaining separate original capture identities
- **WHEN** a button learned to offset +1 sends 127, repeated 127 and release 0
- **THEN** exactly one offset action SHALL be captured
- **AND** a subsequent new press SHALL remain usable without an extra release attack

#### Scenario: Restored encoder Learn retains original productive ownership
- **GIVEN** Learn has saved separate piano SET and transposition SET_RETRIGGER CC mappings with explicit encodings and a new controller reloads that mapping file
- **WHEN** later checkbox and encoding preferences differ and Native dispatch selects A, receives the CC input and selects B before the controller polls
- **THEN** each admitted action SHALL retain its stored variant, encoding, original A content and input timestamp through preparation and the sole scheduler
- **AND** SET SHALL retune A without adding a restart while every genuine SET_RETRIGGER SHALL produce one own attack and selected numeric projection on A, leaving B unchanged
- **AND** absolute initialization/equal values, relative neutral values and button release/held repeats SHALL NOT create phantom captures or attacks
- **AND** productive PCM and actual effect receipts SHALL be checked separately from saved intent and the rendered piano selection

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

#### Scenario: Source metadata editing remains reachable beside the central piano
- **GIVEN** a living selected content has detected, unknown or manually corrected source-key metadata
- **WHEN** the user opens the compact Source-row editor and chooses a major/minor correction or independently restores detected metadata
- **THEN** the rendered controls SHALL use the existing metadata-only set/clear path and persist the correction independently of the piano note, offset and both Retrigger preferences
- **AND** the edit SHALL NOT capture or publish pitch audio work, move playback, change master timing or alter any already accepted action's original tuple, timestamp, content or T/E
- **AND** a selection or content replacement while the editor is open SHALL NOT apply its stale choice to another content
- **AND** an existing edit reservation or exhausted metadata epoch SHALL refuse the edit without changing intent or audio, keep the editor available and display the reason rather than escaping through the render loop
- **AND** the editor SHALL preserve the central piano, Piano toggle, independent offset Reset and all-modification Original Key semantics without restoring the removed Correction dropdown

### Requirement: Analysis epochs preserve newer correction intent
The system SHALL capture deliberate analysis admission without removing correction.
Only matching successful key-component completion SHALL remove its preexisting
correction and publish the detected key version. Skipped, failed or unrequested
key work SHALL preserve prior key intent while independently valid BPM remains
publishable. Late key metadata SHALL remain fenced by source/lifetime/request,
analysis and correction epochs.

#### Scenario: New correction after analysis admission wins
- **GIVEN** old correction C1 and successful new analysis epochE
- **WHEN** C2 is set after admission and E's successful key result arrives late
- **THEN** C2 remains authoritative and saved numeric shifts stay unchanged

#### Scenario: Failed analysis admission or project reopen
- **WHEN** analysis cannot be admitted or the same verified project reopens
- **THEN** existing correction and shifts remain; reopen is not a new analysis
- **AND** late results for removed/reassigned lifetimes cannot affect a replacement

#### Scenario: Real admission and metadata-only version publication
- **GIVEN** the current content instance and native waveform source generation, digest and shape
- **WHEN** deliberate analysis returns a real integer request ID after successful native admission
- **THEN** analysis epoch advances without removing its preexisting correction
- **AND** only matching successful key completion may remove that captured correction
- **AND** skipped, failed or unrequested key work preserves detected key, correction and shifts
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

#### Scenario: Stereo formats preserve original selected history through extreme SET
- **GIVEN** an actual stereo 44.1 or 96 kHz source with distinct left and right content, a genuinely held stem selection and real off-grid source-rate history reaching both 0.5 and 2
- **WHEN** an original SET to total -23 or +24 prepares under either Key Lock mode while selection and rate transitions are active
- **THEN** mixer format, source and stem format, output-clock beat geometry and Native channel grouping SHALL agree at the original quantized T/E
- **AND** the adopted output on each channel SHALL preserve that source's genuine required past and frozen future, with its original content, action, voice generation, source phase and FIFO ownership
- **AND** a mono comparison, a 48 kHz history-window constant at 96 kHz or repeating FullMix through a required stem past SHALL NOT certify that stereo history
- **AND** bounded-reset software agreement SHALL NOT replace the required continuous, acoustic, latency, resource or Human acceptance gates

#### Scenario: Native pitch unity differs from source rate unity
- **WHEN** r=1 with k nonzero or h/r=1 with r nonunit
- **THEN** prepared processing/transition preserves intended pitch and matched output history/latency
- **AND** rate-only/dry bypass cannot silently omit transposition or reset voices

#### Scenario: Stem-mode acknowledgement shares the actual pitch processing decision
- **GIVEN** a live stem-mode handover on a voice with an applied numeric piano intent
- **WHEN** its mode receipt is finalized
- **THEN** required wet processing SHALL follow p=h/r with KEY LOCK ON or p=h with KEY LOCK OFF and the same matching Native-history retention decision used by rendering
- **AND** nonneutral piano pitch at r=1 SHALL await that selection's own current-source wet output under either lock mode
- **AND** a fresh +12/r=2 ON voice with no matching Native history SHALL remain dry at p=1, while an already productive matching Native owner SHALL retain aligned wet history and require its own current-source output

#### Scenario: A dry native-unity birth later requires wet processing
- **GIVEN** an original accepted pitch attack starts at nonunit source rate with h/r=1 and no source-specific Native candidate
- **WHEN** a subsequent accepted rate transition requires nonunity Native pitch
- **THEN** the first emitted wet output SHALL use valid history belonging to that original source and voice while preserving intended pitch, source time and voice generation
- **AND** a generic warmed Native's startup FIFO or readiness flag SHALL NOT certify matched source-specific history
- **AND** unavailable preparation SHALL remain an explicit supported pending or refusal condition rather than an accepted-looking partial retune or hidden source-clock reset

### Requirement: A rate edit cannot expose an unowned wet entry
The system SHALL guard a global rate edit that moves an active native-unity voice
into wet processing without established source-owned Native history, retaining the
previous effective speed, master period, transport and productive output until
one original coalesced rate intent can commit at one exact existing render boundary
E. Healthy free native-unity births SHALL remain immediate. The rate intent SHALL
retain its original stamped identity separately from the settled global timing
fence captured after the actual command and parameter drains. A partially drained
prefix SHALL NOT borrow a later intent's identity or certify its application.

Each drain's settled-fence observation SHALL begin before its first queue read
with a stable revision and publisher-clear observation, and SHALL remain valid
through the completed empty drain. An end-only observation SHALL NOT acknowledge
a publication that entered the queue after the callback observed it empty.

The original rate payload SHALL retain exactly the fields changed by that control
action. The callback SHALL resolve those fields through its one coalesced rate
intent before freezing the complete pending tuple. A speed-only edit SHALL NOT
restore a master period cleared or replaced by an intervening authoritative
control. Superseded preparation SHALL NOT erase the preceding rate intent needed
to resolve a later original master-period-only edit from the same mailbox.

Preparation SHALL reuse the existing per-voice history workers and bounded lanes
to prepare endangered voices for their old effective source trajectory at E,
including genuine dry reads and frozen F-to-E chronology. At E the complete
unchanged participant set, source/content/voice/tuple/paused guards, own preparation
epochs, exact prepared endpoints and retirement capacity SHALL pass before any
mutation. The existing complete ownership claim SHALL adopt the required complete
Native/FIFO owners and apply speed, master period and transport together. Already
productive source-owned Native voices SHALL retain their own history. Ordinary
source-rate smoothing SHALL begin from that boundary; no second scheduler, output
bridge, callback Native preparation or invented constant past is permitted.

#### Scenario: Pending rate preparation preserves confirmed audio
- **WHEN** an accepted speed or coupled master-period edit needs an endangered voice's first source-owned wet history
- **THEN** output before E SHALL retain its previous effective rate, PCM, source trajectory, voice generation and original pitch action
- **AND** status beside the rate control SHALL distinguish requested settings from callback-confirmed effective settings and queued, preparing, applied or rejected state
- **AND** the first wet output after the complete effect SHALL have matched declared history rather than generic startup silence

#### Scenario: Public rate controls compose with original piano and MIDI actions
- **GIVEN** independently productive voices have current accepted source timing and original piano or routed MIDI pitch actions at nonunit source rates
- **WHEN** the existing public global controller requests a speed and coupled master-period change through the sole rate coordinator
- **THEN** requested and confirmed settings SHALL remain distinct until the actual complete effect, and each voice SHALL retain its own source, pitch intent and transport trajectory under the accepted timing
- **AND** KEY LOCK ON SHALL use h/r and OFF SHALL use h at each voice's actual accepted rate rather than a fixture's nominal speed
- **AND** an original pitch action whose prepared future is invalidated by an earlier effective rate change SHALL refuse wholly at its unchanged original T/E without an attack or rollback of the confirmed global tuple
- **AND** a later fresh piano or MIDI action SHALL use its own current capture and prepared ownership, preserving the confirmed source/master timing and unaffected voices
- **AND** software PCM and controller evidence SHALL NOT replace general continuous-O, full acoustic, resource, device or Human acceptance

#### Scenario: A publisher finishing after an empty observation remains unread
- **GIVEN** a global publisher has raised revision N but has not yet queued its original payload
- **WHEN** a callback observes the queue empty and the publisher queues that payload and finishes before drain finalization
- **THEN** the drain SHALL NOT acknowledge N or let another empty drain certify the combined timing fence while that original payload remains unread
- **AND** a later genuine drain SHALL consume that same payload before a fresh stable fence can authorize rate preparation or adoption

#### Scenario: A pending rate edit cannot partially affect participants
- **WHEN** supersession, Stop, restart, source/lifetime/authority/window/timing/selection change, Pause/Resume, participant birth/removal, insufficient history or missed readiness invalidates an original pending rate edit
- **THEN** that edit SHALL visibly refuse as a whole, retaining the previously effective global tuple and unrelated accepted actions
- **AND** its own stale work SHALL retire through existing worker lanes while a later fresh request remains usable
- **AND** another global control that changes the authoritative rate SHALL use the same pending effect or explicitly refuse while it is pending

### Requirement: A live Key Lock change owns a prospective numeric tuple
For a live nonzero applied transposition, the system SHALL retain the previously
effective Key Lock mode and its productive Native/FIFO history until the original
mode request can atomically adopt source-specific prepared owners for every
participating voice at one exact output frame E, or perform the guarded
numerically identical continuous handover below. Preparation SHALL use each
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
variable source history. The bounded history contract below does not certify
acoustic, device or full B5 acceptance.

For a running complete FullMix source with unchanged nonzero numeric pitch,
applied and requested source rate exactly one, no selection transition and an
already productive matching Native owner, a mode request MAY retain that owner
and its complete FIFOs instead of preparing an identical pitch again. This
exception SHALL preserve the original request and E, and SHALL claim all voices
atomically only after fresh source/window/timing permits, original participant
and Native identities, preparation epochs, exact projected source phase and
unchanged source-control continuity have been revalidated. A changed and restored
rate or selection SHALL NOT bypass that continuity fence.

The original Native request and its historical mode SHALL remain immutable. A
separate effective mode association SHALL govern subsequent productive matching
and actual rate changes: ON uses h/r(n), OFF uses h, on the retained owner while
its source continuity remains valid. This association SHALL NOT authorize a
pending candidate or replace fresh source authority. Ineligible voices SHALL
retain the existing prospective preparation contract; failed validation of an
already captured continuous handover SHALL refuse the whole original request
without resetting productive Native history. This exception SHALL NOT certify
general wet/dry alignment, acoustic quality or performer deadlines.

Prospective mode preparation SHALL reuse the owning voice's genuine bounded
source-read chronology before F and frozen F-to-original-E projection under the
same declared O/A replay contract as a moving SET. Each replay span SHALL use the
actual applied numeric pitch with the requested mode and its actual source rate.
A settled endpoint SHALL NOT erase earlier rate, Intro/Tail or selection history.
The same held-selection chronology contract below SHALL apply to prospective
mode and nonstarting SET preparation.

Each source lane SHALL acquire already allocated reusable snapshot storage before
capture; exhausted storage SHALL cause local refusal without callback allocation
or owner destruction. The worker SHALL clear and return snapshot owners through
bounded recycling on successful, stale and unavailable-history paths. A small
guard captured from the genuine snapshot SHALL retain its SOURCE-trajectory and
voice-generation fences with the candidate through adoption even after the large
snapshot has been recycled. That guard SHALL only narrow a fresh strong Native
permit, never replace source/window/timing authority. Its later invalidation
SHALL NOT by itself revoke already productive Native history.

For a participant paused at F, prospective mode preparation SHALL retain the
same original global E while projecting zero additional active source frames
between F and E. The requested tuple SHALL replay only the genuine retained
active chronology to that frozen endpoint. The callback SHALL retain its own
prepared candidate until the all-participant claim at E without consuming its
Native input/output FIFOs, source smoothing clock or filter history during pause.
Pause or Resume before E SHALL fence the original pending request; Resume after
successful adoption SHALL retain the adopted Native/FIFO owner and continue from
the frozen source position. Armed readiness while paused SHALL remain distinct
from that voice's first actual successful Native output after Resume.

#### Scenario: Complete source changes Lock in either direction with nonzero pitch
- **GIVEN** a playing complete source with positive or negative numeric transposition and actual r=1 or a nonunit rate
- **WHEN** its current own mode request changes Key Lock in either direction
- **THEN** real source preparation targets the prospective tuple unless every participant satisfies the guarded numerically identical continuous handover, while uninterrupted old-mode PCM remains effective before E
- **AND** all participating prepared or qualified retained Native/FIFO owners and the same own mode request commit together at E without a source restart, numeric pitch change or tempo change
- **AND** the first resulting PCM uses the prospective tuple and its matched prepared or qualified retained history

#### Scenario: Unity-rate mode change preserves the unchanged productive pitch history
- **GIVEN** every running participant is a current complete FullMix source with settled r=1 and an already productive nonzero pitch owner
- **WHEN** its original Key Lock request commits at its unchanged E with all continuous-handover guards current
- **THEN** every original Native/FIFO owner and every continued PCM sample SHALL remain unchanged relative to independent uninterrupted history, with its own mode acknowledgement first at E
- **AND** a later genuine rate change SHALL use the newly effective mode law on that retained owner rather than the immutable originating request's old mode
- **AND** stale source authority, changed source-control continuity, a changed participant or unavailable productive history SHALL NOT qualify as an equivalent scalar handover

#### Scenario: Concurrent identical-pitch mode requests keep separate voice claims
- **GIVEN** two complete sources have independent productive voices and pending guarded identical-pitch mode requests with different original E frames
- **WHEN** one pad's request is superseded before its E
- **THEN** only that original request's checkpoints SHALL be cleared, and the other pad SHALL retain its own original request and claim at its own E
- **AND** both voices SHALL preserve their Native owners, source chronology and continued PCM relative to uninterrupted independent history

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

#### Scenario: Prospective mode replays actual variable history at its original E
- **GIVEN** a live nonzero applied pitch with genuine off-block rate edits and an Intro or Tail passage inside its required pre-F history
- **WHEN** an ON or OFF request needs a Native preparation bound greater than E minus F
- **THEN** the existing worker SHALL replay the genuine retained past and original frozen future for the requested tuple
- **AND** every adopted PCM sample, source phase and FIFO count SHALL match an independent scalar/direct-Native oracle with the same declared A reset
- **AND** a backwards-constant reference SHALL disagree while old productive mode and owners remain effective until the unchanged original E

#### Scenario: Recycled snapshot leaves strict adoption fences intact
- **GIVEN** prepared prospective work whose large snapshot has already returned to its source lane
- **WHEN** a source control changes and returns to its former value, or a captured voice lifetime ends before E
- **THEN** the retained small guard SHALL refuse the whole tuple at adoption without changing old productive mode or partially swapping another participant
- **AND** a later original request on the same worker SHALL capture retained genuine history and remain usable
- **AND** a valid history guard SHALL NOT authorize expired or foreign Native source/window/timing ownership

#### Scenario: History storage saturation preserves every original owner
- **GIVEN** a source lane whose reusable history owner is in flight or awaiting bounded recycling
- **WHEN** another prospective mode request arrives or unavailable history returns
- **THEN** no callback SHALL allocate replacement snapshot storage, overwrite an owning request or destroy a filled snapshot
- **AND** bounded worker retirement SHALL return that same reusable owner and leave a later request functional

#### Scenario: A paused participant changes nonzero-pitch mode without invented reads
- **GIVEN** a publicly started voice with positive or negative applied pitch is paused with its own source position, active clock and filter history retained
- **WHEN** its addressed ON or OFF request is prepared while global output advances from F to the original E
- **THEN** the actual frozen endpoint SHALL be claimed at E through the same own mode/window transaction, with no START, STOP, retrigger or source-frame advance
- **AND** its prepared Native/FIFO candidate SHALL survive intervening paused callbacks while other running voices continue independently
- **AND** every sample after Resume, source phase and FIFO count SHALL match an independent direct-Native reference with the same declared A reset and zero paused active-frame projection
- **AND** successful paused ON SHALL report armed readiness while first live wet success or failure remains owned by that voice after Resume

#### Scenario: Paused request cancellation and mixed global targets keep independent owners
- **GIVEN** a global request addresses inherited running and paused pads while explicit ON and OFF peers retain their overrides
- **WHEN** one original paused request is superseded, stopped or resumed before its original E, or bounded retirement is temporarily unavailable
- **THEN** that target SHALL preserve its previous effective tuple and original request owner until its own safe completion or visible refusal
- **AND** another inherited target SHALL remain independently eligible for its own E and acknowledgement
- **AND** a later paused request on the same worker SHALL remain usable without borrowing another target's receipt or refreshing the refused E

#### Scenario: Nonneutral GLOBAL targets own their first adopted and resumed PCM
- **GIVEN** independently audible inherited positive and negative transpositions, one running and one paused, alongside explicit ON and OFF peers
- **WHEN** the public GLOBAL controller changes Lock in either direction through separate current resident transactions with distinct own E frames
- **THEN** each target SHALL retain its old effective PCM before its own E and its own prospective Native/FIFO from that E, without a pitch, source-clock or filter restart
- **AND** its first adopted or resumed sample and following Native blocks SHALL match independent original-source/direct-Native/filter chronology for the declared preparation origin and actual pitch-dependent history bound
- **AND** paused output SHALL consume no source, Native FIFO or filter state, while each explicit peer continues its own uninterrupted PCM and retains its override and acknowledgement

#### Scenario: A prepared ordinary birth owns its first live wet acknowledgement
- **GIVEN** a tracked zero-pitch KEY LOCK ON voice previously emitted confirmed wet output and was stopped
- **WHEN** a new addressed or global ordinary start adopts its own prepared Native/FIFO at its original E
- **THEN** the new voice SHALL rearm the existing first-live-wet guard independently of the predecessor's completed readiness
- **AND** preparation alone SHALL NOT report successful live wet output for that new voice
- **AND** failure before its first actual output SHALL retain the declared dry source trajectory and report its own effective OFF/error without rewriting inherited or explicit intent
- **AND** successful first live output SHALL retain the exact adopted Native/FIFO owner, original source phase and acknowledgement
- **AND** this birth guard SHALL NOT adopt a later requested mode or borrow that later mode's completion

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

### Requirement: A moving SET retains the genuine bounded source-feed chronology
A nonstarting SET that needs source history before its capture frame F SHALL use
the owning voice's actual successful source reads, including dry reads, followed
by the frozen deterministic projection from F to its original execution frame E.
Each retained span SHALL preserve the pre-read source clock, physical read plan,
rate epoch and source/selection context. A settled endpoint rate SHALL NOT replace
earlier rate changes, smoothing, Intro/Tail reads or loop-domain changes. Pause
SHALL consume no active-feed frames; refused reads SHALL create an explicit gap.
Start, restart, seek and Stop SHALL fence the retained chronology.

For a nonstarting SET captured while its voice is paused at F, the original
global execution frame E SHALL remain unchanged. Its active-feed endpoint SHALL
remain at F and its frozen F-to-E future SHALL contain zero active source reads.
The existing action-owned snapshot and strong action permit SHALL retain the
genuine required past, original receipt and voice owner through adoption at E.
Preparation and adoption while paused SHALL NOT consume source smoothing, filter
history or Native FIFOs. Resume after adoption SHALL continue with that exact
prepared Native/FIFO owner from the frozen active endpoint.

The callback SHALL record and snapshot only bounded scalar spans in storage
allocated before realtime use. An accepted action SHALL retain its own immutable
snapshot, voice generation, continuity and source-trajectory fences; its worker
SHALL NOT read mutable callback history. Historical domain/context markers SHALL
describe data only, never replace fresh strong Native adoption authority. Every
historical read SHALL remain backed by actually retained PCM/stem pins and its
physical coverage; missing contexts, gaps, retention overflow and stale owners
SHALL produce request-local refusal while leaving productive audio and the same
worker usable.

Source/rate/domain/selection changes and Pause/Resume after F SHALL invalidate the
captured future even if controls return to the same endpoint before E. This fence
SHALL preserve genuine retained past for a later action. Source-neutral DSP
preparation for a later prospective Key Lock request SHALL NOT invalidate an
earlier SET's independent source trajectory or original execution frame.

Historical stem descriptors SHALL retain copied complete-PCM identity values and
their original publication epoch owner/value in addition to allocation markers;
reused addresses SHALL NOT substitute another PCM or publication generation.

For a settled original E, replay SHALL support a preceding selection ramp whose
required stem readers belong to one actually held committed StemSet, including
a ramp still active at capture F that deterministically completes by E. The
request SHALL retain that physical history owner even when E uses FullMix and
requires no endpoint stem reader. One pin MAY fulfill both reader roles only
when each required role independently matches that pin's complete PCM identity,
source, physical backings and original publication generation.

The worker SHALL resolve the current and outgoing roles required by each sliced
read plan against real retained owners before validating coverage or reading.
An empty selected stem mask SHALL retain its owner requirement. Only roles no
longer required by that actual sliced plan MAY be omitted; an unsuccessful
lookup SHALL NOT erase a required role. Frozen F-to-E projection SHALL preserve
the actual reader roles at F and their deterministic retirement at ramp completion.
Endpoint/adoption comparisons SHALL use E's required readers independently of
the retained history pin, while retaining all physical identity, timing, window
and publication checks for readers that remain required. Historical pins SHALL
retire through the existing worker-owned bounded retirement path.

An already productive Native owner SHALL survive a storage-only WindowACK with
the same actual source, accepted timing and committed publication generation
when its current physical coverage remains valid. Its stem publication's old
capture-window acknowledgement SHALL NOT replace the existing productive source
and timing authority check. Fresh preparation and adoption SHALL retain their
strict current-window gates; a changed actual held publication identity or a
source/timing/voice revocation through the owning Native authority SHALL remain
invalidation reasons.
A fresh Native request on the refreshed physical view MAY reuse the already
admitted stem publication stamp; its own fresh Native permit SHALL prove the
current source, timing, window and acknowledgement. Initial stem publication
and admission SHALL retain their existing strict publication-window checks.
For already admitted Mixer-owned data, copied publication owner/expected value
SHALL identify its original data provenance. Mutable operation/receipt status and
intent counters SHALL NOT replace that data identity or the owning Native
authority. Withdrawing or superseding an old selection receipt SHALL NOT revoke
unchanged admitted PCM retained by a valid ordinary start or productive voice.
An independent selection operation reusing an already admitted physical set
SHALL preserve that set's original data-publication stamp. Its new receipt SHALL
retain its own separate operation/admission authority rather than restamping the
unchanged data and invalidating productive Native history.
Reader reconstruction from a shared cache without an admitted Mixer StemSet
SHALL take the new operation's admitted data-publication stamp; an unbound cache
placeholder SHALL NOT be promoted to admitted provenance by retaining it.

This bounded contract MAY admit a selection transition still active at the
unchanged E only through genuine source-feed replay, when one actually held
StemSet proves every required incoming and outgoing logical reader. FullMix and
empty-mask roles SHALL NOT erase a required stem owner. Exact adoption SHALL
retain the original phase, ramp gains/from/duration, source/timing/window authority
and strict physical backing checks for every required role. History-free or
backward constant reconstruction SHALL NOT authorize an active-E endpoint.

An adopted active-E Native owner SHALL carry a bounded scalar expected-transition
state. Before each productive source chunk the actual ramp SHALL match that
state exactly; only the same successful source consumption may advance it by
the actual source-frame progress. Deterministic completion MAY remove an outgoing
reader dependency without replacing/resetting Native, FIFO or filter state.
The completed tracker SHALL remain owned, so premature clearing, wrong progress,
changed from/duration and a later identical restarted ramp SHALL NOT borrow its
old proof. Pause SHALL contribute no source progress; Resume SHALL continue the
same prepared owner and remaining actual ramp.

Source-selection transition state and every rendered ramp gain SHALL be invariant
under callback, snapshot-slice and worker chunk partitions of the same actual
output-frame/rate chronology. Each constant applied-rate epoch SHALL retain one
canonical source-progress anchor and integer output-frame count; only an actual
rate change may reanchor that epoch. Zero-frame calls SHALL contribute no progress
or epoch change. Exact endpoint comparison SHALL retain this canonical state
without adding a numerical tolerance to prepared adoption.

Unheld historical readers or a second distinct StemSet without its own physical
owner proof SHALL remain request-local refusal with the old productive tuple
retained and a later valid request on the same worker usable. This extension SHALL
NOT certify continuous-O equality, acoustic quality or a performer deadline.
Existing constant endpoint priming MAY retain deliberate pre-origin start
semantics only after its whole required actual interval proves the same
rate/read/selection contract without a gap. A settled source clock alone SHALL
NOT certify earlier stem selections, masks or outgoing readers.

Fresh Native replay SHALL declare its active-feed origin O and reset origin A.
For the bounded variable suffix contract, A SHALL lie on O's Native input-block lattice
and precede the action's active-feed endpoint by at least the configured
preparation bound H. A paused endpoint remains at active F despite global E.
Alignment may add
up to one block minus one frames; the complete interval SHALL fit the explicit
retention budget, without clipping H. Native configuration and bound queries
SHALL run off realtime, with reset-dependent bounds queried after configuring the
actual encountered pitch. Replay SHALL preserve pitch changes at the spans that
complete Native input blocks, reset only before replay, and restore the exact E
source clock/read plan and adapter phase. Expanded-pitch cold initialization
SHALL use the first real completed block's pitch profile.

An independent A-reset PCM oracle SHALL certify only that declared finite replay
contract. A bounded suffix or matching FIFO counts SHALL NOT certify equality to
Native processing continuously from O or acoustic transition quality. Separate
continuous-reference diagnostics SHALL retain their actual results; the original
acoustic, performer-deadline and full B5/B6 acceptance gates SHALL remain required.

#### Scenario: A paused active selection endpoint adopts its own prepared tuple
- **GIVEN** a voice paused during a real same-held-set selection ramp with genuine retained source reads
- **WHEN** SET or a prospective Key Lock request prepares at its original E
- **THEN** both actual reader roles and the exact paused ramp SHALL be proved at adoption
- **AND** Resume SHALL retain that Native/FIFO/filter owner while consuming the remaining ramp and after its natural completion
- **AND** original T/E, source phase and transport SHALL remain unchanged

#### Scenario: Transition progress cannot reuse an earlier endpoint proof
- **WHEN** an active-E prepared owner observes premature clear, changed progress/from/duration or an identical restarted ramp after completion
- **THEN** that mismatched feed SHALL NOT count as successful productive use of the old proof
- **AND** exact adoption and current source/permit checks SHALL remain strict

#### Scenario: Callback partitions preserve the original active endpoint
- **GIVEN** a genuine active selection ramp at a fractional applied source rate
- **WHEN** the same F-to-E output interval is projected in one chunk and consumed in different callback partitions, including 17 versus 3 plus 14 frames
- **THEN** exact transition state and every rendered source sample SHALL agree at the original E and through the remaining ramp
- **AND** off-grid actual rate changes SHALL establish the same new epochs on live, sliced-history and worker paths
- **AND** zero-frame calls SHALL NOT alter that agreement or widen adoption authority

#### Scenario: Outgoing-only and empty-mask endpoints retain their owners
- **WHEN** active-E FullMix or an empty selected mask still requires a stem reader on either side
- **THEN** complete and finite paths SHALL prove every required physical role before adoption
- **AND** missing, foreign-generation, truncated or unheld data SHALL refuse locally without replacing the old effective tuple
- **AND** a later valid original request on the same worker SHALL remain usable

#### Scenario: Off-grid rate edits precede an expanded moving SET
- **GIVEN** a voice with actual rate edits away from Native block boundaries and an Intro or Tail passage into Normal inside its retained pre-F history
- **WHEN** a nonstarting expanded-pitch SET needs H greater than E minus F
- **THEN** its worker SHALL replay the genuine required past and original F-to-E projection with the prospective pitch/Lock tuple
- **AND** an independent scalar chronology and Raw Native with the declared reset origin SHALL agree on every adopted PCM sample, source phase and FIFO count
- **AND** a backwards-constant reference SHALL disagree, while a separately evaluated continuous-from-O reference SHALL retain its actual positive or negative result

#### Scenario: History refusal does not damage the next original action
- **WHEN** a required interval crosses a read gap, evicted history, unheld old stem context or a changed voice/source-trajectory fence
- **THEN** only that original action SHALL be visibly refused without changing its original T or adopting a replacement capture
- **AND** old productive audio SHALL remain owned and a later valid action on the same worker SHALL remain usable

#### Scenario: Paused SET replays genuine variable past and retains its original E
- **GIVEN** a publicly playing source whose retained active past contains off-grid rate smoothing and an Intro or Tail passage into Normal is paused at F
- **WHEN** a positive or negative nonstarting SET is prepared under either Key Lock mode for its original global E
- **THEN** its worker SHALL replay the actual required active past ending at F with zero active future, without inventing reads during the global wait
- **AND** adoption at E SHALL preserve the original action sequence, timestamp, T/E, content and voice generation without START, seek or source advance
- **AND** every resumed PCM sample, source phase and FIFO count SHALL match an independent direct-Native A-reset reference while retaining the same prepared owner and filter history

#### Scenario: A paused SET cannot borrow silent waiting time or a restored control state
- **GIVEN** an original paused SET with its own immutable history capture
- **WHEN** its genuine required past is insufficient, or Resume/Pause or source-rate changes after F restore the same endpoint before E
- **THEN** that original SET SHALL refuse locally without changing T/E or consuming the old productive owner
- **AND** silent global frames SHALL NOT supply missing active history, while a later original request with valid retained history SHALL remain usable on the same worker

#### Scenario: Later mode preparation preserves an earlier independent SET
- **GIVEN** a moving SET prepared for its original E and an unchanged source trajectory
- **WHEN** a later prospective Key Lock request prepares a tuple using the still-effective old numeric pitch
- **THEN** the earlier SET SHALL apply its own captured tuple at its original E
- **AND** its effect SHALL preserve the later pending mode request's original receipt and status ownership
- **AND** the later mode tuple SHALL refuse through its original captured-pitch mismatch without partially adopting another mode or invalidating that SET

#### Scenario: A restored endpoint cannot hide a changed future
- **GIVEN** a moving SET with a captured F-to-E source projection
- **WHEN** the source rate or read domain changes after F and is restored before E
- **THEN** that SET SHALL refuse through its source-trajectory fence even when the endpoint matches
- **AND** retained genuine reads SHALL remain available to a later original action

#### Scenario: A settled rate does not erase an earlier selection or failed read
- **GIVEN** a constant-rate source whose required SET history overlaps a previous stem selection or a refused source read
- **WHEN** the current source clock supports backwards constant reconstruction
- **THEN** the system SHALL still prove the actual selection/context chronology before reusing constant endpoint preparation
- **AND** it SHALL replay the genuine held past or refuse that request locally rather than repeat the current selection across the missing history

#### Scenario: A held same-set ramp precedes the settled original endpoint
- **GIVEN** a source whose required history contains a real FullMix-to-stems, stems-to-FullMix or stem-mask ramp backed by one still-held committed StemSet
- **WHEN** a nonstarting SET or prospective mode request captures that history, with any active ramp at F completing by the original E
- **THEN** replay SHALL preserve the actual selection weights and current/outgoing reader roles through the required past and frozen future
- **AND** every adopted PCM sample, source phase and FIFO count SHALL match an independent direct-Native A-reset reference without changing original E or replacing the adopted owner
- **AND** a FullMix E SHALL retain its required historical pin without creating an unused endpoint-reader mismatch

#### Scenario: Slicing across ramp completion preserves only actual reader dependencies
- **GIVEN** a retained or projected span starts during a selection ramp and extends beyond its completion
- **WHEN** replay slices before, exactly at or after that completion
- **THEN** every still-required role SHALL match its genuine held pin and original publication generation, including an empty selected mask
- **AND** a role no longer read after completion SHALL NOT create a fictitious dependency on an obsolete outgoing owner
- **AND** a missing, different or stale required owner SHALL refuse that original request without borrowing a marker or another participant's pin

#### Scenario: A storage-only WindowACK retains productive stem Native ownership
- **GIVEN** an adopted wet stem Native owner with a real source-bound stem publication
- **WHEN** actual resident adoption changes only its storage window and preserves the source, accepted timing, publication generation and required coverage
- **THEN** productive matching SHALL retain its exact Native/FIFO owner without automatic replacement or readiness failure
- **AND** a new preparation or adoption SHALL still require its own fresh current-window authority, while a changed held publication identity or an owning Native source/timing revocation SHALL still refuse

#### Scenario: Public variable-rate SET retains independent acoustic diagnostics
- **GIVEN** a genuine nonstarting SET with off-grid rate edits, a fixed analytic stereo source and separately isolated attack sources
- **WHEN** its adopted output is evaluated against independent direct-Native A-reset and continuous-from-O destination histories
- **THEN** the diagnostic SHALL retain the original action, source clock and uncropped response origins and use predeclared frequency windows, marker positions and unchanged timing, peak, energy-retention and tail limits
- **AND** exact A-reset PCM agreement SHALL NOT stand in for continuous-history, acoustic or performer-deadline acceptance, and every measured failing criterion SHALL remain visible without fitted translation, gain, cropping or threshold changes

#### Scenario: Native diagnostic messages require an isolated owner and actual runtime binding
- **GIVEN** a Native output warning observed while a public SET and independent A-reset and continuous-O references can have surviving Native owners
- **WHEN** a diagnostic attempts to attribute that warning to one of those roles
- **THEN** the diagnostic SHALL isolate the roles in separate processes, preserve the original input samples, rate trajectory, pitch, lock mode and response origins, and identify the actually imported Native DLL and binary identity in each process
- **AND** the diagnostic SHALL retain raw process output, exit status and unchanged PCM, FIFO and acoustic checks, while a non-reproduction SHALL NOT be reported as an attribution, a repair or acoustic acceptance
- **AND** an unavailable library version SHALL remain explicitly unavailable rather than being inferred from a header, API constant or assumed installation

#### Scenario: Ordinary Play withdraws a stopped selection receipt while retaining admitted stems
- **GIVEN** an actual stopped committed ALL selection has admitted its physical StemSet through its original receipt
- **WHEN** an ordinary Play obtains its own resident ACK and withdraws that earlier selection observation while retaining the same admitted PCM
- **THEN** its own fresh Native request SHALL prepare and adopt those exact stem readers at its original T/E without reviving or borrowing the earlier receipt
- **AND** unchanged copied publication provenance SHALL remain data evidence even when the earlier operation counter advances, while changed physical/publication identities and stale owning Native authority SHALL still refuse
- **AND** a subsequent explicit ALL confirmation reusing the same admitted set SHALL keep its original data-publication stamp and existing Native history while reaching its own effective receipt
- **AND** shared-cache reconstruction without such an admitted set SHALL instead obtain the new operation's genuine admitted publication stamp
