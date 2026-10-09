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
