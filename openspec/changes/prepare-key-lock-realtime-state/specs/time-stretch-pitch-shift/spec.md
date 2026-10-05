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
