# stem-cache Specification

## Purpose

Define background stem generation, isolated model acquisition and verified publication
while preserving source-owned audio and realtime playback boundaries.

## Requirements

<!-- From add-offline-stem-cache -->
### Requirement: Stem Generation Is Offline And Cached
The system SHALL generate stem sets only outside the real-time audio callback and store the
results as project-local cached artifacts.

Stem generation SHALL be modeled as offline/background work. It MAY use disk I/O,
temporary files, neural inference, and heavy allocation outside the callback, but the audio
callback SHALL NOT run stem separation or access generation internals.

The initial stem set SHALL include vocals, melody, bass, drums, and instrumental stems.

#### Scenario: Stems are generated for a stopped loaded pad
- **GIVEN** a pad has loaded source audio
- **AND** the pad is not currently playing
- **WHEN** the performer requests stem generation
- **THEN** the system schedules offline/background stem generation for that pad
- **AND** generated stem artifacts are associated with that pad's current source version

#### Scenario: Real-time stem separation is not allowed
- **GIVEN** a pad is playing
- **WHEN** the audio callback renders the pad
- **THEN** the callback does not run stem separation, neural inference, disk I/O, logging, blocking waits, heap allocation, or Python/GIL access


<!-- From add-offline-stem-cache -->
### Requirement: Offline Generation And Effective Adoption Have Separate Authority
The system SHALL accept source-leased stem requests for stopped, playing or paused
pads under the per-pad task fences. Heavy inference SHALL follow the shared
background resource policy. Active playback or a retained paused voice on a
current subscriber defers admission. Saved-set lookup and playback can continue.

A verified disk selection, its registration and requested residency SHALL remain distinct
from effective playback. New-set adoption SHALL use its own state-appropriate source,
content, request and preparation authority and its own Native acknowledgement.
Active adoption SHALL additionally require fresh timing, window, voice and history permits.
Unproved active adoption domains SHALL remain guarded. Same-set residency SHALL NOT
authorize a different complete stem set. Failure, cancellation or stale completion SHALL
preserve current audio and actual reader ownership.

#### Scenario: Playing pad requests offline generation
- **GIVEN** a playing or paused pad has a current immutable source and no conflicting task
- **WHEN** the performer requests stem generation
- **THEN** the request retains its current source lease and visibly waits to start inference
- **AND** existing playback continues

#### Scenario: A current subscriber starts during admitted inference
- **GIVEN** a source-leased inference attempt is active
- **WHEN** a current requested subscriber starts playback
- **THEN** the attempt requests playback-headroom failure and retains its lease through actual backend and reader retirement
- **AND** unrelated playback and playback from a proven obsolete subscriber do not themselves block inference for a surviving current subscriber
- **AND** existing playback continues


<!-- From add-offline-stem-cache -->
### Requirement: Stem Cache Matches The Loaded Source Version
The system SHALL associate cached stems with the exact loaded source version for the pad.

Replacing or unloading the pad source SHALL make previously cached stems unavailable for
playback until a matching valid stem set is generated or restored for the new source
version.

#### Scenario: Replacing a source invalidates old stems
- **GIVEN** a pad has cached stems for source version A
- **WHEN** the pad is loaded with source version B
- **THEN** stems generated for source version A are not eligible for playback on that pad

#### Scenario: Missing cache files degrade safely
- **GIVEN** project state indicates cached stems may exist
- **AND** one or more stem cache files are missing
- **WHEN** the project or pad state is restored
- **THEN** the pad remains usable with full-mix playback
- **AND** missing stem files do not crash load, unload, or playback


<!-- From add-offline-stem-cache -->
### Requirement: Shared Stem Material Has Independent Assignment And Reader Custody
The system SHALL store contained immutable stem generations under verified material and
generation identity. A pad label SHALL NOT define material identity or exclusive ownership.
Compatible verified material may be shared across independently owned assignments; each
subscriber SHALL retain its own source, content, selection and publication authority.
Filename, size, model label or another pad's ACK alone SHALL NOT establish compatibility.

Deletion or unload SHALL withdraw only the addressed assignment's stem intent through its
guarded control path. Logical completion SHALL NOT certify physical file deletion.
Saved selections, running jobs, current/outgoing voices, Native/filter history and complete
PCM readers SHALL retain their exact generations until actual final release. Eligible
physical cleanup SHALL use the existing off-thread asset worker and exact owned inventory;
unknown files, changed identities, external paths and sharing failures SHALL be preserved
or reported without recursive deletion or callback I/O.

#### Scenario: Equal material has independent users
- **GIVEN** two assignments share a verified immutable stem generation
- **WHEN** one unloads or selects a different generation
- **THEN** the other's saved selection and actual readers retain the predecessor
- **AND** the first assignment's ACK does not authorize the second assignment's adoption

#### Scenario: Unload precedes final reader release
- **WHEN** an own Unload acknowledgement ends the addressed assignment
- **THEN** its logical selection is withdrawn
- **AND** files remain protected while any saved, job or actual audio/history reader owns them
- **AND** only eligible exact owned artifacts are later cleaned outside the callback

#### Scenario: Manual deletion preserves full-mix playback
- **WHEN** the performer deletes the selected pad's stems
- **THEN** guarded withdrawal leaves its full-mix source available
- **AND** other assignments and unknown content remain protected


<!-- From add-offline-stem-cache -->
### Requirement: Prepared Stem Buffers Are Aligned For Playback
The system SHALL prepare immutable stem buffers that are aligned with the pad's full-mix
buffer before they are eligible for audio-thread publication.

Prepared stem buffers SHALL use the mixer output sample rate and channel layout, share the
same frame origin as the full mix, and provide frame positions compatible with the pad's
existing loop-region and voice playhead math.

#### Scenario: Prepared stems share the full-mix frame origin
- **GIVEN** stem generation completes for a loaded pad
- **WHEN** the generated audio is prepared for Rust publication
- **THEN** every accepted stem buffer uses the same sample-frame origin as the pad's full-mix buffer
- **AND** the buffers are suitable for synchronized loop playback using the existing voice playhead


<!-- From add-offline-stem-cache -->
### Requirement: Stem Availability Degrades To Full-Mix Playback
The system SHALL preserve existing full-mix playback when stems are unavailable, stale,
incomplete, failed, or disabled.

Stem cache or publication failure SHALL NOT stop currently playing pads, evict scheduled
events, corrupt full-mix sample buffers, or require special recovery from the performer.

#### Scenario: Stem generation fails
- **GIVEN** a pad has loaded full-mix audio
- **WHEN** stem generation fails for that pad
- **THEN** the system reports the failure outside the audio callback
- **AND** the pad remains playable using the existing full-mix buffer


<!-- From add-offline-stem-cache -->
### Requirement: Production Stem Generation Uses A Replaceable Backend
The system SHALL route production stem generation through a file/artifact backend
boundary before cached stem artifacts are eligible for publication. Windows x64
SHALL use the qualified native ONNX worker for the selected separator.

The request SHALL capture the source path and version, target cache and audio
geometry, pinned model installation, device policy and bounded model settings.
Backend objects, tensors, paths, process handles and unbounded metadata SHALL stay
outside the audio callback.

#### Scenario: A stopped pad requests stem generation
- **GIVEN** a stopped loaded pad has source audio
- **WHEN** the performer requests stem generation
- **THEN** the control layer captures a request for the current source version
- **AND** only the expected cache artifacts can reach prepared-stem validation
- **AND** the callback receives bounded prepared-stem handles and scalar mix state

### Requirement: Native Stem Workers Require Installed Model And Codec Resources
The selected worker, graph and verified weights SHALL be installed before UI
generation. Generation SHALL NOT download dependencies. Both separators SHALL
use verified FFmpeg and FFprobe to decode source audio. HTDemucs SHALL also use
FFmpeg to write its four complete PCM16 WAVs. Codec and package failures SHALL
be reported off realtime, leaving the pad playable with its existing assignment.

#### Scenario: Model resources are missing
- **WHEN** the performer requests a separator with no valid model installation
- **THEN** the backend reports the unavailable resource and does not run inference
- **AND** playback remains available

#### Scenario: The required decoder cannot stage input
- **WHEN** required FFmpeg or FFprobe is unavailable or first-stream decode fails
- **THEN** the backend reports a codec error without invoking inference
- **AND** the attempt's artifacts retire through the existing source and last-reader fences

#### Scenario: An explicit FFmpeg directory is used
- **GIVEN** `FLITZIS_FFMPEG_DIR` contains a supported FFmpeg and FFprobe build
- **WHEN** the performer requests stem generation
- **THEN** the backend verifies and binds those tools outside realtime

### Requirement: Windows Stem Workers Retain Model-Specific Processing
BS-RoFormer MUSDB18HQ SHALL use the qualified ONNX graphs with the effective
pinned weights and the established chunk, batch, padding and overlap-add
geometry. HTDemucs SHALL use the qualified ONNX graphs derived from the pinned
stock checkpoint. Its first-stream decode, rate and channel conversion,
normalization, split, captured shifts and overlap, and final crop SHALL preserve
the complete source extent. Both workers SHALL preserve four-stem ordering and
use the common alignment, publication and per-pad OwnACK path.

#### Scenario: A result has no established ONNX producer identity
- **WHEN** a generated stem set lacks verified worker, graph, runtime, codec or
  actual provider evidence
- **THEN** it remains ineligible for automatic cross-job reuse and provider calibration
- **AND** verified saved sets retain their existing source, format, EOF and custody
  checks on restoration

#### Scenario: Inference completes after cancellation or source replacement
- **WHEN** cancellation or changed source identity makes a completed attempt stale
- **THEN** the background owner leaves the previous assignment in place
- **AND** owned pending assets retire after their last reader

<!-- From add-offline-stem-cache -->
### Requirement: Demucs Output Maps To Project Stem Artifacts
The system SHALL map Demucs four-source output into the project's five cached stem artifacts and
align those artifacts to the loaded full-mix buffer before publication.

Demucs `vocals.wav`, `drums.wav`, and `bass.wav` SHALL map to the same project stem names.
Demucs `other.wav` SHALL map to project `melody.wav`. The project `instrumental.wav` artifact
SHALL be generated as a non-vocal cache artifact from the final aligned drums, bass, and melody
stems. The UI `I` preset SHALL continue to mean Drums + Melody + Bass and SHALL NOT become direct
playback of only `instrumental.wav`.

#### Scenario: Other is treated as melody
- **GIVEN** Demucs produces `other.wav`
- **WHEN** the backend writes project cache artifacts
- **THEN** the generated project cache contains `melody.wav` using the aligned `other.wav` audio
- **AND** the generated cache does not require an `other.wav` project artifact

#### Scenario: Generated cache matches the loaded shape
- **GIVEN** Demucs output uses a different sample rate, channel layout, or frame count than the loaded full mix
- **WHEN** the backend writes project cache artifacts
- **THEN** each final cache WAV is resampled, channel-adapted, trimmed, or padded outside the audio callback
- **AND** every final cache WAV matches the loaded full-mix sample rate, channel count, and frame count


<!-- From add-offline-stem-cache -->
### Requirement: Demucs Quality Parameters Are Bounded
When Demucs is selected, the system SHALL run that backend with bounded quality parameters outside the audio
callback.

The HTDemucs backend SHALL use one shift and 0.5 overlap unless the Settings page
provides validated replacement values. The control layer SHALL persist the configured values in
project state and SHALL reject Demucs quality parameters outside the app-supported range before
invoking Demucs. The app-supported range SHALL be `shifts` from 1 through 20 and `overlap` from
0.25 through 0.95.

#### Scenario: Default quality parameters are used
- **GIVEN** the qualified HTDemucs installation and FFmpeg tools are available
- **AND** Demucs is the selected separator
- **AND** no Settings page override has supplied alternate quality parameters
- **WHEN** the performer requests stem generation
- **THEN** the background worker uses one shift
- **AND** the worker uses 0.5 overlap

#### Scenario: Settings quality values are used
- **GIVEN** the performer configures Demucs shifts to 4
- **AND** the performer configures Demucs overlap to 0.25
- **WHEN** the performer requests stem generation
- **THEN** the background worker uses four shifts
- **AND** the worker uses 0.25 overlap

#### Scenario: Invalid settings are rejected before Demucs runs
- **GIVEN** the Settings page supplies an unsupported Demucs quality value such as shifts `0`,
  overlap `0.0`, or overlap `1.0`
- **WHEN** the performer requests stem generation
- **THEN** the control layer rejects the request outside the audio callback
- **AND** Demucs is not invoked
- **AND** the pad remains playable with full-mix playback


<!-- From add-offline-stem-cache -->
### Requirement: Stem Model Cache And Device Fallback Stay Off The Audio Path
Model packages SHALL live outside generated stem artifacts. The default source
input cache is `models/stem-checkpoints/`. Explicit managed and Portable model
bindings take precedence. Protected sample, recording, configuration and source
trees SHALL be excluded from model installation.

The explicit CPU policy SHALL bypass CUDA probing. Auto MAY use a qualified CUDA
provider and device. A qualified CUDA device or provider failure permits one
fresh CPU attempt with the same model, sealed input and effective settings once
the CUDA process, tree, readers and attempt scratch have retired. Cancellation,
stale input, package, codec, host, disk and unclassified failures retain distinct
outcomes.

#### Scenario: A CUDA attempt fails with an eligible receipt
- **GIVEN** a captured request permits automatic CUDA and the worker reports a
  qualified CUDA device or provider failure
- **WHEN** the original attempt's process, tree, readers and scratch have retired
- **THEN** the backend may retry with the same input, model and effective settings
- **AND** the first diagnostic and actual fallback device are reported off realtime

#### Scenario: Compacted diagnostics cannot authorize a CPU retry
- **GIVEN** an owned separator exits with the CUDA-failure exit code
- **AND** its output contains only an indented CUDA-failure marker as diagnostic text
- **WHEN** the host compacts bounded diagnostics
- **THEN** the diagnostic remains distinct from an exact protocol receipt
- **AND** neither separator retries on CPU on the strength of that diagnostic

### Requirement: Separator Diagnostics Cannot Become Protocol Receipts
The host SHALL preserve the distinction between diagnostic text and exact child
protocol lines while compacting or truncating command output. Whitespace
normalization SHALL NOT manufacture encoder, BS input, completed-execution or
CUDA-failure evidence. Genuine independent receipts SHALL remain available, and
duplicate or oversized receipts SHALL remain invalid.
Receipt framing SHALL use the raw child's LF boundaries, retaining the existing
trailing-CR and end-of-stream tolerance. Embedded CR, VT, FF, record separators,
NEL and Unicode line separators SHALL remain diagnostic text after decoding.

#### Scenario: An embedded diagnostic separator precedes a marker
- **GIVEN** an owned child embeds a protocol marker after a non-LF diagnostic separator
- **WHEN** the host captures, decodes and compacts that output
- **THEN** the marker remains diagnostic for all four receipt roles
- **AND** one separate genuine LF-framed receipt remains valid without becoming a duplicate

#### Scenario: An indented execution diagnostic accompanies a genuine receipt
- **GIVEN** a diagnostic contains an indented completed-execution marker
- **AND** the child emits one separate exact completed-execution receipt
- **WHEN** the host normalizes command output
- **THEN** only the separate exact receipt is available to the execution parser
- **AND** the diagnostic remains bounded and visibly distinct from protocol evidence


<!-- From add-stem-performance-controls -->
### Requirement: Stem Control State Follows Current Source Version
The system SHALL expose stem availability to performer controls only for the pad's current
loaded source version.

If source-version metadata is stale, cache files are missing, cache validation fails, or Rust
publication rejects prepared handles, the control layer SHALL show stems as unavailable or
blocked and SHALL preserve full-mix playback.

#### Scenario: Available state requires matching source version
- **GIVEN** a pad has loaded source version A
- **AND** the project has complete stem cache metadata for source version A
- **WHEN** the performer views stem controls for that pad
- **THEN** the controls may show stems as available

#### Scenario: Source replacement clears stem eligibility
- **GIVEN** a pad has available stems for source version A
- **WHEN** the pad source is replaced with source version B
- **THEN** performer controls no longer expose source version A stems as playable for that pad
- **AND** full-mix playback remains available


<!-- From add-stem-performance-controls -->
### Requirement: Full-Mix And Prepared-Stem Modes Are Explicit
The system SHALL model full-mix playback and prepared-stem playback as explicit per-pad stem
mix modes.

Full-mix mode SHALL use the loaded full-mix buffer. All-stems mode SHALL use the complete
current prepared stem set when available and SHALL degrade to full-mix playback when the stem
set is unavailable, stale, incomplete, failed, rejected, or disabled.

#### Scenario: Full-mix mode ignores available stems
- **GIVEN** a pad has loaded full-mix audio
- **AND** the pad has a complete current prepared stem set
- **AND** the pad stem mix mode is full-mix
- **WHEN** the pad is triggered
- **THEN** playback uses the loaded full-mix buffer

#### Scenario: All-stems mode falls back when cache is unavailable
- **GIVEN** a pad's durable stem mix preference is all-stems
- **AND** no valid current prepared stem set is available
- **WHEN** the pad is triggered
- **THEN** playback falls back to the loaded full-mix buffer
- **AND** the performer can regenerate stems without affecting current playback


<!-- From add-stem-performance-controls -->
### Requirement: Per-Stem Controls Use Bounded Known Stem Kinds
The system SHALL limit per-stem performance controls to bounded masks over known stem kinds and
component-stem presets.

Per-stem control state SHALL be represented as bounded masks or scalar modes, not unbounded
lists or file paths. The bottom-bar `I` preset SHALL mean Drums + Melody + Bass, not the
cached `instrumental.wav` artifact. The `A` preset SHALL mean Vocals + Drums + Melody + Bass and
SHALL NOT add `instrumental.wav` as a fifth audible layer. The audio callback SHALL NOT generate
stems, read cache files, decode audio, allocate stem buffers, run neural inference, log, block, or
acquire the Python GIL in response to per-stem controls.

#### Scenario: Stem toggle updates bounded state only
- **GIVEN** a pad has a current prepared stem set already published to Rust
- **WHEN** the performer toggles the selected-pad vocals stem control
- **THEN** the control update is represented as bounded state for the known stem kinds
- **AND** no stem generation, file I/O, decoding, inference, logging, blocking, heap allocation, or Python/GIL access runs in the audio callback

#### Scenario: Instrumental preset avoids direct instrumental artifact rendering
- **GIVEN** a pad is in all-stems mode with a current prepared stem set
- **WHEN** the performer selects the `I` preset
- **THEN** the runtime enabled-stem mask enables Drums, Melody, and Bass
- **AND** the audio callback does not select only the cached `instrumental.wav` artifact

### Requirement: Inference Device Observations Remain Diagnostic
The selected PAD SHALL display confirmed BPM and stem inference devices separately
from live playback and generic processing stages. CUDA availability, requested
device and progress prose SHALL NOT establish actual execution. A running attempt
without a completed execution receipt SHALL display `Device unknown`.

Owned separator children MAY emit one bounded completed-device receipt after
successful inference. The host SHALL check its backend, model and explicit device;
missing, duplicated, malformed or mismatched receipts SHALL remain unknown.
Producer encoding, decoding and passive cost observations SHALL use the same
strict source-owned internal receipt codec. Public execution models SHALL remain
diagnostic values at control, session and history boundaries. Converted diagnostics
SHALL NOT establish producer identity or promote passive calibration. Source
identity SHALL include the actual internal codec and every other executed host
role; unsupported or altered roles SHALL remain unknown.
The shared selector's host roles SHALL include both consulted backend adapters
and their required host dependencies for either selection. Selected child model
roles SHALL retain their separate source custody. Equivalent explicit resource
declarations SHALL preserve the live sampler, captured clock, shared mutable
owners, admission thresholds, recovery latch and fixed query-failure deadline;
component declaration proof SHALL NOT certify unsupported runtime bindings.
Encoder and BS-input execution witnesses SHALL remain independently preserved and
validated. Existing two-argument progress callbacks SHALL remain compatible.

The typed observation SHALL follow the existing current-source subscriber and
Pair placement fences and survive later alignment progress and coalescing. Every
successful shared subscriber SHALL receive the original terminal observation.
New attempts and source replacement SHALL clear active observations. Historical
cache metadata SHALL not be displayed as the current unconfirmed attempt.

An optional stem-cache execution observation MAY persist through project restore,
Copy and Move. Old caches SHALL remain readable with unknown device. Malformed
optional observations SHALL NOT erase valid PCM or Pair evidence. New production
SHALL replace the observation with its own terminal receipt or unknown; completed
cache reuse SHALL preserve the original history. Execution metadata SHALL NOT
grant source, timing, cache reuse, Native publication or audio authority.

#### Scenario: CUDA inference completes before CPU alignment
- **WHEN** a source-fenced completed CUDA receipt is followed by alignment progress
- **THEN** the inference device remains visible independently of the alignment stage
- **AND** returning PCM on CPU does not relabel the inference as CPU

#### Scenario: A new attempt has no confirmed execution
- **GIVEN** an older stem cache records CUDA inference
- **WHEN** the performer starts another generation for that source
- **THEN** its current device is unknown until its own valid receipt arrives
- **AND** copied or restored historical metadata does not confirm that new execution


