## MODIFIED Requirements

### Requirement: Load Audio File Into Sample Slot
The system SHALL expose a Python API to load WAV, FLAC, MP3, AIFF and OGG audio
from a filesystem path into a sample slot in `0..NUM_SAMPLES`.

Before decoding newly selected audio, the system SHALL copy/hash stable actual
bytes into an immutable project-owned snapshot. The retained project original
SHALL be byte-exact under `./samples/`, use the original basename with numeric
suffixes on collision, and be the persisted `ProjectState.sample_paths[id]` asset.
Restore SHALL capture the existing project original immutably without making a
second durable original. Complete validated PCM MAY replace repeat decoding.

Source replacement SHALL preserve already pinned native voice ownership until
its existing stop/unload/transition lifecycle; a bank publication MUST NOT
relabel old voice PCM or timing. The application's explicit unload-before-new-load
path SHALL still stop that pad's voices and reset track-bound intent.
Invalid id/path/format or failed native admission SHALL not publish a new source.

#### Scenario: Load succeeds
- **WHEN** a supported source is selected
- **THEN** actual copied bytes and their digest establish the immutable input
- **AND** decoding uses that snapshot unless a complete compatible PCM entry is validated and reused
- **AND** the project records the byte-exact non-colliding project original

#### Scenario: Loading replaces an already-loaded sample
- **WHEN** a native bank publication replaces the source for a loaded slot
- **THEN** new starts use the newly published source
- **AND** existing pinned voices retain their original source/timing until their lifecycle ends

#### Scenario: Application replaces an assignment
- **WHEN** the performer selects a new source for an occupied pad
- **THEN** the existing explicit unload stops its voices and resets track-bound settings
- **AND** subsequent successful loading assigns the new project original

#### Scenario: Invalid id path or format
- **WHEN** the requested id is out of range, the path cannot be captured, or decode fails
- **THEN** no new source is published and the failure is reported

### Requirement: Unload Sample Slot
The system SHALL expose a Python API to unload a sample slot in `0..NUM_SAMPLES`,
stop its voices through existing native ordering and invalidate its pending jobs.

Owned project originals, PCM and prepared assets SHALL be deleted off-thread
only after their final assignment and reader/job/queued/voice owner retires.
Shared entries SHALL survive another pad's unload. Deletion SHALL verify resolved
managed-path containment and exclusive cleanup ownership, tolerate missing files
and never delete external originals. Out-of-range ids SHALL raise an exception.

#### Scenario: Unload removes sample for subsequent playback
- **WHEN** a loaded slot is unloaded
- **THEN** it has no loaded source for subsequent starts
- **AND** invalidated completions cannot republish it

#### Scenario: Unload stops currently playing audio for the sample id
- **WHEN** the admitted unload executes in the native lifecycle
- **THEN** its active voices stop contributing and their handles retire off-thread

#### Scenario: Unload removes cached audio file
- **GIVEN** a project-owned file under `./samples/` has no remaining assignments or readers
- **WHEN** unload cleanup obtains exclusive ownership after native retirement
- **THEN** the owned file is removed off-thread

#### Scenario: Unload retains a shared cached audio file
- **WHEN** another pad or job still owns the same asset or digest entry
- **THEN** the shared asset remains readable until the final owner retires

#### Scenario: Unload ignores missing cached audio file
- **WHEN** a safely owned cleanup file is already absent
- **THEN** cleanup does not crash

#### Scenario: Unload cannot escape managed asset root
- **WHEN** a stale project path traverses out of samples or resolves through a link to an external source
- **THEN** unload never deletes that external path

#### Scenario: Unload missing sample id is handled safely
- **WHEN** an in-range slot is already empty
- **THEN** unload is handled safely without restoring a source

#### Scenario: Unload sample id is out of range
- **WHEN** the id is outside `0..NUM_SAMPLES`
- **THEN** the request fails with a Python exception
