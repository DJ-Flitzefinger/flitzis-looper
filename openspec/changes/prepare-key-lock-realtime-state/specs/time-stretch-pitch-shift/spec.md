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
