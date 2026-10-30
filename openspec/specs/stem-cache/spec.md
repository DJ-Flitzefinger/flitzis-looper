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
The system SHALL permit source-leased offline generation for stopped, playing or paused
pads without running generation in the audio callback. Conflicting load, analysis,
unload and generation intent SHALL retain their existing admission fences.

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
- **THEN** the bounded background path retains its source lease through actual backend return
- **AND** existing playback continues independently of generation

#### Scenario: Pad starts during generation
- **WHEN** a pad starts before generation completes
- **THEN** a verified result may commit for still-current material and model intent
- **AND** effective new-set playback requires the pad's independently guarded transaction and own ACK
- **AND** an unavailable adoption domain leaves existing audio usable


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
The system SHALL route production stem generation through a replaceable file/artifact backend
boundary before cached stem artifacts are eligible for publication.

The backend request SHALL contain stable control-plane inputs such as the source audio path,
pad id, source version, project-local target cache directory, target sample rate, target channel
count, target frame count, model cache directory, device policy, and bounded backend quality
parameters. Backend implementations SHALL NOT expose Demucs, Torch, tensors, Python module
objects, process handles, or unbounded metadata to project state, Rust control messages, or the
audio callback.

#### Scenario: Demucs is isolated behind the backend boundary
- **GIVEN** a stopped loaded pad has source audio
- **WHEN** the performer requests stem generation
- **THEN** the control layer builds a backend request for the current source version
- **AND** the backend writes only the expected project-local cache artifacts
- **AND** Rust publication continues to use the existing prepared-stem validation path

#### Scenario: Backend objects do not reach the audio callback
- **GIVEN** the Demucs backend imports Torch or runs a subprocess
- **WHEN** stem generation completes or fails
- **THEN** the audio callback receives no Demucs objects, Torch objects, tensors, file paths, or process handles
- **AND** the callback remains limited to bounded prepared-stem handles and scalar mix state


<!-- From add-offline-stem-cache -->
### Requirement: Demucs Runtime Dependency Is Declared Separately From Model Files
The system SHALL declare the Demucs adapter package as an application runtime dependency while
requiring Demucs model files to be installed before UI-driven generation starts.

Demucs model files SHALL remain runtime cache data rather than vendored repository files or
project-local sample artifacts. The Looper SHALL NOT trigger Demucs model download from the
Generate Stems UI path. If the required model checkpoint is missing, the system SHALL report the
short error `no Model installed` outside the audio callback and SHALL NOT invoke the Demucs
separation subprocess. If required external audio tools are unavailable, the system SHALL report
the short error `FFmpeg/ffprobe unavailable` outside the audio callback before invoking Demucs. If
TorchCodec cannot load for the current Torchaudio output path, the system SHALL report
`TorchCodec unavailable` outside the audio callback before invoking Demucs.
The backend SHALL resolve `ffmpeg` and `ffprobe` from the process `PATH`, from an explicit
`FLITZIS_FFMPEG_DIR` directory, or from a local WinGet `Gyan.FFmpeg*` package directory before
reporting those tools unavailable.

#### Scenario: Preinstalled model allows generation to start
- **GIVEN** the project dependencies have been synced into the active Python environment
- **AND** the required Demucs model checkpoint exists in the standard Torch Hub checkpoint cache
- **AND** a stopped loaded pad has source audio
- **WHEN** the performer requests stem generation
- **THEN** `python -m demucs` is available to the background backend
- **AND** the background backend may run Demucs without starting a model download from the UI path

#### Scenario: Missing model fails briefly before Demucs runs
- **GIVEN** the project dependencies have been synced into the active Python environment
- **AND** the required Demucs model checkpoint is missing
- **WHEN** the performer requests stem generation
- **THEN** the backend reports `no Model installed`
- **AND** Demucs is not invoked
- **AND** the pad remains playable with full-mix playback

#### Scenario: Unsynced environment fails without affecting playback
- **GIVEN** the active Python environment is missing the declared Demucs runtime dependency
- **WHEN** the performer requests stem generation
- **THEN** the backend reports a normal generation error outside the audio callback
- **AND** the pad remains playable with full-mix playback

#### Scenario: Missing external audio tools fail before Demucs runs
- **GIVEN** the required Demucs model checkpoint is installed
- **AND** `ffprobe` or `ffmpeg` is missing or inaccessible
- **WHEN** the performer requests stem generation
- **THEN** the backend reports `FFmpeg/ffprobe unavailable`
- **AND** Demucs is not invoked
- **AND** the pad remains playable with full-mix playback

#### Scenario: Explicit FFmpeg directory is used
- **GIVEN** the required Demucs model checkpoint is installed
- **AND** `FLITZIS_FFMPEG_DIR` points to a directory containing `ffmpeg` and `ffprobe`
- **WHEN** the performer requests stem generation
- **THEN** the backend prepends that directory to the Demucs subprocess environment
- **AND** the backend preflights those tools before invoking Demucs

#### Scenario: Missing TorchCodec support fails before Demucs runs
- **GIVEN** the required Demucs model checkpoint is installed
- **AND** `ffprobe` and `ffmpeg` are available
- **AND** TorchCodec cannot load its native libraries
- **WHEN** the performer requests stem generation
- **THEN** the backend reports `TorchCodec unavailable`
- **AND** Demucs is not invoked
- **AND** the pad remains playable with full-mix playback


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

The Demucs backend SHALL pass `--shifts 1` and `--overlap 0.5` unless the Settings page
provides validated replacement values. The control layer SHALL persist the configured values in
project state and SHALL reject Demucs quality parameters outside the app-supported range before
invoking Demucs. The app-supported range SHALL be `shifts` from 1 through 20 and `overlap` from
0.25 through 0.95.

#### Scenario: Default quality parameters are used
- **GIVEN** the required Demucs model, FFmpeg tools, and TorchCodec support are available
- **AND** Demucs is the selected separator
- **AND** no Settings page override has supplied alternate quality parameters
- **WHEN** the performer requests stem generation
- **THEN** the background backend invokes Demucs with `--shifts 1`
- **AND** the background backend invokes Demucs with `--overlap 0.5`

#### Scenario: Settings quality values are used
- **GIVEN** the performer configures Demucs shifts to 4
- **AND** the performer configures Demucs overlap to 0.25
- **WHEN** the performer requests stem generation
- **THEN** the background backend invokes Demucs with `--shifts 4`
- **AND** the background backend invokes Demucs with `--overlap 0.25`

#### Scenario: Invalid settings are rejected before Demucs runs
- **GIVEN** the Settings page supplies an unsupported Demucs quality value such as shifts `0`,
  overlap `0.0`, or overlap `1.0`
- **WHEN** the performer requests stem generation
- **THEN** the control layer rejects the request outside the audio callback
- **AND** Demucs is not invoked
- **AND** the pad remains playable with full-mix playback


<!-- From add-offline-stem-cache -->
### Requirement: Demucs Model Cache And Device Fallback Stay Off The Audio Path
The system SHALL use the configured, isolated model directory and resolve GPU-to-CPU fallback
only on the background generation path.

The source-clone default Demucs model checkpoint directory SHALL be
`models/torch/hub/checkpoints` inside the repository and ignored by Git. Explicit
external model-cache overrides SHALL retain precedence, including managed developer
and installation-owned Portable caches. Model paths inside protected sample,
recording, configuration or source trees SHALL be rejected without a global TEMP fallback. The default `htdemucs` model SHALL require
the `955717e8-8726e21a.th` checkpoint in that directory before UI-driven generation invokes
Demucs. The default device policy SHALL attempt CUDA only when Torch reports it is available, then
retry once on CPU if the CUDA run fails.

#### Scenario: Models are separate from generated stem artifacts
- **GIVEN** a performer starts Demucs stem generation
- **WHEN** the backend prepares model cache configuration
- **THEN** model lookup uses the ignored repo-local models directory or its explicit external override, outside `samples/stems/`
- **AND** the project cache stores only generated stem artifacts

#### Scenario: CUDA failure falls back to CPU
- **GIVEN** Torch reports CUDA is available
- **AND** the Demucs CUDA run fails during initialization, model loading, inference, or GPU memory allocation
- **WHEN** CPU processing can still proceed
- **THEN** the background worker retries the same source-version request on CPU
- **AND** the failure and fallback are reported only through non-audio-thread status or diagnostics


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


