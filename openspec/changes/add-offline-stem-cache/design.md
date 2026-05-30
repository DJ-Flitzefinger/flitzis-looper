## Context
The current engine already separates blocking work from real-time playback. Sample loading,
decoding, resampling, and analysis happen outside the CPAL callback, then immutable audio
data and bounded metadata are published to Rust. The audio callback owns fixed sample slots,
fixed voice slots, transport timing, scheduling, and mixing.

Stem separation is a heavier operation than the existing analysis pipeline. It may require
neural inference, large intermediate buffers, cache files, and progress reporting. None of
that can run in the audio callback. The callback can only mix audio buffers that have
already been prepared and published into audio-thread-owned bounded state.

## Goals
- Define offline/cache-based stem generation before implementation.
- Keep stem generation available only for pads that are not currently playing.
- Prevent a completed stem task from replacing buffers for a pad that became active during
  generation.
- Define immutable prepared stem buffers that are aligned with the source pad audio.
- Preserve current full-mix playback when stems are missing, stale, incomplete, or failed.
- Keep future stem publication compatible with fixed-size control messages.
- Keep future stem mixing real-time safe: no disk I/O, no Python/GIL access, no blocking,
  no logging, no heap allocation, no neural inference, and no long-running work.

## Non-Goals
- Implementing stem generation, model inference, or cache file formats in this planning
  slice.
- Automatically downloading model weights from the Looper UI.
- Adding performer-facing stem controls or UI indicators in this change.
- Changing transport quantization, phase-aware playback, loop-region behavior, or BPM-lock
  behavior.
- Generating or replacing stem buffers for pads that are currently playing.
- Performing any stem work inside the audio callback beyond mixing already prepared buffers.

## Proposed Design

### Stem Model
Represent a generated stem set as five expected stem kinds:

- vocals,
- melody,
- bass,
- drums,
- instrumental.

Each generated stem buffer must be aligned to the pad's source audio after project-local
decoding/resampling:

- same output sample rate as the loaded pad buffer,
- same channel layout used by the mixer,
- same frame origin as the full mix,
- frame length suitable for sharing the pad voice playhead and loop-region math.

The cache should record enough source identity to detect stale stems after a pad is
reloaded or replaced. The exact identity can be a content hash, cached sample path plus file
metadata, or another deterministic project-local version token, but it must not require the
audio callback to read files or compute hashes.

### Background Generation
Stem generation is a manual per-pad background task. It must be rejected or deferred when:

- the pad is currently playing,
- the pad is loading or unloading,
- another conflicting per-pad task is running,
- the pad has no loaded source audio.

Generation may use disk I/O, neural inference, temporary files, and allocation only in
background/control-plane code. It should report progress through the existing loader or
background-task event path, using task/stage names that remain stable enough for the UI.

If a pad starts playing while generation is in progress, the task may finish writing cache
artifacts outside the callback, but publication/replacement of audio-thread stem buffers
must be rejected or deferred until the pad is stopped and the source version still matches.

### Cache Lifecycle
Generated stem artifacts are project-local cache data, not production source files. The cache
directory is pad-scoped (`samples/stems/#1/` through `samples/stems/#216/`) while source-version
metadata still determines whether files are eligible for the currently loaded source. Loading a
project may discover existing valid cached stems and mark them available without regenerating.
Replacing or unloading a pad must mark that pad's stem cache unavailable for the old source
version and must not leave stale stems eligible for playback. Unloading a pad or explicitly
deleting stems removes the tracked pad cache directory outside the audio callback.

Cache cleanup can be best-effort outside the audio callback. Missing cache files must not
crash project load, sample unload, or playback.

### Initial Artifact Writer
The first implementation writes deterministic project-local WAV artifacts under the
pad-scoped `samples/stems/#<pad-number>/` cache directory. The task runs on a Rust
background thread from the already decoded and resampled `SampleBuffer`, so every written
artifact uses the mixer output sample rate, the mixer channel layout, the same frame origin,
and the same frame length as the loaded full-mix buffer.

This writer intentionally does not choose or run a neural separation model. It remains useful for
low-level engine validation: `instrumental.wav` contains the aligned full mix and `vocals.wav`,
`melody.wav`, `bass.wav`, and `drums.wav` contain aligned silence placeholders. Production source
separation is provided by the later Demucs backend slice, which writes the same cache contract from
Python-side background code before using the same prepared-publication path.

### Prepared Publication Slice
After generation completes, Python revalidates the source version, the current pad playback
state, and complete cache files before requesting Rust publication. Rust then validates the
cached WAV artifacts outside the audio callback against the currently loaded full-mix buffer:
sample rate, channel layout, zero-offset frame origin by cache convention, non-empty usable
length, and exact frame count must match before any handle is published.

The publication request sends one fixed-size control message containing bounded metadata and
shared immutable buffer handles. The audio callback accepts the handles into bounded
per-pad/per-stem storage only when the pad is still loaded and inactive; otherwise it rejects the
publication without touching full-mix playback.

### Prepared Stem Mixing Slice
The mixer can now use accepted prepared stem sets as the render source for a pad voice. When a
valid complete set is present, the audio callback fills the existing per-voice stretch input
buffers by summing the bounded prepared stem handles at the same frame/channel positions that the
full-mix buffer would have used. This keeps loop regions, transport-scheduled start frames,
BPM-lock tempo ratio handling, key-lock stretch processing, EQ/gain, peak metering, and playhead
updates on the same path as full-mix playback.

When prepared stems are unavailable, stale, incomplete, rejected, or fail bounded render-shape
checks, rendering falls back to the loaded full-mix `SampleBuffer`. This slice adds no performer
UI, no stem mute/solo/toggle state, no production source-separation model, and no audio-callback
disk I/O, Python/GIL access, logging, blocking waits, neural inference, or stem-buffer allocation.

### Publication To Rust
Prepared stem buffers are published to Rust only after background generation and validation
complete. The control-to-audio message should contain bounded scalar metadata and shared
buffer handles, not file paths or full audio payload copies.

The audio callback may store the accepted handles in fixed per-pad/per-stem slots. It must
not allocate, decode, log, touch disk, acquire the Python GIL, or run model inference while
accepting or mixing stems.

If the control ring buffer is full or Rust rejects a stale generation/version, the failure
must not affect current full-mix playback.

### Prepared Stem Mixing
Future stem mixing should use the existing voice playhead and loop-region model. A voice
that is playing a pad has one musical/sample-frame position; full-mix and stem buffers are
read from the same position so stems remain synchronized with the loop.

Stem mute/solo/toggle controls are intentionally outside this planning slice, but any
future control must update bounded audio-thread stem mix state through fixed-size messages.
The callback must never respond to a toggle by generating, loading, decoding, or reading
stem files.

When stems are unavailable, stale, incomplete, or disabled, the engine must preserve current
full-mix playback behavior.

### Production Demucs Backend Slice
The first production source-separation backend is Demucs, but it is isolated behind a
replaceable Python control-plane backend boundary. The durable integration contract remains the
same project-local cache layout and Rust publication path:

1. Python validates that the pad is loaded, inactive, and not already running a conflicting task.
2. Python captures the current source version and loaded sample shape from Rust outside the audio
   callback.
3. A background worker runs the configured stem-generation backend with source path, pad id,
   source version, target cache directory, target sample rate/channel count/frame count, model
   cache directory, and device policy.
4. The backend writes `vocals.wav`, `melody.wav`, `bass.wav`, `drums.wav`, and `instrumental.wav`
   under `samples/stems/#<pad-number>/`.
5. Python reuses the existing completion path: current-source and inactive-pad checks, complete
   cache-file checks, then `AudioEngine.publish_prepared_stems(...)`.

The Demucs adapter runs outside Rust and outside the CPAL callback. It invokes Demucs lazily on
the background generation path, so app startup and unrelated tests do not import Torch or Demucs.
The adapter maps Demucs `other.wav` to project `melody.wav`, and writes `instrumental.wav` by
summing the final aligned `drums + bass + melody` artifacts. The cached `instrumental.wav`
remains a cache artifact; performer-facing `I` still means the explicit Drums + Melody + Bass
preset and does not select only `instrumental.wav`.

Demucs is declared as an application runtime dependency. The model weights are not vendored and
are not stored in project samples. The Looper does not start model download from the UI; the user
must install the selected Demucs model before using Generate Stems. If the expected checkpoint is
missing, the backend returns the short error `no Model installed` and does not invoke Demucs. If
the active Python environment has not been synced and `python -m demucs` is unavailable, that is
reported as a normal background generation error and does not affect full-mix playback.
Because Demucs uses external `ffprobe` and `ffmpeg` executables for audio probing/decoding, the
backend checks those tools before invoking Demucs and returns `FFmpeg/ffprobe unavailable` when
Windows cannot execute them.
Tool lookup first honors the process `PATH`, then an explicit `FLITZIS_FFMPEG_DIR` directory,
then local WinGet `Gyan.FFmpeg*` package installs. When a directory is resolved, it is prepended
to the Demucs subprocess `PATH` so Demucs and TorchAudio see the same tools that were preflighted.
The current Torchaudio output path also requires TorchCodec for saving WAVs, so the backend checks
TorchCodec import/native-library loading before invoking Demucs and returns `TorchCodec
unavailable` when that dependency is not usable.

The adapter uses high-quality Demucs defaults for the first production implementation:
`--shifts 1` and `--overlap 0.5`. These are stored on the backend request as bounded quality
parameters rather than hard-coded backend behavior, so the Settings page can supply validated
replacement values without changing the backend boundary. App-supported settings are `shifts`
from 1 through 20 and `overlap` from 0.25 through 0.95; invalid values are rejected in controller
state before Demucs starts.

The adapter postprocesses Demucs output before publication. If Demucs output differs from the
loaded full-mix buffer, the backend resamples, adapts channels, trims, or pads final cache WAVs so
Rust's prepared-stem validator sees the same sample rate, channel count, frame origin, and exact
frame count as the loaded full mix.

Model caches are kept out of the repository and out of project samples. The backend uses Demucs'
standard Torch Hub checkpoint directory. On Windows this is
`C:\Users\<YOUR_NAME>\.cache\torch\hub\checkpoints`; for the default `htdemucs` model the expected
checkpoint is `955717e8-8726e21a.th`. The backend configures `TORCH_HOME` only to keep Demucs on
that standard cache path. The default device policy is `auto`: use CUDA only when Torch reports it
is available, then retry once on CPU if the CUDA run fails.

This slice does not add real-time separation, model training or fine-tuning, new stem UI states,
component right-click solo behavior, or any audio-callback disk I/O, Python/GIL access, logging,
blocking waits, heap allocation, neural inference, or long-running work.

## Risks And Trade-offs
- Stem cache identity needs to be stable enough to avoid stale playback but cheap enough to
  compute outside the callback.
- Separation quality and model runtime can vary widely. This design intentionally isolates
  model choice from the real-time mixer contract.
- Publishing several large buffers by handle is safer than copying audio through messages,
  but it still requires clear lifetime ownership and stale-generation rejection.
- UI controls should be designed after the cache/publication model is implemented and
  tested; otherwise the UI could promise real-time behavior that the engine cannot safely
  provide.

## Open Questions
- Whether stem generation is implemented in Rust, Python, or an external worker process.
- Whether availability/progress events reuse `LoaderEvent` or introduce a separate
  background-task event API.
- Exact first performer-facing stem controls after prepared mixing is available.
