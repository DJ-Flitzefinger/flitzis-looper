## ADDED Requirements

### Requirement: Selected-map captured observations have bounded immutable readback
Editor API version3 and later SHALL expose bounded selected-map review info and pages after
transient Raw discard, Copy and fresh restore. A page MUST contain at most64
accepted rows and128 Raw events with exact binary coordinates and the inner
capture revision. It SHALL use a once-built charged immutable index, without
full per-frame parsing, a new Raw lease or a second timing authority. Legacy maps
without complete embedded capture MUST report unavailable without breaking old edits.

#### Scenario: A restored Map3 is reviewed without its former Raw candidate
- **WHEN** the source-current selected map is opened for a bounded review page
- **THEN** its original embedded observations and corrected accepted rows remain inspectable
- **AND** no inference, independent Raw acquisition or full Python JSON parse is required

### Requirement: Map edit allocation is admitted before restoration and growth
Affected restore/parse/index and edit paths MUST use shared checked simultaneous
peak admission before large receipt reads, full JSON trees, index allocation,
splice/inverse growth and serialization/reparse. The512-MiB job bound SHALL charge
held source and coexisting old/new map, complete Raw, index and Undo data. Existing
Raw-build guards and structural count/byte checks SHALL remain. Overflow and
resource refusal MUST preserve previous durable/current state.

#### Scenario: Existing cold map jobs coexist with held audio
- **WHEN** Validate, Store, Restore or Create would read, parse or index a map
- **THEN** admission includes the job's actual held source before that allocation
- **AND** manual creation validates the bounded borrowed descriptor before cloning it

#### Scenario: A short record contains allocation-dense generic payloads
- **WHEN** nested one-element arrays or tiny objects exceed the conservative tree-heap budget
- **THEN** a streaming shape admission refuses before constructing typed/full Value trees
- **AND** the historical Raw-build byte arithmetic remains unchanged without being treated as generic JSON proof

#### Scenario: A small intent would copy a large existing inverse
- **WHEN** actual prospective map/Undo/index/source peak exceeds the admitted bound
- **THEN** the operation is refused before the large copy or mutation
- **AND** the8-KiB intent limit is not misrepresented as an output or RAM proof

### Requirement: Bounded window review grants no automatic musical or audio qualification
Readback, explicit labels and successful window edits SHALL NOT turn detector
ordinals, logits or downbeats into trusted musical count, meter or coverage.
Separate existing qualifications and all B2, renderer, audio-adoption and Human
gates MUST remain. Launcher and Portable checks SHALL require the real new editor
capability and method signatures before presenting this flow as available.

#### Scenario: A newly extended interval remains uncertain
- **WHEN** a review-window edit has been durably selected
- **THEN** local playback qualification still requires the separate coverage/count checks
- **AND** no effective-SYNC, full-music or Human acceptance is claimed
