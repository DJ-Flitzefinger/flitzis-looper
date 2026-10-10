## MODIFIED Requirements

### Requirement: Load Audio File Into Sample Slot
The system SHALL expose the existing asynchronous Python audio-load API for
zero-based sample slots 0 through 215 and copy byte-exact original audio into the
canonical immutable project-local `samples/materials/M<stable-id>/original/` material version before the
assignment becomes part of the current project.

The persisted assignment SHALL bind durable ContentInstance identity/lineage and
the shared material version. Reopen SHALL allocate a fresh nonreused runtime
lifetime and SHALL NOT restore saved action/feedback/HoldRelease authority. Its
sample path SHALL name that material-owned original using its original
filename and actual extension/encoding. A same-name collision with different or
leased content SHALL use a non-colliding short suffix or safe deferred naming,
never overwrite live bytes. The decoder SHALL retain WAV, FLAC, MP3, AIFF and OGG
support and existing imperfect-MP3 policy. Safe legacy references SHALL remain
readable during verified migration; the final layout SHALL NOT depend on permanent
legacy global containers or filesystem aliases. Canonical shared material storage
SHALL be the final target, with #1..#216 identifying stable slot membership.

Source capture, decoding/resampling, full lineage validation, assignment admission
and cleanup SHALL remain off-thread with real native source adoption ACK. App
source replacement SHALL retire its previous playing voices through the existing
bounded native transaction; an invalid path/format/slot or pre-adoption admission
failure SHALL leave previous assignment state intact.

#### Scenario: Load succeeds
- **WHEN** an existing supported audio file is loaded into a valid slot
- **THEN** its stable original is captured under the immutable material owner and decoded/resampled off-thread
- **AND** matching native ACK precedes assignment Success and project path update

#### Scenario: Loading replaces an already-loaded sample
- **WHEN** app replacement of a loaded slot completes successfully
- **THEN** its fresh content lifetime, new buffer and material project path become effective together
- **AND** previous app voices stop through bounded native retirement

#### Scenario: Sample id is out of range
- **WHEN** loading uses an id outside0..215
- **THEN** the call fails without modifying any slot

#### Scenario: File path is invalid
- **WHEN** loading names a nonexistent source
- **THEN** the call fails without changing previous assignment state

#### Scenario: File format is unsupported
- **WHEN** a source has no usable supported decodable audio
- **THEN** loading fails without changing previous assignment state

#### Scenario: Original format and material membership are preserved
- **WHEN** WAV audio named `Take.wav` is imported into slot215
- **THEN** its byte-exact original is owned below `samples/materials/M<id>/original/Take.wav` with slot#216 membership
- **AND** its `.pcm-cache` and `stems` use the same immutable material version without MP3 relabelling

#### Scenario: Canonical absolute restore reuses its material
- **WHEN** an existing canonical original is restored using a relative, ordinary absolute or Windows extended absolute path
- **THEN** explicit restore intent preserves its material binding and original without creating another copy
- **AND** the attempt still receives fresh native adoption authority

#### Scenario: Invalid namespace cannot become an import
- **WHEN** a managed source reference contains lexical traversal, a reparse ancestor, a Windows leaf alias, an invalid material/slot ID or a non-original artifact kind
- **THEN** the shared native resolver rejects it before normalization, filesystem writes or assignment mutation
- **AND** external bytes and previous assignment state remain intact

#### Scenario: Owner admission fails after productive preparation
- **WHEN** original owner capacity is exhausted before a prepared source can be enqueued
- **THEN** its rollback guard retires only that attempt's original and PCM creations
- **AND** previous source, voice and saved assignment ownership remain intact

#### Scenario: Supported original basename starts with a dot
- **WHEN** supported audio named `.Take.wav` is imported
- **THEN** its canonical original keeps the exact basename and bytes
- **AND** isolated material metadata cannot be mistaken for that original

#### Scenario: Same basename is imported to different pads
- **WHEN** two pads import different source bytes named `Take.wav`
- **THEN** each different material version has a collision-safe original and independent content assignment
- **AND** neither import overwrites another pad or an earlier leased generation

#### Scenario: Replacement cannot overwrite a retained original
- **WHEN** a pad loads different audio with a basename still owned by old readers
- **THEN** capture uses a collision-safe owned filename and fresh source identity
- **AND** successful app adoption stops the replaced assignment's old voices safely

#### Scenario: Invalid slot or unsafe path is rejected
- **WHEN** a request uses an invalid slot, #0, #217, traversal or a reparse alias
- **THEN** no assignment or owned path is changed
- **AND** missing/undecodable source fails without falsely reporting Success

### Requirement: Unload Sample Slot
The system SHALL expose the existing unload API and immediately revoke the
unloaded slot's source eligibility through bounded native admission before
changing its project intent.

Track-bound settings SHALL reset as already specified. Owned originals, PCM and
stem generations SHALL be physically retired off-thread only after their final
all-bank assignment/project/job/subscriber/action/queued/history/voice/native-ACK reader retires. Cleanup SHALL target
resolved captured file identities, preserve other owners and unknown content,
and tolerate missing files. A full admission queue SHALL preserve previous state.

#### Scenario: Unload removes sample for subsequent playback
- **WHEN** a loaded slot unloads successfully
- **THEN** it has no eligible source and later triggers are ignored/dropped

#### Scenario: Unload stops currently playing audio for the sample id
- **WHEN** successful app unload targets a playing sample
- **THEN** its voices stop contributing through bounded native retirement

#### Scenario: Unload removes cached audio file
- **WHEN** an unloaded pad's owned cached original has no remaining reference or reader
- **THEN** it is physically deleted off-thread under captured identity checks

#### Scenario: Unload ignores missing cached audio file
- **WHEN** an unloaded owned cached file is already missing
- **THEN** cleanup is harmless and does not crash

#### Scenario: Unload missing sample id is handled safely
- **WHEN** unload targets an empty valid slot
- **THEN** the request is safely ignored

#### Scenario: Unload sample id is out of range
- **WHEN** unload uses an id outside0..215
- **THEN** it fails without modifying slot state

#### Scenario: Unload retires pad assets after readers
- **WHEN** slot0 unloads while an old owned immutable reader remains
- **THEN** slot0 cannot launch new voices and existing app voices stop safely
- **AND** its tracked asset bytes remain until the reader retires off-thread
- **AND** all surviving equal material users and unrelated content are preserved

#### Scenario: Missing sample and missing files are harmless
- **WHEN** unload targets an empty valid slot or an already missing owned file
- **THEN** it is handled safely without a crash or external deletion
- **AND** an out-of-range slot fails without mutation

#### Scenario: Removing an origin preserves copies through reopen
- **GIVEN** independent contents in different banks share prepared material
- **WHEN** the origin slot or source bank is removed and the project saves/closes/reopens
- **THEN** surviving contents restore with fresh native source/timing ACK and remain usable
- **AND** no new analysis, separation, complete decoding or file duplication occurs for valid prepared data

#### Scenario: Admission failure preserves content and hold ownership
- **WHEN** unload or replacement cannot reserve full native/action/feedback retirement capacity
- **THEN** old content, hold ownership, settings, source authority and files remain intact
- **AND** successful removal later fences old content-bound actions before slot reuse
