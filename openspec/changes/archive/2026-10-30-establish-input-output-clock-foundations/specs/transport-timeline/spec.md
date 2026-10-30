## ADDED Requirements

### Requirement: Output Clock Observations Map Estimated Device Time
The system SHALL maintain bounded Rust output-clock observations relating an absolute output frame
to estimated audible-device time in the shared engine timestamp epoch.

The mapping SHALL combine a shared-epoch callback-entry observation with checked CPAL
callback-to-playback timestamp duration, sample rate and current transport grid metadata. CPAL
stream timestamps SHALL NOT be treated as Rust or Python monotonic timestamps. Observations SHALL
carry validity/freshness information. Missing, stale, overflowing, reversed or discontinuous
observations SHALL make mapping unavailable, without resetting or reanchoring transport or voices.

Snapshot publication SHALL use fixed-size state without callback locks, allocation, logging or
unbounded retries. Control-side reads SHALL either return one coherent snapshot or report
unavailability. These estimates SHALL NOT be presented as measured hardware sample accuracy or
include unmeasured DSP delay compensation.

#### Scenario: Device buffering is represented
- **GIVEN** CPAL reports a valid playback timestamp later than its callback timestamp
- **WHEN** Rust observes the first frame of the callback buffer
- **THEN** its estimated audible time includes the reported buffering duration
- **AND** the output-frame clock remains unchanged

#### Scenario: Unusable observation does not move audio
- **WHEN** a clock observation is stale, reversed or discontinuous
- **THEN** captured-input mapping is unavailable for that observation
- **AND** transport and active voice timing remain unchanged

#### Scenario: Concurrent reader gets coherent state or no result
- **WHEN** a control reader overlaps snapshot publication
- **THEN** it receives one complete observation or an unavailable result
- **AND** the callback performs no reader-dependent retry or blocking wait

### Requirement: Captured Input Target Diagnostics Use Nearest Grid Boundary
The system SHALL calculate a diagnostic quantization target from a valid captured input timestamp,
an available output-clock mapping and the permanent Rust transport grid in a focused Rust timing
implementation.

The supported diagnostic subdivisions SHALL retain `1/16`, `1/32` and `1/64`. The target SHALL be
the nearest selected-grid boundary to the captured input time, with exact midpoint ties selecting
the future boundary. Processing delay and callback partitioning SHALL NOT replace captured input
time with processing time. Invalid or unavailable timing inputs SHALL return no diagnostic target.
This foundation diagnostic SHALL NOT activate nearest-boundary playback or source catch-up.

#### Scenario: Delayed input retains its chosen target
- **GIVEN** one captured input time and an unchanged valid master grid
- **WHEN** the target is calculated after different dispatch delays or callback partitions
- **THEN** equivalent valid clock observations produce the same target frame

#### Scenario: Exact midpoint chooses the future boundary
- **WHEN** captured input time maps exactly halfway between selected-grid boundaries
- **THEN** the diagnostic target is the later boundary

#### Scenario: Missing timing reports no target
- **WHEN** the timestamp, master grid or clock mapping is unavailable or invalid
- **THEN** the diagnostic returns no target
- **AND** it does not silently recapture or reround at processing time

### Requirement: Timing Foundations Preserve Current Launch Execution
The system SHALL retain current immediate and future-grid loop-start launch execution while
introducing timestamp propagation and device-clock diagnostics in this foundation slice.

Timestamp fields and diagnostic calculations SHALL NOT seek the source, alter persisted loop
markers, compensate DSP latency or change the existing master bootstrap/anchor policy.

#### Scenario: Stamped launch retains current scheduling
- **WHEN** a valid captured timestamp accompanies a play request
- **THEN** playback uses the same current scheduling and effective loop start as an unstamped request
- **AND** the original timestamp remains attached to its scheduled command
