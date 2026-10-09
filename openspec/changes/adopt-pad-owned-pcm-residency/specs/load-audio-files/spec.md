## MODIFIED Requirements

### Requirement: Load Audio File Into Sample Slot
The system SHALL expose the existing asynchronous Python audio-load API for
zero-based sample slots 0 through 215 and copy byte-exact original audio into the
corresponding project-local `samples/#1` through `samples/#216` owner before the
assignment becomes part of the current project.

The persisted sample path SHALL name that pad-owned original using its original
filename and actual extension/encoding. A same-name collision with different or
leased content SHALL use a non-colliding short suffix or safe deferred naming,
never overwrite live bytes. The decoder SHALL retain WAV, FLAC, MP3, AIFF and OGG
support and existing imperfect-MP3 policy. Safe legacy references SHALL remain
readable during verified migration; the final layout SHALL NOT depend on permanent
global original/PCM/stem containers or filesystem aliases.

Source capture, decoding/resampling, full lineage validation, assignment admission
and cleanup SHALL remain off-thread with real native source adoption ACK. App
source replacement SHALL retire its previous playing voices through the existing
bounded native transaction; an invalid path/format/slot or pre-adoption admission
failure SHALL leave previous assignment state intact.

#### Scenario: Load succeeds
- **WHEN** an existing supported audio file is loaded into a valid slot
- **THEN** its stable original is copied under that pad's owner and decoded/resampled off-thread
- **AND** matching native ACK precedes assignment Success and project path update

#### Scenario: Loading replaces an already-loaded sample
- **WHEN** app replacement of a loaded slot completes successfully
- **THEN** its new buffer and pad-owned project path become effective together
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

#### Scenario: Original format and visible owner are preserved
- **WHEN** WAV audio named `Take.wav` is imported into slot215
- **THEN** its byte-exact original is owned below `samples/#216/Take.wav`
- **AND** its `.pcm-cache` and `stems` use the same pad owner without MP3 relabelling

#### Scenario: Same basename is imported to different pads
- **WHEN** two pads import different source bytes named `Take.wav`
- **THEN** each pad owns its own original in its own folder
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
assignment/project/job/queued/history/voice reader retires. Cleanup SHALL target
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
- **AND** another pad's assets and unrelated content are preserved

#### Scenario: Missing sample and missing files are harmless
- **WHEN** unload targets an empty valid slot or an already missing owned file
- **THEN** it is handled safely without a crash or external deletion
- **AND** an out-of-range slot fails without mutation
