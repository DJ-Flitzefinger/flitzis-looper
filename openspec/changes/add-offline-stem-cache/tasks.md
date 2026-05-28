## 1. Specification And Planning
- [x] 1.1 Create the OpenSpec proposal, design, tasks, and spec deltas for offline/cached stems.
- [x] 1.2 Update relevant planning documentation without implementing production feature code.
- [x] 1.3 Run `openspec validate add-offline-stem-cache --strict`.

## 2. Stem Cache Model
- [x] 2.1 Define a project-local stem cache layout and source-version identity.
- [x] 2.2 Represent the expected stem kinds: vocals, melody, bass, drums, and instrumental.
- [x] 2.3 Mark cached stems unavailable when the source pad is unloaded or replaced.
- [x] 2.4 Treat stale cache use for a replaced pad as a blocker before merge.
- [x] 2.5 Store generated project stem artifacts in pad-scoped `samples/stems/#<pad>/` directories.
- [x] 2.6 Delete tracked pad stem cache artifacts when a pad is unloaded.

## 3. Background Stem Generation
- [x] 3.1 Add a manual per-pad background stem generation task.
- [x] 3.2 Reject or defer generation when the pad is currently playing, loading, unloading, or running a conflicting task.
- [x] 3.3 Ensure neural inference, disk I/O, temporary files, and heavy allocation run only outside the audio callback.
- [x] 3.4 Report progress and errors without freezing the UI.
- [x] 3.5 Add Python/controller tests for task gating, progress, failure, and stale-source behavior.
- [x] 3.6 Write deterministic aligned stem cache artifacts outside the audio callback without Rust publication or mixer changes.

## 4. Prepared Stem Publication
- [x] 4.1 Validate generated stem buffers for sample rate, channel layout, frame origin, and usable length before publication.
- [x] 4.2 Publish prepared immutable stem buffers to Rust by fixed-size control message and shared buffer handles.
- [x] 4.3 Reject stale generation results if the pad started playing or the source version changed before publication.
- [x] 4.4 Add Rust/Python tests for fixed-size publication, ring-buffer-full failure, and stale-generation rejection.

## 5. Prepared Stem Mixing
- [x] 5.1 Add fixed per-pad/per-stem storage in Rust audio-thread state.
- [x] 5.2 Mix prepared stems using the same voice playhead, loop region, transport timing, BPM-lock, and key-lock behavior as full-mix playback.
- [x] 5.3 Fall back to full-mix playback when stems are missing, stale, incomplete, or disabled.
- [x] 5.4 Treat any audio-thread disk I/O, Python/GIL access, logging, blocking operation, heap allocation, neural inference, or long-running work as a blocker.
- [x] 5.5 Add deterministic Rust mixer tests for synchronization, fallback, and real-time-safe state transitions.

## 6. Future UI And Persistence Follow-up
- [x] 6.1 Design performer-facing stem availability indicators and controls in a separate OpenSpec slice (`add-stem-performance-controls`).
- [x] 6.2 Design project persistence for stem cache metadata after the cache identity is implemented (`add-stem-performance-controls`).

## 7. Validation
- [x] 7.1 Run official OpenSpec validation for this change before implementation is considered complete.
- [x] 7.2 Run focused Rust and Python tests for any implementation slice that changes code.
- [x] 7.3 Run full uv-managed Rust/Python validation before merging production stem behavior.

## 8. Production Demucs Backend
- [x] 8.1 Introduce a replaceable stem-generation backend boundary before Demucs-specific logic.
- [x] 8.2 Add a Demucs backend adapter that runs outside Rust and imports/runs neural dependencies only on the background generation path.
- [x] 8.3 Use Demucs' standard Torch Hub checkpoint directory outside the repository and outside `samples/`.
- [x] 8.4 Implement the default `auto` device policy with CUDA detection and CPU fallback.
- [x] 8.5 Map Demucs `other.wav` to project `melody.wav` and derive `instrumental.wav` from aligned non-vocal stems.
- [x] 8.6 Postprocess backend outputs so final cache WAVs match the loaded full-mix sample rate, channel count, and frame count.
- [x] 8.7 Add fake-backend and monkeypatched tests that avoid model downloads and neural inference.
- [x] 8.8 Keep component right-click solo, model training/fine-tuning, and new visible download UI out of this slice.
- [x] 8.9 Declare Demucs as an application runtime dependency so model installation state is checked separately from package installation.
- [x] 8.10 Require the default `htdemucs` checkpoint to be preinstalled and return `no Model installed` without invoking Demucs when it is missing.
- [x] 8.11 Declare TorchCodec as a runtime dependency for Demucs/Torchaudio WAV output and preflight `ffprobe`/`ffmpeg` before invoking Demucs.
- [x] 8.12 Run Demucs with defaults `--shifts 4` and `--overlap 0.5`.
- [x] 8.13 Model Demucs quality settings as bounded request parameters for Settings page control.
- [x] 8.14 Resolve FFmpeg tools from `PATH`, `FLITZIS_FFMPEG_DIR`, or local WinGet `Gyan.FFmpeg*` installs before preflight.
- [x] 8.15 Feed validated Settings page Demucs quality values into the backend request before generation starts.
