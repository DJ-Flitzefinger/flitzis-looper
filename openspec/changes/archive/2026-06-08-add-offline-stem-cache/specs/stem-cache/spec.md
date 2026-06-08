## ADDED Requirements

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

### Requirement: Stem Generation And Replacement Require An Inactive Pad
The system SHALL allow stem generation and stem-buffer replacement only when the target pad
is not currently playing.

If a pad starts playing while stem generation is in progress, the completed result SHALL
NOT replace audio-thread stem buffers for that pad until the pad is stopped and the source
version is still current.

#### Scenario: Playing pad blocks generation
- **GIVEN** a pad is currently playing
- **WHEN** the performer requests stem generation for that pad
- **THEN** the system rejects or defers the request
- **AND** no stem generation work is run for that active pad in the audio callback

#### Scenario: Pad starts during generation
- **GIVEN** stem generation is running for a pad that was inactive when the task started
- **WHEN** the pad starts playing before generation completes
- **THEN** the generated buffers are not published as active stem buffers for that playing pad
- **AND** current full-mix playback continues unchanged

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

### Requirement: Stem Cache Is Pad-Scoped And Deletable
The system SHALL store generated project stem artifacts in a pad-scoped cache directory named
after the pad label under `samples/stems/`.

Pad 1 SHALL use `samples/stems/#1/`, pad 2 SHALL use `samples/stems/#2/`, and the same
numbering pattern SHALL continue through pad 216. Deleting stems SHALL remove only the target
pad's project-local stem cache directory and SHALL clear the pad's tracked stem cache metadata.
Unloading a pad SHALL delete its tracked cached stem artifacts before the pad becomes available
for a different source. Deletion SHALL NOT run inside the audio callback.

#### Scenario: Generated stems use the pad label directory
- **GIVEN** pad `#1` has loaded source audio
- **AND** the pad is not currently playing
- **WHEN** the performer requests stem generation
- **THEN** the background stem backend writes the generated cache artifacts under `samples/stems/#1/`
- **AND** the tracked source version remains the current source version for pad `#1`

#### Scenario: Unload removes pad stems
- **GIVEN** pad `#1` has tracked cached stem artifacts under `samples/stems/#1/`
- **WHEN** the performer unloads audio from pad `#1`
- **THEN** the system deletes `samples/stems/#1/` outside the audio callback
- **AND** pad `#1` no longer exposes those stems as available

#### Scenario: Manual stem deletion preserves full-mix playback
- **GIVEN** pad `#1` has loaded full-mix audio and cached stems
- **WHEN** the performer deletes stems for pad `#1`
- **THEN** the system deletes only pad `#1` stem cache artifacts outside the audio callback
- **AND** pad `#1` remains playable through its full-mix buffer

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

### Requirement: Demucs Quality Parameters Are Bounded
The system SHALL run the default Demucs backend with bounded quality parameters outside the audio
callback.

The default Demucs backend SHALL pass `--shifts 1` and `--overlap 0.5` unless the Settings page
provides validated replacement values. The control layer SHALL persist the configured values in
project state and SHALL reject Demucs quality parameters outside the app-supported range before
invoking Demucs. The app-supported range SHALL be `shifts` from 1 through 20 and `overlap` from
0.25 through 0.95.

#### Scenario: Default quality parameters are used
- **GIVEN** the required Demucs model, FFmpeg tools, and TorchCodec support are available
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

### Requirement: Demucs Model Cache And Device Fallback Stay Off The Audio Path
The system SHALL use Demucs' standard Torch Hub checkpoint cache and resolve GPU-to-CPU fallback
only on the background generation path.

On Windows the default Demucs model checkpoint directory SHALL be
`C:\Users\<YOUR_NAME>\.cache\torch\hub\checkpoints`. The default `htdemucs` model SHALL require
the `955717e8-8726e21a.th` checkpoint in that directory before UI-driven generation invokes
Demucs. The default device policy SHALL attempt CUDA only when Torch reports it is available, then
retry once on CPU if the CUDA run fails.

#### Scenario: Model files are not stored in the project cache
- **GIVEN** a performer starts Demucs stem generation
- **WHEN** the backend prepares model cache configuration
- **THEN** model lookup uses the standard Torch Hub checkpoint cache outside the repository and outside `samples/stems/`
- **AND** the project cache stores only generated stem artifacts

#### Scenario: CUDA failure falls back to CPU
- **GIVEN** Torch reports CUDA is available
- **AND** the Demucs CUDA run fails during initialization, model loading, inference, or GPU memory allocation
- **WHEN** CPU processing can still proceed
- **THEN** the background worker retries the same source-version request on CPU
- **AND** the failure and fallback are reported only through non-audio-thread status or diagnostics
