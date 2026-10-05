## ADDED Requirements

### Requirement: Rubber Band Cold State Is Prepared Outside The Callback
The system SHALL construct, reset and silence-warm reusable Rubber Band LiveShifter state outside
the audio callback, including its allocating initial pitch setup and first processing call.

Each voice SHALL own a unique active handle and use a bounded prepared reserve exchange. Callback
start, retrigger, seek, stop and bypass transitions SHALL invalidate local fixed buffers without
calling native reset. Steady ratio updates SHALL use an already warmed uniquely owned handle.
The system SHALL report preparation-worker startup failure rather than silently starting without
replenishment. Recycled native state SHALL be reset and destroyed only outside callback rendering.

#### Scenario: Retrigger adopts a warm reserve
- **GIVEN** a shifted voice has a prepared reserve
- **WHEN** the voice is retriggered and renders shifted audio again
- **THEN** it adopts that reserve and transfers the old handle to non-audio preparation
- **AND** the callback does not construct, native-reset, cold-update or destroy native state

#### Scenario: Rapid discontinuities exhaust the reserve
- **GIVEN** a voice has no ready reserve or its recycle lane is full
- **WHEN** shifted rendering needs fresh state
- **THEN** it retains ownership of its invalidated handle and fills shifted output with silence
- **AND** it does not wait, lock, allocate, reset native state or advance the transport to hide delay
- **AND** bypassed varispeed audio can still render through fixed buffers

#### Scenario: Unavailable preparation worker prevents startup
- **WHEN** the native preparation worker cannot be started
- **THEN** audio-engine startup reports that failure outside the callback

### Requirement: Rubber Band Adapter Delay Is Independent Of Callback Partitions
The system SHALL use an explicit fixed adapter delay of one native block minus one output frame
for an initialized shifted stream, independent of the bounded callback segment sequence.

Native algorithmic delay, adapter delay and CPAL device-buffer estimates SHALL be reported as
distinct domains. Silence preparation and nominal dynamically changed delay SHALL NOT be claimed
as exact source-content pre-roll or audible hardware synchronization.

#### Scenario: Unequal segment sizes retain the adapter timeline
- **GIVEN** identical varispeed samples are supplied to warmed shifted processors
- **WHEN** one stream uses regular segments and another uses irregular segments
- **THEN** their adapter output sequences have the same fixed delay and no inserted underflow gaps
- **AND** their native processing blocks consume identical sample sequences

#### Scenario: Delay accounting preserves musical state
- **WHEN** DSP and adapter delay are measured
- **THEN** transport time, source loop ownership, persisted markers and input timestamps remain unchanged
- **AND** device precision is limited to the measured or documented device-clock evidence

### Requirement: Stem Selection Preserves Key Lock Processing History
The system SHALL preserve active Rubber Band history and adapter FIFOs while applying the existing
source-domain stem-selection crossfade at a common source address.

#### Scenario: Stem mask changes during Key Lock playback
- **GIVEN** a voice is playing prepared stems with Key Lock enabled
- **WHEN** an accepted stem mask or full-mix selection changes
- **THEN** the same voice processor consumes the crossfaded source without fresh cold state
- **AND** source position, transport phase and loop ownership remain unchanged

### Requirement: Fractional Source Rendering Is Independent Of Callback Partitions
The system SHALL generate dry varispeed samples and Rubber Band input from one canonical
fractional source-frame timeline for full-mix and prepared-stem playback in every lock mode.

Identical immutable sources, accepted controls at identical active output-frame positions,
loop/seek policy and starting source state SHALL produce identical source sample sequences and
next fractional source positions under bounded regular, irregular and one-frame partitions.
The timeline SHALL retain the fractional source remainder across segment boundaries and rate
changes instead of independently rounding each segment's source-frame count. Source progression
SHALL use the actual bounded native `f32` tempo ratio promoted to `f64` and the number of active
output frames elapsed in a rate epoch. Integer playhead telemetry SHALL floor the next source
cursor without changing persisted loop markers, transport phase or launch scheduling.

Linear interpolation SHALL resolve both neighboring source reads through the common half-open
loop and explicit before-loop/after-loop seek policy. Source selection and source-domain stem
transition gains SHALL use the same fractional progress in every channel and partition. Rate
rebases, pause/resume and in-range live loop edits SHALL preserve the fractional cursor; explicit
seek, retrigger and out-of-range loop edits SHALL retain their existing source-position policy.
Rendering SHALL reuse accepted immutable buffers and preallocated storage without callback
allocation, blocking, disk I/O, logging, Python/GIL access or unbounded work.

#### Scenario: Fractional playback survives irregular partitions
- **GIVEN** an immutable nonconstant source and a bounded fractional tempo ratio
- **WHEN** the same active output duration is rendered with fixed, irregular and one-frame segments
- **THEN** dry rendering and the source samples supplied to Rubber Band are identical
- **AND** the next fractional source cursor is identical without cumulative segment rounding

#### Scenario: Interpolation follows loop and explicit seek boundaries
- **GIVEN** a configured half-open loop and an explicit seek before or after that loop
- **WHEN** fractional source reads cross the loop start, loop end or track end
- **THEN** both interpolation taps follow the shared intro, tail and wrapping policy
- **AND** callback boundaries do not change the selected source samples

#### Scenario: Stem selection uses the same fractional source path
- **GIVEN** compatible prepared stems and a source selection or stem-mask transition
- **WHEN** the source is rendered at a fractional rate through different callback partitions
- **THEN** all enabled stems and both transition sides use common source addresses and progress
- **AND** an equivalent prepared-stem sum retains the full-mix source timing

#### Scenario: Pause and source rebases retain the fractional remainder
- **GIVEN** a voice cursor contains a fractional source remainder
- **WHEN** the voice is paused and resumed, its rate changes, or an in-range loop edit is accepted
- **THEN** the next active render retains that fractional source remainder
- **AND** paused output frames do not advance source playback

### Requirement: Tempo Smoothing Uses Active Output Frames
The system SHALL apply the existing per-voice maximum tempo-ratio step of `0.05` on intervals of
`512` active output frames in dry and Key Lock modes, independent of callback partitioning.

A newly accepted target SHALL initiate its first bounded step at its accepted active output-frame
position. Subsequent steps SHALL occur after each fixed active-output-frame interval until the
target is reached. Rendering SHALL split bounded source/native work at rate-step boundaries so
the same controls produce the same source ratios under different callback partitions. With
equivalent initialized native/preparation state and prepared-reserve availability, native
pitch-update order SHALL also be identical. Source-feed equality SHALL NOT depend on reserve
availability; missing native reserves SHALL retain the existing bounded silence fallback.
Paused output SHALL NOT advance the smoothing interval.

#### Scenario: Unequal segments retain the same rate-change timeline
- **GIVEN** two streams accept the same tempo targets at the same active output frames
- **AND** their initialized native/preparation state and prepared-reserve availability are equivalent
- **WHEN** one stream uses regular segments and the other uses irregular or one-frame segments
- **THEN** both streams apply the first and subsequent smoothing steps at the same active frames
- **AND** their canonical source feed and native pitch-update order remain identical

#### Scenario: Pause freezes a pending smoothing step
- **GIVEN** a voice is partway through a smoothing interval
- **WHEN** playback is paused and later resumed
- **THEN** the remaining active output frames before the next step are preserved
- **AND** silence rendered while paused does not consume the interval

### Requirement: Exact Source Preparation Has An Independent Non-Realtime Proof Gate
The system SHALL verify exact-ratio source preparation outside the audio callback against an
independently resampled contiguous native reference before enabling source-prepared live adoption.

The proof SHALL reuse the live fractional source policy for its preparation feed, preserve the
caller's logical source state, and retain distinct logical and future feed cursors together with
native state and fixed input/output FIFOs. It SHALL apply the actual native `f32` inverse-pitch
conversion before reset and before the first source shift. Prepared output plus continuation
SHALL equal the separate raw native reference at an explicitly declared discard index, including
nonaligned discards, fractional ratios, loop/intro/tail addressing, stereo and prepared stems
under regular, irregular and one-frame partitions. Preparation bounds SHALL reject invalid source
layouts, unsupported ratios, excessive discards and render sizes without reading outside buffers.

The proof SHALL report uncropped onset, 1%-of-peak onset and peak translated by the discard index,
discarded response energy and startup-peak clipping as well as retained-window metrics. Nominal
API delay and exact reference equality SHALL NOT be treated as audible synchronization evidence.
This test-only gate SHALL NOT activate live state, alter transport, move markers or change the
existing reserve-starvation behavior. Asynchronous identity, on-time handover, off-thread
retirement and click-safe transitions SHALL remain separate acceptance gates.

#### Scenario: Prepared output continues a contiguous exact-ratio reference
- **GIVEN** immutable source buffers, a constant fractional ratio and an explicit discard index
- **WHEN** preparation retains native output and continues under unequal render partitions
- **THEN** every retained and continued stereo sample equals the independently generated reference slice
- **AND** preparation advances only future feed while output advances the separate logical cursor
- **AND** no second silent adapter lead or partition-dependent gap is inserted

#### Scenario: Nominal discard clips a startup response
- **GIVEN** a startup transient has a response peak before the experimental discard index
- **WHEN** the non-realtime response is measured
- **THEN** the proof reports that clipped peak, discarded energy and uncropped translated residuals
- **AND** a finite retained response does not count as audible launch acceptance

#### Scenario: Invalid preparation bounds are rejected
- **WHEN** a proof request has invalid layout, ratio, discard arithmetic or render capacity
- **THEN** it returns a bounded explicit error without reading outside accepted source buffers
- **AND** an oversized render request leaves the prepared state available for a valid continuation

### Requirement: Source History Has Explicit Forward Provenance
The system SHALL prove non-live source preparation from an explicitly declared earlier source
origin and bounded output-frame history before the requested logical source phase.

The constant-ratio history origin SHALL advance through the canonical loop/intro/tail policy to
the requested logical frame, fractional remainder and seek mode. The proof SHALL reject a
mismatched history or, for nonzero history, an active source-selection transition whose earlier state was not supplied.
It SHALL preserve the caller's logical cursor and markers. History length H and raw native discard
D SHALL remain separate coordinates; retained output SHALL equal the independent raw reference
suffix at D, and its effective source-relative translation SHALL be D-H. Zero history SHALL
preserve the existing proof. Bounds SHALL be checked before native pitch/reset/source processing.

#### Scenario: Earlier intro or tail history reaches a looping logical phase
- **GIVEN** an explicit origin in the intro or tail and a bounded history at an exact ratio
- **WHEN** that origin advances into the loop before the requested logical phase
- **THEN** preparation reads the declared intro or tail exactly once before looping
- **AND** logical playback starts at the requested phase while the native feed continues from the history origin

#### Scenario: Ambiguous or unsupported history is rejected
- **WHEN** declared history does not reach the requested fractional phase or nonzero history contains an active stem transition
- **THEN** the proof returns an explicit error before processing native source content
- **AND** it does not infer a backward loop, intro, tail or transition path from the logical phase

### Requirement: Musical Timing Candidates Use Uncropped Content Evidence
The system SHALL evaluate source-history discard candidates with uncropped impulse, short-tone
and percussion-burst evidence before choosing live audible compensation.

The non-live engineering criterion SHALL use stereo energy-envelope 10% and 50% cumulative-energy
times relative to the actual independently resampled dry reference, with an exploratory two-ms
maximum absolute residual for each. A candidate SHALL also retain the original response peak,
discard at most 0.1% of target-response energy, and leave at most 0.0001% of measured energy in the
last 20 ms of the capture. This budget SHALL be identified as an engineering gate, not a universal
perceptual threshold. Absolute/1%-peak onset, peak, 90% energy time, retained metrics and cut/join
discontinuity SHALL remain visible alongside the criterion. The proof SHALL evaluate block phases,
history lengths and 44.1/48/96 kHz ratios without fitting separate compensation to each marker or
signal. Failure SHALL remain reported as failure; cropping, a fitted signal-specific index, or
exact reference equality SHALL NOT establish live/device acceptance.

#### Scenario: A retained response hides an early lost attack
- **GIVEN** the original response begins or peaks before a candidate discard
- **WHEN** retained output alone appears near the target
- **THEN** the report still records uncropped residuals, lost energy and the original peak
- **AND** the candidate fails the content criterion if its declared retention bounds are exceeded

#### Scenario: No single candidate meets every fixture
- **WHEN** the tested history, marker phases and burst types have incompatible timing or retention bounds
- **THEN** the proof records the failed common compensation gate
- **AND** live adoption and synchronized Quantize remain pending
