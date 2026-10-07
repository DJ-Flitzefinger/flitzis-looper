## ADDED Requirements

### Requirement: Immutable copy-first decoder input
The system SHALL copy actual stable source bytes into a project-owned immutable
snapshot and hash those bytes while copying before decoding that same snapshot.

The byte-exact project original SHALL remain separate from regenerable PCM.
Content association MUST NOT rely on a path, size/mtime tuple or a pre-decode hash
of another file opening. Snapshot stability and reader lifetime SHALL be enforced;
an input that cannot be captured stably SHALL fail without publishing source state.

#### Scenario: External path changes between copy and decode
- **WHEN** an external path is replaced after immutable snapshot capture
- **THEN** decode and original digest refer to the captured bytes
- **AND** no decoder read reopens the changed external path

#### Scenario: Concurrent mutation or ABA cannot establish false lineage
- **WHEN** mutation prevents a stable capture or a path changes away and back
- **THEN** a stat-only or before/after path-hash match cannot authorize publication
- **AND** only a stable snapshot and actual copied digest can establish lineage

### Requirement: Complete versioned PCM identity
The system SHALL store complete decoder PCM and any complete playback derivative
with versioned identities binding actual original and full PCM content digests,
decoder/processing versions, sample format/layout, rate, channels, full frame
extent, source origin and the executed transform.

Complete interleaved PCM and the complete arithmetic-channel-mean analysis digest
SHALL remain distinct identities. Window hashes MUST NOT substitute for full hashes.

#### Scenario: Identical bytes have a different interpretation
- **WHEN** format, rate, channel layout, extent, origin or processing version differs
- **THEN** the entry is incompatible even if some PCM bytes or paths match
- **AND** the required compatible full artifact is prepared off-thread

### Requirement: Separate rate and transform domains
The system SHALL retain decoder/source, playback, device and analyzer rates and
extents separately without requiring all sources to become 48000 Hz.

Playback derivatives SHALL preserve the existing engine-format contract. Decoder
delay/padding and resampler phase, leading delay, tail and exact ceiling-derived
length SHALL have recorded versioned policies and independent execution evidence.
No unproved trimming or delay compensation SHALL silently redefine source zero.

#### Scenario: 44100-Hz source on a 96000-Hz device
- **WHEN** the source is cached and prepared for playback or analysis
- **THEN** decoder PCM retains its actual 44100-Hz dimensions
- **AND** playback/device/analyzer derivatives bind their own actual dimensions,
  digests and transforms without relabelling accepted historical PCM evidence

### Requirement: Validated complete cache reuse
The system SHALL reuse only complete compatible artifacts whose full source and
PCM content, manifest, exact lengths and dimensions were validated on the
immutable lease used by the consumers.

Fresh warm leases SHALL perform complete integrity verification; verified
immutable in-process leases MAY be shared. Partial, corrupt or incompatible
entries SHALL be rejected and regenerated with bounded work. Integrity I/O and
CPU cost SHALL be accounted for in warm and save measurements.

#### Scenario: Same-size cache corruption or unfinished write
- **WHEN** PCM is changed without a size change or an entry lacks its committed manifest
- **THEN** it is not treated as ready or reused
- **AND** regeneration cannot expose a partially written artifact

### Requirement: Bounded preparation and guarded publication
The system SHALL bound admission, queue size, worker concurrency and transient
PCM bytes for copy/decode/validation/resampling/window preparation.

Artifact publication SHALL commit an immutable complete entry atomically.
Pad publication SHALL separately check current source/request/intent and window
revision and atomically publish matching handles, metadata and completion state.
Queue-full, failure or cancellation MUST NOT produce partial native ownership.

#### Scenario: Late completion after unload or a newer request
- **WHEN** a worker completes for an invalidated request or window revision
- **THEN** no source, window, analysis, stem or readiness state is republished
- **AND** its exclusively owned temporary artifacts and readers retire off-thread

### Requirement: Full metadata is independent of residency
The system SHALL retain stable complete-source identity, source zero, full rate/
channels/frame count/duration and timing/label provenance independently of
resident ranges, resident frame counts and monotonically guarded window revisions.

Absolute source addresses SHALL be translated explicitly into resident storage.
Changing residency MUST NOT create new accepted evidence, relabel pinned voices,
move loop markers or derive full duration from a window's allocation.

#### Scenario: Saved middle loop has fewer resident than full frames
- **WHEN** only the admitted loop and proved context are resident
- **THEN** full duration and source-relative waveform/grid/seek coordinates remain unchanged
- **AND** CURRENT, MIDI, stems and history refer to matching complete-source and window ownership

### Requirement: Proved loop and DSP context
The system SHALL admit a loop window only when its full read set and retained
processing state cover interpolation taps, physical bounds, fractional musical
wrap, supported rate/smoothing, Key Lock history/native/FIFO/filter state and
matching stem transitions.

The context SHALL be derived from executed reader/DSP behavior and independently
proved against complete-buffer playback. Unproved or over-budget context SHALL
use an explicit admitted full-track exception or report preparation unavailable.

#### Scenario: Fractional seam or Key Lock needs samples outside physical loop
- **WHEN** the required context exceeds the proposed loop window
- **THEN** a guessed fixed margin cannot authorize playback
- **AND** the system prepares proved context or a complete-track fallback before readiness

### Requirement: Finite transactional readiness
The system SHALL expose requested intent, preparation/pending/error state and
effective acknowledged source/window state separately for finite replacements.

Existing audio and its complete matching loop/timing/stem/DSP state SHALL remain
valid until the new transaction passes bounded native guards and adoption ACK.
Pending work SHALL NOT claim an effective seek, loop, ALL or accepted timing change.

#### Scenario: Rapid edits while a window is preparing
- **WHEN** a newer edit supersedes a pending window
- **THEN** the previous effective audio continues and the older completion is rejected
- **AND** only the latest matching ready transaction can change effective playback

### Requirement: Complete editor and analysis access
The system SHALL preserve full-source waveform navigation, extreme sample zoom,
source-relative overlays and complete-track analysis through non-realtime
complete-source readers independent of playback residency.

View-only navigation SHALL NOT seek audio. Pending full-source reads SHALL keep
the UI usable, retain honest source-bound readiness and never use a cropped loop
as complete input to timing, key or stem analysis.

#### Scenario: View jumps outside the resident loop
- **WHEN** the editor jumps to the full track's start or end
- **THEN** matching complete-source waveform data becomes available off-thread
- **AND** audio progression, source zero, timing and labels remain unchanged

### Requirement: Explicit seek and full-track exceptions
The system SHALL preserve explicit seek inside, before and after the loop:
intro playback reaches the loop, tail playback reaches the actual full source
end then wraps, paused seek remains paused and stopped seek remains a no-op.

Nonresident seeks and ALL SHALL prepare an admitted complete-track resident
exception before effective publication. These are finite preparations and
MUST NOT introduce callback file reads or a continuous streaming framework.

#### Scenario: Seek past a short resident loop
- **WHEN** an active pad requests a nonresident tail position
- **THEN** old playback remains effective during complete-track preparation
- **AND** after matching ACK the tail plays to the full source end then wraps into
  the existing loop without changing markers, grid or auto-loop intent

### Requirement: Same-source stem windows
The system SHALL bind full-mix and prepared component stems to matching complete
source identity, accepted timing where applicable, absolute source ranges and
window revisions before transactional adoption.

Full stem extent/channel metadata SHALL remain separate from resident counts.
Cached instrumental data SHALL NOT become a fifth live component; stale windows,
source/timing permits or incomplete component sets SHALL remain unavailable.
Active finite window relocation SHALL preserve the identical complete source
and already accepted complete StemSet identity. Generation or adoption of a new
complete set SHALL retain the inactive-pad restriction.

#### Scenario: Stem result races a window edit
- **WHEN** same-source stems finish for an older window revision
- **THEN** they cannot replace the matching current resident set
- **AND** the prior valid full-mix/stem trajectory and mask continue

### Requirement: Last-user cleanup and external-original safety
The system SHALL invalidate pending work on cancellation/unload and retire all
pad, job, editor/analysis, queued-command, native-history and active/pinned-voice
readers through bounded off-thread cleanup before deleting owned artifacts.

Shared digest entries SHALL remain valid until their final owning assignment and
reader retire. Cleanup SHALL check managed-path containment, serialize against
new reader admission, retry safe deferred deletion and tolerate missing files.
The system MUST NOT delete an external original.

#### Scenario: One of two pads sharing a digest unloads
- **WHEN** the first pad unloads while the second pad or a voice/job retains a reader
- **THEN** the shared PCM and required project original remain available
- **AND** final deletion occurs only after the last owner and reader safely retire

### Requirement: Realtime residency boundary
The system SHALL perform disk access, integrity scans, decoding, resampling, JSON,
large allocation/deallocation and deletion outside the audio callback.

The callback SHALL use only prebuilt resident immutable handles, bounded read/
guard/ACK operations and reserved retirement capacity, without GIL, blocking
locks, logging, neural inference, plugin work or unbounded preparation.

#### Scenario: Retirement capacity is temporarily exhausted
- **WHEN** a replacement cannot reserve safe retirement and feedback capacity
- **THEN** effective publication is deferred without freeing payloads or reading disk on the callback

### Requirement: Measured cache and residency acceptance
The system SHALL establish cache/residency claims with real cold/warm measurements
on 200 occupied pads, short loops in long sources and explicit full-track exceptions.

Evidence SHALL include source identities, readiness, worker/handle peaks, steady/
transient/process RAM, disk/integrity/save I/O and CPU, corruption/cancellation/
cleanup, 44100/48000/96000-Hz parity, fractional periods/rates/starts/tails,
Key Lock and stems, accepted timing and <=1-loaded-frame unwrapped loop bounds.
Human listening and actual device acceptance SHALL remain separately open until
their final human-run stage.

#### Scenario: A warm startup report claims improvement
- **WHEN** cold/warm results are compared
- **THEN** sources and conditions match and integrity/full-track exception costs are reported
- **AND** current 96-native-handle costs are measured rather than inferred from
  historical 64-handle results or substituted for hearing/device acceptance
