## ADDED Requirements

### Requirement: Semitone Intent Is Independent Of Musical Time
The system SHALL define an internal semitone intent k with frequency ratio h=2^(k/12) independently
of canonical source mapping, source rate, master beats, creative phase and launch target.

In this preparation slice production k SHALL remain zero and nonzero k SHALL be injectable only
in isolated tests/examples. KEYLOCK diagnostics SHALL retain intended source pitch plus k while
tempo follows the accepted map. Pitch intent SHALL NOT modify global.speed, manual_key, manual
BPM, original audio/stems, B/S revision, loop markers, M, phi, captured trigger T or other pads.

#### Scenario: One pad changes diagnostic key while several mapped pads play
- **GIVEN** exact maps and identical transport/input events for several simulated voices
- **WHEN** only one voice receives a nonzero diagnostic k sequence
- **THEN** every voice retains the control run's canonical source positions, wraps and target frames
- **AND** the common master clock, source maps and original metadata remain unchanged

### Requirement: Pitch And Duration Ratios Have Explicit Distinct Meanings
The system SHALL compare equal-rate mapped-varispeed rendering with native pitch p=h/r against
original-source Stretcher rendering with duration ratio q=1/r and independent pitch p=h.

The inverse map SHALL return source seconds, which SHALL be multiplied by the loaded source
sample rate before deriving r as source frames per output frame, excluding explicit loop wraps.
Seconds per output frame SHALL NOT be passed as the tempo ratio to native pitch/duration APIs.

The diagnostic SHALL distinguish sample-rate conversion from physical duration if rates differ
and SHALL NOT apply the same tempo change twice. It SHALL cover proposed future KEYLOCK-off
varispeed plus p=h, whose audible pitch is r*h and whose k=0 case preserves legacy varispeed.
This proposal SHALL NOT activate a new live control or reinterpret existing Key Lock behavior.

#### Scenario: Equal physical source and output progression has unit rate
- **GIVEN** equal 48000-Hz source/output rates and a map advancing one source second per output second
- **WHEN** the diagnostic derives r from inverse-map coordinates
- **THEN** it converts seconds to source frames and obtains r=1
- **AND** it does not treat 1/48000 seconds per output frame as the native tempo ratio

#### Scenario: Unit-rate playback has deliberate nonzero transposition
- **GIVEN** r=1 and nonzero diagnostic k
- **WHEN** the renderer evaluates the requested pitch
- **THEN** pitch processing produces the intended h ratio or reports rejection
- **AND** a rate-only bypass does not silently omit the transposition

### Requirement: Combined Pitch And Rate Bounds Are Never Silently Clipped
The system SHALL validate and report the supported combined pitch/rate domain before accepting
diagnostic transposition, retaining separate requested, pending, effective and rejected state.

The [-12,+12] semitone and [0.5,2] rate matrix SHALL be treated as diagnostic coverage requiring
LiveShifter p in [0.25,4], not a promised UX range or proven native operating envelope. Unsupported
requests SHALL retain the previous effective value with a reason. No hidden clamp, map change,
rate change, phase shift or clock movement SHALL make an unsupported request appear accepted.

#### Scenario: Current inverse-pitch limits cannot represent a requested combination
- **GIVEN** r=0.5 and k=+12 require native p=4 in the mapped-reader path
- **WHEN** the evaluated configuration supports only p up to 2
- **THEN** the request is reported as unsupported instead of silently applying p=2
- **AND** source progression and master timing remain unchanged

### Requirement: Prepared Pitch State Uses The Same Intended Output Coordinates
The system SHALL include pitch revision/trajectory in source-specific prepared-state and derived
render-cache identity and compare old/new states at the same intended output n and source s(n).

The identity SHALL also include KEYLOCK mode and intended output range alongside source, map,
loop, tempo, channel/stem topology, renderer options/version and initial history.
Pitch-only edits SHALL preserve raw source/map/stem identity. A newer pitch revision SHALL reject
stale prepared output without rerounding the captured launch target. Pending, late and rejected
adoption SHALL record the effective pitch and declared fallback/defer outcome. Native feed,
buffered output and estimated audible position SHALL remain distinct timing domains.

#### Scenario: Key changes while a source-specific launch is preparing
- **GIVEN** a pending captured target T and a prepared result for an older pitch revision
- **WHEN** a newer diagnostic pitch request arrives
- **THEN** the older result is not adopted as the newer requested pitch
- **AND** T, M and phi stay unchanged while the declared preparation/defer outcome is recorded

#### Scenario: Native pitch crosses unity with a nonunit source rate
- **GIVEN** a k/r trajectory crosses p=h/r=1
- **WHEN** the diagnostic evaluates a wet/bypass or old/new transition
- **THEN** it measures output alignment and history at the same musical coordinates
- **AND** near-unity pitch alone is not accepted as proof that an immediate bypass is synchronized

### Requirement: Pitch Timing Evidence Separates Logical And Acoustic Guarantees
The system SHALL require exact canonical timing invariance under changes to k and independently
report audible pitch, event timing, duration, attack/energy/peak, loop and stem measurements.

All stems selected for a pad SHALL share its pitch intent and source trajectory. Shared settings
or equal lengths SHALL NOT certify spectral coherence or independently processed null-sum equality.
Different intentional keys SHALL NOT be compared by requiring identical PCM waveforms. Original
failed acoustic and causal criteria SHALL remain unchanged and separately visible.

#### Scenario: Source timing is correct but shifted attacks move audibly
- **WHEN** a candidate passes source-coordinate invariance but fails an acoustic timing predicate
- **THEN** the report records both results and does not declare synchronized audible acceptance
- **AND** neither the shared clock nor the original acoustic threshold is altered to hide failure

### Requirement: Contract Preparation Does Not Activate Live Transposition
The system SHALL keep nonzero transposition diagnostics outside production playback and the audio
callback, with all experimental construction, reset, preparation and destruction off callback.

Offline results SHALL NOT certify live native allocation safety, deadlines, hardware latency or
long-session acceptance. No KEY UI, public input action, persistence rollout, silent model download
or new live pitch adoption SHALL be introduced by this preparation change.

#### Scenario: A nonzero transposition diagnostic finishes
- **WHEN** the isolated experiment exports its measurements
- **THEN** production remains at zero KEY intent with existing behavior and originals intact
- **AND** future live adoption and performer controls remain separate unimplemented work
