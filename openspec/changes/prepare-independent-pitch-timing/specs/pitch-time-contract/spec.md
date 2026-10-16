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

The first bounded original-source comparison SHALL reuse the existing isolated
48-kHz, source-rate2, H16384 impulse and its original source/output target. Offline
Stretcher duration q=.5 and independent pitch p=1 SHALL be configured before its
complete study pass. Its overall duration and source-to-output anchors SHALL be
fixed from source geometry before processing, with no response-fitted offset,
gain, crop, target or threshold. The diagnostic SHALL distinguish documented
offline startup alignment from an illustrative nominal LiveShifter translation.

Both predeclared R2 and R3 engines SHALL report their actual engine selection,
duration/pitch factors, native frame counts and complete uncropped output. Study
and process partitions SHALL preserve the same input and complete-output
comparison. The unchanged two-ms q10/q50, original peak, .1-percent discarded
energy and capture-tail criteria SHALL remain visible for uncropped output and
strict first emission at the original target. A failed predicate SHALL remain a
failed predicate, independently of API/coordinate/partition success. This
bounded result SHALL NOT certify variable trajectories, finite live handover,
nonzero pitch, callbacks, device latency or Human listening.

The diagnostic SHALL distinguish direct native-call partition behavior from
offline adapter ingress partition independence. An adapter MAY stage at most
512 source frames per channel and submit fixed 512-frame native blocks plus
one final remainder. It SHALL preserve every original input frame and anchor,
record the actual native block sequence and require bitwise equality of the
complete output across caller partitions and against the direct fixed-block
reference. Passing this adapter check SHALL NOT relabel a failed direct-native
partition or acoustic predicate as passing. The semantic zero origin SHALL
remain implicit in the native keyframe encoding to avoid a zero first divisor.

#### Scenario: Original-source duration processing is compared with the failed scalar route
- **GIVEN** the existing source-rate2 H16384 impulse with a frozen source/output map and original target
- **WHEN** separate offline R2 and R3 Stretcher instances study and process that original source with q=.5,p=1 while the mapped-reader LiveShifter retains p=.5
- **THEN** full uncropped measurements and every original failed acoustic/causal criterion SHALL be reported separately from exact output duration and processing-partition agreement
- **AND** no selected measurement offset, early audible retune or production renderer change SHALL manufacture acceptance

#### Scenario: Equal physical source and output progression has unit rate
- **GIVEN** equal 48000-Hz source/output rates and a map advancing one source second per output second
- **WHEN** the diagnostic derives r from inverse-map coordinates
- **THEN** it converts seconds to source frames and obtains r=1
- **AND** it does not treat 1/48000 seconds per output frame as the native tempo ratio

The bounded causal source-filter diagnostic SHALL freeze its model order,
original-source fixtures, innovation mapping, synthesis equations and frequency
windows before measurement. Its AR2 simplification SHALL retain the complete
source residual, admit each innovation no earlier than its mapped frame, and
separately test identity reconstruction and nonzero pitch on sustained and mixed
tones. A causal impulse alone SHALL NOT establish tonal pitch preservation.
The diagnostic SHALL report actual strongest spectral peaks without selecting
only peaks near requested frequencies. Any failed original15-cent tonal predicate
SHALL reject that fixed candidate. Unsupported poles and out-of-range transformed
frequencies SHALL be explicit, with no coefficient, gain or pitch clipping.
It SHALL NOT claim the original acoustic timing, complete-tail, finite-history,
live or Human gates from this frequency-focused falsification. Previous Native
and Stretcher failures SHALL remain visible; no production route is activated.

#### Scenario: Causal innovations do not imply correct transformed residual pitch
- **GIVEN** fixed original-source mono and two-tone fixtures, an identity control and k=-7,0,+7 at source-rate2
- **WHEN** the same fitted AR2 source-filter model retimes residual innovations and rotates its pole pair
- **THEN** source calibration, identity error, metrics over the complete captured output and actual strongest output frequencies SHALL be recorded separately from structural onset causality
- **AND** an incorrect tonal frequency SHALL reject the candidate without changing its order, excitation map, scoring window or original acceptance thresholds

The bounded full-PCM overlap diagnostic SHALL freeze its source-only cue detector,
window, hop, search bound, interpolation, segment mapping and retirement rules
before measurement. Its renderer SHALL receive only original stereo PCM, rate
and pitch intent, with no fixture attack coordinates or output scoring targets.
It SHALL resample every component of each source segment and retain every
generated positive-time grain sample, including overlap beyond the next segment.
Source-derived segmentation SHALL NOT authorize a crop, fitted gain, fitted
offset or final-sum normalization. Whole-source offline lookahead SHALL be
reported explicitly and SHALL NOT be claimed as streaming causal inference.

The diagnostic SHALL cover the original impulse, short tonal attack, sustained
mono, mixed two-tone source and two nearby attacks over a running background.
Source calibration, exact r=1/k=0 reconstruction, centered stereo energy timing,
original peak, pre-target energy, complete finite retirement and actual strongest
spectral peaks SHALL remain distinct predicates. A paired background control
SHALL expose any changed pre-target PCM; nonlinear mixture differences SHALL NOT
be relabeled isolated attack responses or used to certify original retention.
Any failed original acoustic or 15-cent tonal predicate SHALL reject the frozen
candidate. Identity reconstruction or an aligned single impulse SHALL NOT close
the mixed-signal, finite live history, callback or Human acceptance gates.

#### Scenario: Full PCM overlap preserves or rejects source-derived nearby attacks
- **GIVEN** frozen original-source fixtures, k=-7,0,+7 at source-rate2, an r=1/k=0 identity control and no attack metadata passed to the renderer
- **WHEN** source-derived boundaries and bounded stereo-shared waveform alignment render complete PCM with every admitted grain tail retained
- **THEN** original timing, retention and strongest-frequency results SHALL be recorded without post-measurement tuning
- **AND** two nearby attacks on a running background and its matched control SHALL reveal boundary gain, duplicated attacks or premature changes without manufacturing a passing isolated response

The bounded full-phase-gradient diagnostic SHALL use an explicit common centered
STFT coordinate convention for analysis, synthesis and scoring. It SHALL retain
the complete signed output support of every admitted synthesis packet and finite
resampling filter, including negative-time output. A predetermined capture-array
origin MAY encode that signed support; it SHALL NOT change the original target,
fit an acoustic delay or discard pre-target energy. Canonical dry references
SHALL use the same declared coordinates.

Temporal instantaneous-frequency and frequency-direction phase gradients SHALL
both participate. Rounded synthesis centers SHALL retain their explicit phase
correction. Static synthesis windows/dual weights and finite SRC coefficients
SHALL be fixed before measurement; wet values SHALL NOT determine a fitted gain.
Stereo channels SHALL share phase-correction decisions, retaining their measured
interchannel spectral relation. A magnitude priority threshold SHALL NOT discard
small nonzero spectral coefficients. Unsupported stretch factors SHALL fail
explicitly instead of being clamped.

Real DC/Nyquist corrections SHALL enter stored phase state before dependent
frequency or subsequent-frame propagation uses them. Tail energy SHALL be
measured on the actual renderer-end interval; appended silence SHALL NOT provide
a passing tail predicate. Regression controls SHALL expose a sign-changing
endpoint/state mismatch and a nonzero final renderer sample before their fixes.

The diagnostic SHALL separately verify centered-coordinate gradients, unchanged
coefficient unity reconstruction, source-frequency calibration and the original
isolated timing/peak/retention criteria. It SHALL reuse the failed short attack
and lower two-tone source, plus a matched busy-background attack control. Passing
unity reconstruction SHALL NOT certify phase integration, complete live history,
general pitch range, real-time execution or Human listening. Fixed SRC filter
delay SHALL remain in every reported acoustic timing result.

#### Scenario: Full phase gradients preserve the original signed acoustic response
- **GIVEN** a frozen centered full-spectrum candidate, original source-only fixtures and unchanged source-rate2 k=-7,0,+7 plus unity controls
- **WHEN** shared stereo phase correction and finite resampling render every admitted packet on its predetermined signed timeline
- **THEN** gradient integrity, source calibration, short-attack timing, low-polyphonic pitch and complete pre-target energy SHALL be reported separately
- **AND** no crop, fitted translation, wet gain, omitted tail or changed target SHALL manufacture acceptance

#### Scenario: Unit-rate playback has deliberate nonzero transposition
- **GIVEN** r=1 and nonzero diagnostic k
- **WHEN** the renderer evaluates the requested pitch
- **THEN** pitch processing produces the intended h ratio or reports rejection
- **AND** a rate-only bypass does not silently omit the transposition

### Requirement: Combined Pitch And Rate Bounds Are Never Silently Clipped
The system SHALL validate and report the supported combined pitch/rate domain before accepting
diagnostic transposition, retaining separate requested, pending, effective and rejected state.

The expanded B5 matrix SHALL cover base -5..+6, every37 extra values -18..+18,
total k=-23..+24 and actual accepted r(n) extrema. At equal-rate r=.5..2 the
lockON h/r path requires p approximately.132433..8; lockOFF SHALL evaluate p=h.
The old [-12,+12]/[.25,4] examples SHALL remain regressions, not replacement coverage
or proof of native quality/RT/readiness/latency/finite-context support. Unsupported
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
Pitch-only edits SHALL preserve raw source/map/stem identity. A newer requested-state revision SHALL reject preparation falsely presented as
that newer request without rerounding its captured launch target. Each already
admitted SET_RETRIGGER SHALL instead retain its own frozen tuple/action sequence/
source-lifetime/current permit and existing scheduler event; a later pitch request
SHALL NOT automatically stale or replace that admitted attack. Only preparation
work MAY coalesce; removal/invalid current lifetime SHALL still fence unsafe events. Pending, late and rejected
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

This requirement scopes the preparation-only diagnostic change. Production
activation is separately governed by `add-key-transposition-performance`; its
performer, native readiness and live acceptance obligations cannot be inferred
from passing these diagnostics.

Offline results SHALL NOT certify live native allocation safety, deadlines, hardware latency or
long-session acceptance. No KEY UI, public input action, persistence rollout, silent model download
or new live pitch adoption SHALL be introduced by this preparation change.

#### Scenario: A nonzero transposition diagnostic finishes
- **WHEN** the isolated experiment exports its measurements
- **THEN** production remains at zero KEY intent with existing behavior and originals intact
- **AND** future live adoption and performer controls remain separate unimplemented work
