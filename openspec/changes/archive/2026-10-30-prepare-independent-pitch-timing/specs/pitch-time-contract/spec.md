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
- **GIVEN** equal loaded-source/output rates at 44100, 48000 and 96000 Hz and a map advancing one source second per output second
- **WHEN** the diagnostic derives r from inverse-map coordinates
- **THEN** it converts seconds to source frames and obtains r=1
- **AND** it does not treat 1/Fs seconds per output frame as the native tempo ratio

#### Scenario: Accepted inverse coordinates agree with actual playback and native pitch
- **GIVEN** a current owned source, a genuinely admitted and prepared nonzero pitch action and the actual transport at each equal loaded-source/output rate
- **WHEN** a current acknowledged constant-timing publication reaches the voice through the real preparation path and the hardware-free diagnostic evaluates the Python scalar inverse in seconds, converts once to loaded frames and derives a non-wrapping source-frame/output-frame rate for unit, slower, faster and fractional-period progression
- **THEN** absolute source coordinates and their progression SHALL agree with an independent physical time/beat oracle, the accepted render grid and the actual voice cursor
- **AND** the retained pitch intent SHALL configure the actual native state to h/r with KEYLOCK on and h with KEYLOCK off without changing that source progression
- **AND** omitting or duplicating the seconds-to-frames conversion SHALL fail the unit-rate comparison
- **AND** unequal-rate arithmetic SHALL be identified separately from the productive loader's equal-rate resampling invariant; this diagnostic SHALL NOT claim unequal-rate live support, acoustic quality, realtime deadlines or device/Human acceptance

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

The bounded subband AM/FM diagnostic SHALL define a finite complete analysis bank
whose real subband sum reconstructs every original stereo sample, including DC,
Nyquist and every complementary component. It SHALL freeze source-only segment
births, filter support, phase unwrapping, stereo reference, interpolation and
retirement rules before measuring output. All components SHALL receive the same
requested envelope-time and carrier-frequency transformation; an unchanged
persistent residual or untransposed attack bridge SHALL NOT substitute for a
complete transformation.

Segment reconstruction SHALL be verified separately from transformed-signal
quality. Every generated subband and segment tail SHALL remain in the complete
output, including overlap beyond a later segment and beyond nominal duration.
The renderer SHALL receive only original PCM, rate and pitch intent. Source-only
birth detection SHALL NOT receive the scorer's attack targets or authorize a
fitted delay, gain, final-sum normalization or response crop. Known offline source
access and nonanalytic finite-band leakage SHALL be reported as limitations.

The original short tonal attack, nearby impulse pair, lower two-tone source and
matched busy-background controls SHALL be retained. Two independently gated
carriers plus an impulse SHALL additionally expose unresolved band mixtures and
complementary energy. Original source calibration, strongest spectral peaks,
complete pre-target energy, timing and actual renderer-end tail predicates SHALL
remain unchanged. Any failed original predicate SHALL reject the candidate;
perfect unity reconstruction SHALL NOT certify arbitrary mixtures, the full pitch
domain, live history, realtime resources or Human acceptance.

#### Scenario: Complete subband envelopes and carrier phases are independently scaled
- **GIVEN** one frozen finite complete bank, source-only births and original PCM fixtures including independently gated carriers and nearby impulses
- **WHEN** every subband envelope follows source coordinate r*n and every carrier follows the independently requested h ratio with all generated tails retained
- **THEN** analysis completeness and unity identity SHALL be checked before measuring the original timing, retention and strongest-frequency predicates
- **AND** a failing mixed or transient response SHALL remain a rejection without omitting a residual, shifting targets or fitting output parameters

The fixed-bank synthesis calibration SHALL independently calculate each finite
filter response at both positive and negative source frequencies and compare it
with explicit FIR evaluation. A real source sinusoid SHALL retain both images in
the prediction. The linear reconstruction identity SHALL NOT be substituted for
a transformed synthesis identity. Carrier-anchored phase winding and finite
source-onset phase history SHALL remain explicit rather than replaced by a
principal filter phase or a fitted output offset.

Predicted transformed PCM and gain SHALL be compared with the actual unchanged
renderer at predetermined source coordinates. Any oracle frequency used in an
analytic integrity control SHALL remain confined to that control and SHALL NOT
enter the PCM-only renderer. A source-derived estimate SHALL report its error
separately. Any image-aware inverse SHALL report conditioning, reconstruction
error and every component outside its modeled span. A mode-specific correction
SHALL NOT be represented as a complete arbitrary-mixture renderer, and no fitted
wet gain or omitted residual SHALL manufacture quality acceptance.

#### Scenario: Known analysis responses explain or reject transformed synthesis gain
- **GIVEN** the unchanged finite bank, original source-only mono/mixed fixtures and predetermined measurement coordinates
- **WHEN** independent filter-response and image/winding predictions are compared with actual transformed PCM
- **THEN** response integrity, source estimation, predicted gain error, conditioning and out-of-span residual SHALL be reported separately
- **AND** a resolved-mode calibration SHALL NOT close the original transient, mixture, full-range or live acceptance gates

The explicit-mode diagnostic SHALL estimate carrier count, frequencies and
interchannel phase coefficients from original source PCM alone. It SHALL freeze
its window, rank, pole, coherence, refinement and conditioning rules before
measurement. Independent nonnegative source envelopes SHALL be fitted jointly
without observing wet PCM, timing targets or requested output gains. Continuing
carriers SHALL retain their declared source birth gauge across unrelated impulses
and global source-cue boundaries.

The complete source residual SHALL equal original PCM minus the real sum of all
admitted modes, including every uncertain, rejected, transient, noisy and numerical
component. Every residual sample SHALL pass through the complete finite-bank
transform with all generated tails retained. No unchanged residual, unpitched
attack bridge or output normalization SHALL replace that transformation. Modes
and residual SHALL share the same declared zero-extended interpolation geometry,
so their combined h=r path reconstructs the original interpolated source.

Source estimation, nonnegative-fit conditioning, exact decomposition, rate-only
identity and nontrivial mode phase correction SHALL be checked separately. All
original quality fixtures SHALL remain, and unseen frequencies, close spacing,
independent gates, impulses, AM/chirps, noise, unrelated stereo and near-Nyquist
sources SHALL expose assumptions and unsupported domains. A known carrier shifted
outside the representable Nyquist interval SHALL be rejected explicitly rather
than silently aliased or clamped. Unresolved residual aliasing SHALL remain an
explicit unclosed limitation. A small source-fit residual SHALL NOT certify a
small transformed-output error or universal physical-mode identification.

#### Scenario: An unrelated impulse does not rebirth independent sustained modes
- **GIVEN** source-derived stationary carriers with independent compact envelopes and an exact complementary residual
- **WHEN** the full source is rendered at independent requested rate and pitch, including a later unrelated impulse
- **THEN** continuing mode gauges SHALL persist and every residual component and tail SHALL be transformed
- **AND** original and unseen timing, retention, frequency and gain evidence SHALL determine rejection without changing the original acceptance gates

The source-window shadow diagnostic SHALL distinguish changing mixture energy
from changing carrier frequency. It SHALL preserve the original energy/crest
refusal while independently reporting source-only rank, pole, conditioning and
complete-window exponential-fit evidence. Two fixed column geometries SHALL use
the same original window, row count, one-sample shift and admission thresholds;
the sparse column offsets SHALL NOT change the frequency sample rate or authorize
aliasing. All original and unseen source windows, including chirp, noise, short
attacks, silence and unrelated stereo, SHALL retain explicit refusal results.
Shadow certificates SHALL NOT admit carriers into the frozen renderer or certify
transformed timing, gain, source causality, aliasing or realtime quality.

#### Scenario: Stationary close carriers beat below an energy-flatness guard
- **GIVEN** source-only windows refused by the original energy/crest guard
- **WHEN** fixed compact and spread-column pencils evaluate the same complete source windows
- **THEN** the report SHALL retain the guard refusal alongside actual rank, pole and full-window source-fit evidence
- **AND** unchanged rendering results and original acoustic acceptance gates SHALL remain in force

The certified-mode candidate SHALL use full-window exponential-fit certificates
before the existing recurrence, source refinement and stereo-coherence checks.
Its local causal envelope option SHALL use only trailing source windows and
already-computed envelope knots; no future knot SHALL be interpolated into the
source prefix. The original centered-envelope candidate SHALL remain selectable
with unchanged arithmetic. A fixed identical carrier basis SHALL separately
isolate envelope prefix invariance from all-file carrier discovery. All-file
carrier estimation SHALL NOT be called causal merely because local envelopes
use no future samples. Exact residual transformation, all tails, unsupported
domains and all original/unseen acoustic criteria SHALL remain required.

#### Scenario: A later impulse cannot change earlier local envelopes on a fixed basis
- **GIVEN** two sources with the same prefix and exactly the same frozen source-carrier basis
- **WHEN** trailing-window NNLS and left-held knots fit their independent envelopes
- **THEN** every envelope and exact complementary residual sample before the changed source frame SHALL remain equal
- **AND** complete transformed profiles SHALL independently expose timing, gain, mixture and global-discovery limitations

An independent known-mode reference SHALL separate declared exact carrier/envelope
behavior from estimation and transformed-residual errors on the same additional
control source. Its phase birth convention and complete support SHALL be fixed
without fitting output. Known parameters SHALL enter only the analytic reference;
the estimated candidate SHALL still receive source PCM alone. Any reference
mixture-energy timing failure SHALL remain visible alongside original failures
and SHALL NOT establish universal impossibility or relax an acoustic gate.

The affine-envelope candidate SHALL jointly fit two nonnegative source-window
endpoint amplitudes per admitted carrier on the same fixed trailing window and
knot grid. Its current endpoint SHALL be predicted only from past weighted
samples and SHALL retain the declared left hold. Both Hann endpoints have zero
weight; the prediction SHALL NOT be represented as a directly observed current
amplitude or guaranteed transient improvement. The actual doubled-column design
SHALL retain existing conditioning, bounded NNLS and refusal rules, with no ridge,
output gain fitting or altered quality threshold.

The diagnostic SHALL report true design conditioning, endpoint/slope evidence,
explicit refused tiles and unconstrained endpoint leverage separately from
constrained active-set sensitivity. Every refused or imperfect sample SHALL
remain in the exact completely transformed residual. Independent constant,
affine-amplitude, off-grid future-impulse, differing-hull, stereo and conditioning
controls SHALL precede complete original/unseen quality measurements. Exact
endpoint recovery at resolved knots SHALL NOT certify between-knot interpolation,
mixture energy timing, arbitrary modulation, aliasing or realtime quality.

#### Scenario: A causal affine endpoint fit removes only resolved linear-envelope bias
- **GIVEN** admitted fixed source carriers with independently known constant or affine control envelopes
- **WHEN** bounded trailing-window NNLS fits nonnegative endpoints and holds the current endpoint
- **THEN** knot prediction, declared between-knot hold error, prefix invariance, conditioning and refusal SHALL be reported independently
- **AND** all source errors, finite decay and transformed tails SHALL remain in the unchanged full-response acoustic comparisons

### Requirement: Causal Affine Prediction Retains Complete Error Evidence
The separate predictive-envelope diagnostic SHALL reuse the frozen affine
endpoint fit, admitted source carriers, conditioning/refusal limits and fixed
knot grid. Between knots it SHALL evaluate max(0,R+d*(R-L)/255) for source-frame
age d=0..63, without future samples, retrospective interpolation, an appended
end knot or a last-active mask. Projection SHALL preserve nonnegative envelopes;
every projected error and fitted decay SHALL remain in the exact fully
transformed complementary residual and complete output.

Independent affine full-hop and analytic zero-crossing controls SHALL distinguish
removed hold error from extrapolation sensitivity and abrupt-change error.
Unconstrained prediction row norms at ages0 and63 SHALL be reported separately
from actual PCM-noise gain and NNLS active-set sensitivity. Old envelope variants
SHALL remain selectable. Neither exact affine reproduction nor projection SHALL
certify physical AM interpretation, stationary-mixture energy timing, arbitrary
modulation, residual-only sources, full-pipeline causality or product quality.

#### Scenario: A fitted affine slope predicts only the next declared source hop
- **GIVEN** fixed source carriers and a fully supported affine fitting window
- **WHEN** the diagnostic extrapolates the fitted endpoints until the next fixed knot
- **THEN** it SHALL compare full-hop amplitudes, knot identity and an independently projected zero crossing
- **AND** it SHALL retain unchanged full-response acoustic gates and all conditioning, residual and tail failures

### Requirement: Positive Current Weight Retains Complete Diagnostic Obligations
The separately selected current-weight diagnostic SHALL use the fixed rising
half-cosine weights w[m]=0.5*(1-cos(pi*m/255)),m=0..255, retaining the same causal
affine endpoint model, source-only carriers, actual doubled-column conditioning,
NNLS limits, fixed knot grid and projected next-hop publication. Old variants
SHALL remain selectable. The positive current weight SHALL NOT be described as
an exact current-envelope observation or guaranteed attack-quality improvement.

Before quality measurement, independent positive-plateau controls with small
signed physical current-frame perturbations SHALL verify the predicted full-hop
response and the old Hann zero-response comparator. They SHALL verify that the
declared unconstrained endpoint and projection constraints remain inactive.
Independent Gram/moment controls SHALL distinguish weighted-target row norms
from unconstrained actual-PCM row norms, including squared weights in the latter.
Neither norm SHALL certify sensitivity after active-set changes or full quality.
All affine, projection, prefix, refusal, exact-complement, finite-tail and
unchanged complete-response acoustic obligations SHALL remain required.

#### Scenario: Positive current weight responds to a physical source-knot perturbation
- **GIVEN** a fixed circular-stereo carrier on a positive plateau and predeclared small signed current-frame cues
- **WHEN** the diagnostic fits the original source window and predicts its next fixed hop
- **THEN** whole-hop cue responses SHALL match independent weighted-design algebra with inactive constraints
- **AND** original failures, complete residuals, raw amplification and unsupported domains SHALL remain explicit

### Requirement: Positive AM Families Have A Separately Certified Phase Owner
The offline diagnostic SHALL provide a separately selected source-certified
positive-AM phase-owner model. Only a certified symmetric three-carrier family
MAY be replaced with its central carrier and a real positive source-clock
envelope. Admission SHALL use original stereo PCM and source-estimated carriers
only, independently of requested pitch/rate, fixture metadata and scoring targets.
Constrained symmetric-frequency source fits, common complex stereo direction,
conjugate sideband/center coefficient ratios, strict envelope positivity and
consistent complete source-window support SHALL all be required. Existing source
fit, conditioning, coherence and support limits SHALL remain in force. Ambiguous
overlapping families SHALL retain the original independent carriers.

The algebraic factorization SHALL NOT certify unique physical AM intent for an
arbitrary mixture. Only a certified family's central phase SHALL follow pitch;
its real envelope SHALL follow the unchanged source clock. The unchanged causal
current-weight envelope fitter SHALL retain every subtraction error in the exact
fully transformed residual, with all decay and generated tails. Uncertified
carriers, prior variants, original/unseen sources and acoustic gates SHALL remain
available and unchanged. No response-fitted phase, gain, offset, crop, target or
threshold SHALL manufacture acceptance.

Independent positive and negative source controls SHALL verify exact AM
factorization, stereo/conjugacy/positivity/spacing/support refusals and ambiguous
families before the full unchanged quality matrix. A known-source comparison
SHALL distinguish grouped carrier/envelope semantics from independently pitched
sidebands. Failure on a close pair or residual-only chirp/noise SHALL remain
visible and SHALL NOT be relabeled as universal impossibility or acceptance.
Additional independent source-center tonal controls SHALL measure the candidate
AM carrier against the unchanged 15-cent bound without supplying that center to
preparation or changing existing fixture frequency tuples and quality scores.

#### Scenario: Source algebra certifies a positive AM triple without output intent
- **GIVEN** source-estimated carriers and original PCM with at least the existing minimum clean complete-window support
- **WHEN** symmetric constrained fits and conjugate vector coefficients certify a strictly positive real envelope around one central carrier
- **THEN** the diagnostic MAY fit that family's source envelope using the unchanged current-weight model and pitch only its central phase
- **AND** it SHALL preserve the complete transformed residual, all unsupported domains and every original full-response failure

### Requirement: Certified Positive Modulation Can Belong To The Fixed Source Basis
The offline diagnostic SHALL provide a separately selected fixed-modulator
experiment after source-only positive-AM family certification. The certified
global modulation frequency and conjugate sideband ratio SHALL define the real
positive modulator in the original source coordinates. That modulator SHALL
multiply the existing carrier features in every weighted source fit. The same
bounded joint nonnegative affine endpoint solver, trailing 256-frame window,
64-frame prediction grid, observation weights and rejection limits SHALL then
estimate the common amplitude before modulation. Published physical envelopes
SHALL multiply each nonnegative common-amplitude prediction by the certified
modulator at that same source frame. No output intent, fixture identifier,
scorer target, response-fitted phase or extra end mask SHALL select the basis.

Common-amplitude endpoints and conditional fit-parameter sensitivity SHALL be
reported separately from the published physical envelope. Independent weighted
moment controls SHALL verify both fit-parameter and physical-envelope response
to signed source cues, including modulation at prediction ages zero and 63.
Physical-envelope row norms SHALL include the modulator's value at publication;
neither norm SHALL claim nonlinear active-set or general mixture sensitivity.
Additional bounded workspace SHALL be included before fitting allocation.

Uncertified carriers SHALL use the unchanged stationary basis. Prior variants
SHALL remain available, and source without a certified family SHALL preserve the
prior signal path. Exact subtraction complements SHALL retain every fitting
error and all decay; the existing complete residual transform SHALL process all
of them. Source certificate refusals, original unsupported requests, source
calibration, the full unchanged quality matrix and all product gates SHALL remain
required. Algebraic controls SHALL NOT substitute for measured acoustic quality.

#### Scenario: Fixed source modulation separates common amplitude from known curvature
- **GIVEN** an independently certified positive AM family and an affine common source amplitude
- **WHEN** the separate experiment fits original stereo PCM against the modulated carrier features
- **THEN** complete supported prediction hops SHALL reproduce the common amplitude times its positive source modulator
- **AND** independent source-cue and future-cue controls SHALL retain exact complements and distinguish common-amplitude from physical-envelope sensitivity

### Requirement: Source Certified Linear Chirplets Retain One Integrated Phase Law
The offline diagnostic SHALL provide a separately selected source-only linear-chirplet
experiment when the existing stationary estimator admits no carrier. Blind source
zero crossings MAY initialize frequency and slope, but original complete stereo
windows SHALL certify the model by bounded projected refinement. Admission SHALL
retain the existing minimum three clean complete 8192-frame windows, 4096-frame
scan hop, relative source RMS limit of 1e-4, design condition limit of 1e6 and
normalized complex stereo coefficient distance limit of 1e-3. Support windows
SHALL agree on one affine instantaneous-frequency law and one fixed stereo phase
gauge at a declared source epoch. Independent window phase resets, output intent,
fixture identifiers and scorer targets SHALL NOT determine that law. Ambiguous,
insufficient or invalid source models SHALL remain in the complete residual.

The same current-weight nonnegative affine envelope solver SHALL evaluate the
integrated source phase in its features. The source phase SHALL integrate the
affine frequency once. For constant source rate r and pitch ratio h, each admitted
mode SHALL apply exp(i*(h/r-1)*(theta(s)-theta(b))) at source position s and fixed
birth b. Independent controls SHALL verify the resulting instantaneous frequency
h*f(r*n), including the slope h*r*alpha. Fractional interpolation error SHALL be
reported separately from continuous phase algebra. The h=r combined-source identity,
exact fully transformed complement and all finite decay tails SHALL be retained.

Known instantaneous frequencies SHALL be positive and below Nyquist over the
original active source support. Rendering SHALL additionally refuse output-band
crossings over the entire nonzero fitted mode and its interpolation support;
neither cropping nor a new end mask SHALL hide a crossing. Bounded phase workspace
SHALL be accounted for before fitting allocation. Prior variants and every existing
unsupported-domain refusal SHALL remain available. Independent unseen ascending
and descending chirplets and negative noise, impulse, nonlinear-FM, phase-jump,
close-pair and unrelated-stereo controls SHALL precede the unchanged full quality
matrix. Source algebra SHALL NOT certify general mixtures, variable-rate maps,
native realtime execution or any remaining product acceptance gate.

#### Scenario: A source glide shares one phase law across clean stereo windows
- **GIVEN** original PCM supports one linear instantaneous-frequency law and coherent stereo phase in at least three clean complete windows
- **WHEN** the separately selected diagnostic prepares and transforms that source
- **THEN** its envelope features and output phase correction SHALL use the same integrated source phase law
- **AND** complete residuals, finite tails, prior variants and all original full-response criteria SHALL remain observable

### Requirement: Conditional Finite AM Gates Compare All Feasible Source Candidates
The offline diagnostic SHALL provide a separate conditional finite-gate experiment
for one already certified positive-AM family without other retained carriers.
Its declared model SHALL combine a finite raised-cosine attack, plateau and release
with one global complex amplitude/phase multiplier. Original stereo PCM and the
fixed source carrier/modulator SHALL determine the fit. No pointwise division by
a carrier sample, output intent, fixture identifier or scorer target SHALL select
the model. A certified interior reference window SHALL be an explicit plateau
restriction of this experiment, not a claim that certification proves a plateau.
An admitted gate's fitted onset SHALL supply its fixed mode phase birth. This
source-model coordinate SHALL remain distinct from the first observed nonzero PCM
frame and SHALL NOT alter source progression, musical targets or residual support.

The initial source grid SHALL be integer frames. Each edge observation neighborhood
SHALL extend 512 frames on each side of the corresponding first/last active source
frame and remain inside the finite input. Onset candidates SHALL span first-active
minus 511 through first-active; end candidates SHALL span last-active through
last-active plus 511. Rise and release widths SHALL span 3 through 512 frames.
All complete candidate pairs SHALL preserve the entire reference window between
their ramps. Censored boundaries and unresolved search-boundary optima SHALL refuse.

Joint stereo I/Q least squares SHALL estimate the common complex multiplier.
Global and each edge-local relative RMS SHALL separately satisfy the unchanged
1e-4 source-fit policy. Numerical rank and the 1e6 condition bound SHALL apply to
the plateau I/Q design and local six-parameter identifiability design. Absolute
source-domain uncertainty SHALL NOT be inferred from condition number alone.
Sufficient-statistic residuals SHALL receive conservative numerical allowance;
final source residuals SHALL be recomputed directly against all original PCM.

The search SHALL retain all feasible candidates within fixed resource limits of
4096 survivors per edge, 2000000 paired evaluations and batches of at most 512.
Exceeding a limit or leaving ambiguity search incomplete SHALL refuse, rather than
selecting a best subset. All feasible onset, rise-end, release-start and end
landmarks SHALL span at most one source frame, and normalized complex multipliers
SHALL agree within 1e-3, before one representative source fit is published.
This source-grid resolution SHALL NOT replace the original acoustic timing gates.
Additional work SHALL fit within the existing 256MiB guard before allocation,
including a conservative 128 bytes per input frame plus 64MiB bounded workspace.

The result SHALL explicitly report conditional isolated-model validity and offline
noncausality, without asserting unique physical interpretation or a bound on unknown
latent model error. Every source sample outside the fitted gate and every fitting
error SHALL remain in the exact fully transformed residual; fitted endpoints SHALL
NOT crop that residual. Refusal SHALL preserve the prior v8 signal path. Prior
variants, complete finite tails, existing unsupported domains and every original
full-response acceptance gate SHALL remain available and unchanged.

Independent unseen finite gates and negative underresolved, carrier-zero, scaled,
truncated, out-of-domain, no-plateau and ambiguous cases SHALL precede the full
unchanged source/profile matrix. Such controls SHALL NOT certify production,
variable-rate maps, realtime execution or any remaining Human/product gate.

#### Scenario: A conditional finite gate is identifiable in original stereo PCM
- **GIVEN** one certified positive-AM family, an interior plateau restriction and a complete bounded source-grid search
- **WHEN** all feasible direct source fits agree on gate landmarks and global complex amplitude/phase within the declared resolution
- **THEN** the separate experiment SHALL publish that source model and retain every complementary PCM sample and tail
- **AND** incomplete, ambiguous, censored or ill-conditioned fits SHALL retain the prior signal path without manufacturing acoustic acceptance

### Requirement: Independent Finite Carrier Gates Use A Complete Joint Source Fit
The offline diagnostic SHALL offer a separate opt-in extension for at most two
already source-certified stationary carriers with independent integer raised-cosine
gates. Each gate SHALL have one onset, plateau and end, with rise/release widths
3 through 512. The joint real I/Q design SHALL include every cross-carrier term.
One optional nuisance frame SHALL have two unrestricted real stereo amplitudes;
its position SHALL be determined from original PCM, never a fixture identifier,
known event coordinate, requested output or scorer. This is a conditional sampled
source model, not physical source identification or productive backend acceptance.

A source-only 8192-frame reference SHALL be checked in sixteen 512-frame bins.
Every proper carrier subset's least-squares error after arbitrary deletion of
at most one whole stereo frame SHALL exceed the global feasible error budget in
every bin before a common central 2048-frame plateau is certified. All permitted
central-frame deletions SHALL contribute to a conservative joint I/Q enclosure.
Ordinary activity traces, recurrence proposals and successful optimizers SHALL
NOT replace these certificates or justify local edge brackets.

The search SHALL examine the complete finite source grid using conservative
interval lower bounds, including arbitrary nuisance-frame removal. It SHALL
retain every candidate not proved infeasible and account for all feasible joint
models. It SHALL use at most 4096 surviving anchors and edges per carrier side,
2000000 width evaluations per side, 2000000 joint edge combinations/pairs,
4096 direct model/support confirmations and 64-width batches.
Each joint flank's stored survivor list SHALL separately be bounded to 4096
entries before append, including simultaneous onset and release storage.
Any exhausted cap,
unresolved numerical bound or incomplete search SHALL refuse; best-subset
selection SHALL NOT be presented as complete ambiguity analysis. Before allocation,
additional working memory SHALL be bounded by 512 bytes per input frame plus
96MiB, within the existing 256MiB limit.

Joint least squares SHALL consider every possible nuisance frame and the
no-nuisance case before direct original-source confirmation of surviving models.
The coefficient enclosure SHALL include every vector satisfying each surviving
gate/support model's global budget, not only its least-squares representative.
When a global least-squares representative fails an edge check, rejection SHALL
require an independent conservative edge-infeasibility bound. Unresolved joint
coefficient feasibility SHALL refuse the experiment. The geometry condition
check SHALL cover the retained coefficient enclosure using a conservative
perturbation bound rather than certify only one representative phase.
Global and every 1025-frame onset/end neighborhood SHALL separately meet the
unchanged relative RMS limit 1e-4. Only the explicit nuisance contribution MAY
be subtracted for evaluating model error; normalizers SHALL use original PCM.
The full joint I/Q design and amplitude-invariant nuisance-projected 6K geometry
Jacobian SHALL satisfy rank and condition 1e6. Every feasible gate's onset,
rise-end, release-start and end SHALL agree within one source frame, and its
source-normalized complex coefficients within 1e-3. Feasible source/ramp boundary
contact, censored observations and unresolved support ambiguity SHALL refuse.
Final physical coefficient rescaling SHALL refuse nonfinite or zero amplitudes
before publishing modes, including finite-PCM cases whose modal amplitude exceeds
the representable float64 range.

Each admitted carrier SHALL retain its own fitted onset as phase birth and its
own global complex I/Q phase. The prepared residual SHALL remain original PCM
minus the entire tonal reconstruction, including the nuisance and all other
complementary samples. Every residual sample and generated tail SHALL be
transformed. Refusal SHALL preserve the previous v9 signal path. Previous variants,
original sources, targets, criteria and all remaining acceptance gates SHALL
remain unchanged. Conservative floating allowances SHALL be explicit; they SHALL
NOT be described as universal floating-point or libm certification.

Independent source controls SHALL cover staggered/shared gates, phase/translation/
scale and stereo variation, missing or weak modes, impulses at plateau/edge,
multiple transients/noise, non-raised-cosine/censored sources and actual cap
exhaustion before the unchanged full source/profile measurements.

#### Scenario: An impulse accompanies independently gated stationary carriers
- **GIVEN** a source-only reference and a complete joint integer-gate search with at most one arbitrary stereo nuisance frame
- **WHEN** every feasible directly confirmed model agrees within the unchanged source resolution and satisfies conditioning and residual checks
- **THEN** each carrier SHALL retain its independent onset and complex phase, while the impulse remains in the fully transformed exact remainder
- **AND** ambiguous, unsupported, numerically unresolved or incomplete cases SHALL retain the previous signal path without claiming acoustic or product acceptance

### Requirement: Source Group Magnitude Is Evaluated As An Explicit Amplitude Change
The offline experiment SHALL evaluate a separate source-group magnitude candidate
using the complete already admitted stationary same-spatial-direction carrier
set. Source PCM and its existing certificates SHALL determine group eligibility;
fixture labels, requested output targets, fitted phase offsets, close-frequency
cutoffs and adjustable component weights SHALL NOT select the group.

At each declared source coordinate s, Z(s) SHALL be the sum of the existing
zero-extended complex-linearly sampled analytic modes. V(s) SHALL be the sum of
the same samples with their existing independent pitch-phase corrections.
The candidate SHALL synthesize Re(norm(Z(s))*V(s)/norm(V(s))) with one shared
complex-stereo norm. Complex interpolation SHALL precede the source norm so that
combined h=r identity retains the existing fractional sampling contract.
This is a deterministic time-varying analytic amplitude replacement. It SHALL
NOT be described as unchanged gain or fitted measured-output compensation.

When Z and V are both zero, the grouped contribution SHALL be zero. A positive
source magnitude with zero or numerically unresolved transported phase SHALL
refuse explicitly; an epsilon denominator, clipped gain or erased frame SHALL
NOT conceal that failure. Working memory and final scaling SHALL be bounded
before publication; nonfinite, underflow-lost or unrepresentable results SHALL
refuse. Common binary normalization SHALL retain every accepted input component
exactly, including values that would round up from subnormal to normal at the
scaling boundary; adjacent nonzero cancellation SHALL NOT become a false zero.
Inputs SHALL remain unchanged.

The complete transformed exact residual, including nuisance samples and all
generated tails, SHALL remain in the response. Existing carrier-domain refusals,
source clock, legacy variants and acoustic criteria SHALL remain unchanged.
Independent identity, phase-singularity, scaling and stereo controls SHALL
precede the frozen close-pair and independently gated900/1100Hz comparisons.
The experiment SHALL report generated spectral peaks and actual full-response
15-cent/2-ms results. Analytic magnitude preservation SHALL NOT be presented as
proof of real-PCM energy timing, alias safety, realtime or product acceptance.

#### Scenario: A source group has a defined transported phase
- **GIVEN** a complete eligible source group and its unchanged residual
- **WHEN** source magnitude and independently transported phase define a finite grouped contribution on the entire capture
- **THEN** the experiment SHALL measure the original whole-response criteria and retain every residual tail
- **AND** unresolved phase, numeric failure or failing tonal/timing measurements SHALL remain explicit negative evidence

### Requirement: Residual Energy Attribution Keeps Complete Band And Segment Interference
The offline noise audit SHALL retain the existing complete finite-bank renderer,
source cues, exact-zero trimming, source coordinates, capture and every tail.
It SHALL audit the unchanged noise source at rate2 with -7,0,+7 semitones and
the h=r=2 identity control without changing synthesis parameters or acoustic
criteria. Its independently accumulated whole output SHALL agree with the
existing renderer under the declared numerical reconstruction bound.

For each band the audit SHALL sum all of that band's segment contributions in
the global capture before squaring. It SHALL define nonnegative autoenergy A as
the sum of those whole-band squared stereo norms, whole energy E as the squared
norm of their sum and signed cross energy C=E-A. Per-segment squares SHALL NOT
be mislabeled as whole-band autoenergy. Source-grid and h=r reconstruction,
source complex-coefficient autoenergy and complete support SHALL be checked.
The audit SHALL use bounded sequential band storage rather than a complete
band-by-time PCM matrix.

The existing centered smoothing and quantiles SHALL apply only to nonnegative
A,E and dry energy. Signed C SHALL NOT receive quantiles or a normalized CDF.
At the fixed canonical dry quantile times, the audit SHALL report signed linear
cumulative balances for A,E,C with the same dry-energy denominator and check
that the whole balance equals the sum of auto and cross balances within an
explicit numerical bound. The h=r control SHALL remain a separate comparison.
Energy attribution SHALL NOT alone establish an allpass delay, a replacement
synthesizer, realtime safety or acceptance of the still-failing audio backend.

#### Scenario: Segment contributions overlap within one band
- **GIVEN** two segment contributions cancel in the same band's complete capture
- **WHEN** the audit measures that band's autoenergy and the bank's cross energy
- **THEN** it SHALL sum the segment PCM before squaring and retain the cancellation inside the band
- **AND** only interference between the resulting whole bands SHALL enter the signed cross term

### Requirement: Joint Residual Phase Is Evaluated Before Independent Band Rotation
The separate offline candidate SHALL form the complete complex sum of the
existing finite bank before extracting its single pooled reference phase.
All source-segment and band contributions SHALL share this global field; source
cues SHALL NOT reset its phase. The candidate SHALL retain the existing principal
negative-pi tie, exact-common-zero run and birth semantics with reference omega0.

For N=1024 the field SHALL equal Q=x+i*(x*q), where q[l]=2*w[l]/N*cot(pi*l/N)
at odd l and zero at even l, with w[l]=(.5+.5*cos(pi*l/(N-1))) and the existing
exact final zero tap. Independent controls SHALL compare this algebraic kernel
with the complete original bank sum. Convolution SHALL preserve structural zero
parity and support; FFT roundoff or an amplitude threshold SHALL NOT invent or
erase activity at a theoretically zero impulse tap.

The frozen output SHALL be Re(L(Q,s)*exp(i*(h/r-1)*L(Psi,s))), where Psi is the
existing pooled elapsed phase of the global field and s is the existing source
coordinate. Complex interpolation SHALL precede phase action. This is one common
rotation, not a fitted gain, mode weight, output delay or magnitude replacement.
The complete nominal silence, predetermined signed capture and finite generated
tails SHALL remain. Invalid or unbounded input/allocation SHALL refuse explicitly.
Input ownership and all existing production paths SHALL remain unchanged.

The first comparison SHALL use the unchanged noise, original impulse and
mono900.37Hz sources, each at r1/k0 and r2/k-7,0,+7,+12, with the existing
target-blind tonal, 2-ms, prefix, peak, retention and complete-tail criteria.
The mono source SHALL remain necessary because even-born impulse decimation can
skip the quadrature tail. A failed first comparison SHALL remain negative evidence,
without output fitting or a larger parameter search. Shared relative band phase
SHALL NOT be presented as proof of real-energy timing, alias safety, finite
causality, realtime resources or product acceptance.

#### Scenario: An impulse has algebraically zero quadrature taps
- **GIVEN** an isolated source impulse and the complete declared finite kernel
- **WHEN** the joint source field and its phase runs are constructed
- **THEN** even quadrature lags and the final tap SHALL remain exactly zero without a threshold
- **AND** the unchanged tonal source SHALL exercise the joint phase action separately from the decimated impulse

### Requirement: Symmetric Quadrature Is A Separate Signed-Support Experiment
The offline comparison SHALL separately evaluate the antisymmetric mirror of the
already fixed causal quadrature kernel. Positive taps and window SHALL remain
unchanged, negative taps SHALL be their negative mirror and zero/even/final
plus-or-minus1023 taps SHALL remain structurally zero. This new Qsym=x+i*(x*qs)
SHALL NOT be described as the original complete513-band sum.

The field SHALL retain its declared source origin -1023 and all positive and
negative finite support. Complex sampling followed by the same single pooled
phase action SHALL use the declared source coordinate and field origin.
Source cues, fixture labels and measured targets SHALL NOT alter synthesis.
All nominal silence, pre-event samples and complete tails SHALL be scored.
A fitted delay, output crop or hidden causalizing shift SHALL NOT remove
lookahead or retention failures. The1023-source-frame lookahead SHALL remain an
explicit offline limitation, with no finite live-context or realtime acceptance.

Existing immutable ownership, bounded-allocation preflight, work/range refusals
and independent scalar controls SHALL be reused and extended to signed support.
Controls SHALL independently check antisymmetric impulse taps, scalar two-sided
convolution, whole phase/sampling, fractional identity, full capture bounds,
caller mutation and actual resource refusals. There SHALL be no gain, window,
weight, cutoff or fitted phase adjustment.

The first comparison SHALL keep the same unchanged noise, impulse and
mono900.37Hz sources and fifteen requests. The original target-blind15-cent,
2-ms, prefix, peak, retention and complete-tail predicates SHALL remain.
Finite-event measurements SHALL determine the result; marginal properness or
an ensemble circularity argument SHALL NOT substitute for those measurements.
The causal predecessor and all production paths SHALL retain their behavior.
If the first comparison passes, the same frozen candidate SHALL be evaluated
with the existing eighteen sources and four profiles, including the unchanged
target-blind close-pair ranking, unrelated stereo and near-Nyquist input.
Every requested full response and negative result SHALL remain visible; the
comparison SHALL NOT invent an aliasing admission guard or claim realtime,
finite-context, acoustic or product acceptance from the first fifteen passes.

#### Scenario: Symmetric preparation generates pre-event quadrature
- **GIVEN** the mirrored finite kernel can use future source samples
- **WHEN** the joint phase action generates real PCM before the source event
- **THEN** every generated pre-event sample SHALL remain in the original retention measurement
- **AND** a failure SHALL remain negative evidence without a fitted delay or crop

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
