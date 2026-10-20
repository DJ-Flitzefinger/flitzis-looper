## ADDED Requirements

### Requirement: Active immediate piano retrigger retains its original past target
The system SHALL admit an active constant-rate piano Retrigger whose original target T precedes callback frame F only when a fixed callback-owned observation certifies a contiguous actually rendered active interval or an actually observed stationary paused-control interval covering [T,F] under the same source, selection, Normal loop domain, constant rate and source-control revision. The observation SHALL extend only across exact adjacent output intervals and matching logical source progression. Pause, gaps, missing source reads, clock discontinuity or changed source/control contracts SHALL invalidate the active proof. Paused observations SHALL remain distinct from rendered observations, SHALL claim no PCM read and SHALL require physically available immutable source/readers, a stationary logical checkpoint and an unchanged constant rate matching the current mixer target. Paused pending rate smoothing or an active stem transition SHALL NOT be inferred as constant history. Neither kind SHALL certify T at its first output boundary, whose ordering against a same-frame control change is unavailable.

The new independent launch SHALL retain original T and emit its complete selected physical beginning at actual E without consuming E-T as unheard source or filter output. Its source phase SHALL count actually emitted frames from that beginning. The admitted actual rate, rate target and remaining rate-step clock at E SHALL remain independent of this phase reset; neither the final target nor a reset smoothing interval SHALL substitute for the actual accepted rate state. Source rate/BPM, pitch intent, own Native/FIFO custody and the old voice until E SHALL remain preserved. Once [T,F] is certified, an independent later voice birth or matching Hold release SHALL NOT revoke the new launch merely by changing the predecessor voice's lifetime. Existing content-control/action guards SHALL still reject incompatible source, loop, rate or selection changes. This full-onset revision supersedes this change's former T-born suffix recipe; current scalar admission proof remains mandatory and SHALL NOT be replaced by guessed history.

The callback SHALL use bounded fixed scalar work only; it SHALL NOT allocate a new source/PCM journal, perform preparation, block, access Python or shift T to F/E.

#### Scenario: A real unquantized piano press arrives just after its target
- **GIVEN** the selected pad has continuously rendered a constant valid source
- **WHEN** a piano Retrigger captured at T is processed at F>T within that interval
- **THEN** the original action can prepare and execute once at its existing E
- **AND** its complete emitted beginning agrees with an independently prepared
  full beginning under the same admitted rate state, including nonunity rates and irregular partitions
- **AND** its original T and truthful E remain distinct without claiming original-T source-phase alignment
- **AND** both start policies retain their own original action contract

#### Scenario: A piano retrigger starts a previously paused pad
- **GIVEN** real callbacks have observed a stationary paused source/control contract
  beginning before the original press T and continuously through F
- **WHEN** the real piano press is processed after T
- **THEN** a new independent Normal launch may use that certified control interval
- **AND** it retains T and emits the complete new beginning at E rather than resuming the paused source position or preparing unheard E-T output
- **AND** it leaves other pads intact and does not infer a paused pending glide as supported history
- **AND** a pause/control change at the very first ambiguous boundary remains unavailable

#### Scenario: The missed interval crosses an uncertified control or playback gap
- **GIVEN** T<F but pause, a rate/loop/source/selection change or a read gap makes
  the interval unavailable under one constant contract
- **WHEN** the original retrigger is considered
- **THEN** it is honestly refused without modifying the prior voice or timestamp
- **AND** the implementation does not infer missing history from matching endpoints

#### Scenario: A past or paused glide has no supported admission proof
- **GIVEN** an original press would require an unproved past-T glide, paused pending rate smoothing or an unsupported active selection transition
- **WHEN** its full new beginning is considered
- **THEN** the action SHALL remain a whole-action refusal without changing the old audio or original T
- **AND** full-onset semantics SHALL NOT create missing rate/control history or waive existing source, ABA, cancellation and reader guards

### Requirement: Exclusive retrigger reserves every voice generation increment
An exclusive retrigger SHALL admit only when its selected destination can represent every required stop and new-start generation increment without saturation or reuse and every other active voice can represent its own stop increment. Exhaustion SHALL reject the whole action before stopping another PAD, resetting DSP, changing source placement or reporting effective feedback. A nonexclusive retrigger SHALL require only its actual increment count.

#### Scenario: Active exclusive destination has only one generation remaining
- **GIVEN** an exclusive retrigger would stop and restart an active destination at generation MAX minus one
- **WHEN** its complete effect is considered
- **THEN** sequence exhaustion rejects it before any audible or ownership mutation
- **AND** a destination with two remaining generations or a one-increment nonexclusive start retains its valid boundary behavior

#### Scenario: Another active PAD has no stop generation remaining
- **GIVEN** the destination can restart but another active PAD is already at generation MAX
- **WHEN** an exclusive retrigger would stop that PAD
- **THEN** the whole retrigger rejects without stopping either PAD or resetting either filter
