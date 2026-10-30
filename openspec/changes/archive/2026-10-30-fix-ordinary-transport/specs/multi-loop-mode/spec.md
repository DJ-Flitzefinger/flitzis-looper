## ADDED Requirements

### Requirement: Single mode transport uses the last effective explicit target

With MultiLoop OFF, ordinary START/STOP SHALL use one valid source/content-bound
last effective explicit playable PAD choice. Editor selection and callback arrival
SHALL NOT redefine it. Original global order SHALL resolve equal effect frames.
STOP SHALL retain valid history; unload/replacement SHALL invalidate it and
Move/Swap SHALL follow content. Only an explicit START without valid history MAY
use a loaded selected PAD. Recording group starts and STOP ALL SHALL remain global.

#### Scenario: Stop and restart do not resurrect a group
- **WHEN** A, B and C have been explicitly played in that order and MultiLoop is OFF
- **THEN** ordinary right STOP and the next START SHALL affect only C
- **AND** editor selection of A SHALL NOT change that target

#### Scenario: Old pending starts are fenced without stopping peers
- **WHEN** OFF STOP is admitted while an older ordinary start remains pending
- **THEN** that original SHALL NOT play after STOP
- **AND** already active non-target peers SHALL retain their pause/Hold owners

#### Scenario: No effective target exists
- **WHEN** OFF right STOP is admitted without a valid effective target
- **THEN** it SHALL fence old starts with zero audible stop entries
- **AND** it SHALL NOT become STOP ALL or start editor selection

### Requirement: Disabling MultiLoop is one admitted playback transition

ON-to-OFF SHALL admit the complete runtime image, old launch/group fences and
source-bound peer stops together. It SHALL retain an actually playable latest
target without retrigger/resume. Fallible admission SHALL precede mutation, and
accepted work awaiting callback completion SHALL remain pending. Later starts
SHALL NOT overtake the transition's peer stops.

#### Scenario: Three PADs become one without a new attack
- **WHEN** three PADs play and MultiLoop is disabled
- **THEN** peers SHALL stop while the last qualified playable target keeps its voice position
- **AND** no new attack, generation or master phase SHALL be introduced

#### Scenario: Admission refuses or target is not playable
- **WHEN** transition resources are unavailable
- **THEN** the prior mode, voices and fences SHALL remain unchanged
- **WHEN** an admitted transition has no playable target
- **THEN** it SHALL NOT resume a paused, stopped or held target

### Requirement: BPM Lock remains independent with a stable master owner

BPM Lock SHALL be toggleable with either MultiLoop state. Selection, explicit PAD
playback and mode changes SHALL preserve the same qualified reference and
established master timeline. Hierarchy-only requalification SHALL retain that
owner. An empty captured reference MAY bind its first qualified content; source
replacement SHALL NOT silently bind a new source by slot.

#### Scenario: Changing PAD or mode preserves master
- **WHEN** Lock has an established qualified master and the performer changes PAD or MultiLoop
- **THEN** requested Lock, reference owner, master tempo and timeline epoch SHALL remain established
- **AND** existing speed, KeyLock and quantization ownership SHALL remain coherent

## MODIFIED Requirements

### Requirement: MultiLoop Enabled Allows Concurrent Loops
MultiLoop and BPM Lock SHALL be independently toggleable. Changing MultiLoop SHALL preserve the requested Lock state, its qualified source reference and established master tempo. Mode transitions SHALL preserve existing source and acknowledgement fences, precise scalar loop periods, ordinary scheduling and retirement. Retired source-SYNC and REALTIME BPM preferences SHALL have no runtime effect.

When MultiLoop mode is enabled, the system SHALL allow multiple pads to be active simultaneously. Triggering a pad SHALL start or restart that pad’s loop without stopping other active pads.

#### Scenario: Two pads play concurrently
- **WHEN** MultiLoop mode is enabled
- **AND** the performer triggers pad 1
- **AND** the performer triggers pad 2
- **THEN** pad 1 is active
- **AND** pad 2 is active
- **AND** both pads contribute to the mixed audio output (see `play-samples`)

#### Scenario: Retrigger affects only the selected pad
- **WHEN** MultiLoop mode is enabled
- **AND** pad 1 is active
- **AND** pad 2 is active
- **AND** the performer retriggers pad 1
- **THEN** pad 2 remains active

### Requirement: Loop Onset Is Determined by Trigger Time
Without trigger quantization, the system SHALL start loops immediately when a pad trigger action occurs. With trigger quantization enabled, loop onset SHALL follow the existing sample-frame scheduler and atomic quantized transition requirements.

#### Scenario: Onsets are independent
- **WHEN** MultiLoop mode is enabled
- **AND** pad 1 is triggered at time T1
- **AND** pad 2 is triggered at time T2
- **THEN** pad 1 starts playback at time T1
- **AND** pad 2 starts playback at time T2
