## ADDED Requirements

### Requirement: Shared cleanup intent preserves independent subscriber authority
A live cache assignment SHALL retain its right to admit an independent verified subscriber when another assignment requests eventual shared cleanup or rolls back only its own creation. Admission MUST still reject a retired current assignment, missing ownership-set membership, exact queued deletion, changed source or cache identity, invalid manifest or exhausted bounded subscriber capacity.

#### Scenario: Another assignment requests shared cache cleanup
- **GIVEN** two independent verified assignments still share sealed backing
- **WHEN** one assignment retires itself and requests eventual cleanup
- **THEN** the surviving assignment can admit its own verified subscriber
- **AND** the retired assignment cannot borrow that survivor's authority

### Requirement: Unadopted Copy subscribers have terminal custody
Copy preparation SHALL retain explicit custody of each newly registered subscriber across fallible work and pending callback publication. A terminal failure or safely cancelled unclaimed Copy SHALL release exactly its unadopted subscriber. An already claimed Copy MUST preserve custody until callback settlement, and successful adoption MUST retain the target's subscriber independently of the source.

#### Scenario: Copy is cancelled after preparation and before publication
- **GIVEN** Copy preparation registered a fresh subscriber
- **WHEN** the unclaimed operation is cancelled and its preparation handles are released
- **THEN** both durable ownership sets return to their prior membership
- **AND** the original assignment and backing remain valid

#### Scenario: Preparation fails after subscriber admission
- **GIVEN** a fresh subscriber was registered for Copy preparation
- **WHEN** later preparation fails before callback claim
- **THEN** that subscriber is released without retiring the original assignment or shared backing

### Requirement: Copied complete readers retain independent native registration
Each prepared Copy source SHALL register its own subscriber and source reader through the existing reserved ProjectAssets lane before publication. A claimed Copy MUST retain its cleanup epoch until settlement, and successful control mirroring SHALL transfer custody to the target assignment. Complete-source workers for a finite copied PAD SHALL resolve that target's own registered reader. Final collection SHALL follow existing saved-owner and physical-reader lifetime rules without requiring the original source assignment to remain active.

#### Scenario: Finite copied PAD prepares complete timing PCM
- **GIVEN** a source PAD and its acknowledged Copy both hold finite resident windows
- **WHEN** public timing preparation materializes the Copy's complete source
- **THEN** materialization resolves the Copy's independent registered assignment
- **AND** a healthy original source does not lend its assignment identity to the Copy

#### Scenario: Final Copy custody ends
- **GIVEN** an acknowledged Copy has transferred its independent assignment to its target
- **WHEN** the actual final native readers and saved owners release that backing through existing retirement rules
- **THEN** collection releases the Copy subscriber as well as the original subscriber
- **AND** unloading one PAD alone does not revoke another live PAD or saved owner

#### Scenario: Copied content is moved and swapped after a seek
- **GIVEN** independent acknowledged copies hold active finite source voices
- **WHEN** an actual seek is followed by Move to an empty slot, Swap with another copy and loop editing, with collection before and after each original Native acknowledgement and control mirror
- **THEN** each destination resolves the assignment and lifetime of its moved content
- **AND** complete-source preparation and subsequent loop edits preserve the other copies and original PAD

#### Scenario: A copied voice survives a later source-bank adoption
- **GIVEN** an acknowledged Copy has an active finite voice and a later source adoption replaces its bank window while preserving that voice
- **WHEN** seek preparation captures the actual voice and collection runs before and after its Native acknowledgement
- **THEN** the worker resolves the verified source lease appropriate to that actual voice
- **AND** later loop preparation resolves the current bank's own registered assignment without borrowing a sibling's authority

### Requirement: Saved direct stem sets have consistent typed retirement
The existing artifact namespace SHALL recognize the five declared stem WAV names and complete marker directly beneath a saved canonical material stem container, for both readable and material-ID layouts. Retirement SHALL retain exact path, object identity and last-owner checks. It MUST preserve other live subscribers, unknown files, the material original and newer generation directories; accepting known direct leaves MUST NOT authorize recursive deletion of the base container.

#### Scenario: A copied saved direct set loses its last selection
- **GIVEN** two independent PAD assignments share a saved direct stem set beneath a readable or material-ID root
- **WHEN** the first selection is removed and then the final selection is removed
- **THEN** the first removal preserves all shared artifacts and the final removal admits only the six declared direct files for off-thread retirement
- **AND** both PAD source assignments, unknown files and newer generations remain intact without an artifact-classification cleanup error
