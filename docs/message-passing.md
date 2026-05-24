# Message Passing

Flitzi's Looper keeps the audio callback real-time safe by communicating with it through fixed-size, single-producer/single-consumer ring buffers.
Python never touches the ring buffers directly; it calls `AudioEngine` methods which enqueue small, fixed-size control messages.

## Channels

Two independent SPSC ring buffers are used:

- Control → audio (Python thread → CPAL callback)
  - Carries control events like “load this sample into slot X” or “trigger slot X”.
- Audio → control (CPAL callback → Python thread)
  - Carries small status messages polled via `receive_msg()`.

Both buffers are fixed-capacity (1024 messages). When a buffer is full, pushing returns an error instead of waiting for space.

The Rust input-mapping layer also uses bounded non-audio callback queues for MIDI events and
diagnostic/Learn events. Those queues are outside the CPAL callback. A mapped MIDI trigger can
only affect audio by sending a bounded control message through the existing control-to-audio path.

## What gets sent

Messages are intentionally small and allocation-free on the audio thread:

- Playback triggers are referenced by `id` and `velocity` (no file paths in the callback).
- Loading publishes decoded sample data via a shared handle; the large sample buffer is not copied just to cross the thread boundary.
- Beatgrid/downbeat publication sends one bounded per-pad timing anchor, not full beat-grid vectors.
- Input mapping publishes stable binding/action keys across the Python/Rust boundary, then uses
  in-memory snapshots for normal MIDI lookup. Mapping JSON is not read or written in the audio
  callback or MIDI hot path.
- `ping()`/`Pong` exists as a minimal end-to-end messaging check.

## Real-time safety rules

The CPAL callback:

- Drains pending messages without blocking.
- Owns and advances the Rust transport timeline by rendered output sample frames.
- Owns the fixed-capacity scheduler used for current-frame playback commands and future
  quantized events.
- Avoids Python/GIL interaction entirely.
- Performs no disk I/O or decoding.
- Does not own MIDI ports, poll keyboards, read mapping JSON, update Learn state, or call input
  mapping UI/controller code.
- Mixes audio using fixed-capacity data structures (`MAX_SAMPLE_SLOTS`, `MAX_VOICES`).

Disk I/O and decoding happen in `load_sample(...)`, outside the callback.

## Failure modes and guarantees

- Ring buffer full: `AudioEngine` methods return an error to Python; the audio thread continues unaffected.
- Ring buffer empty (audio → control): `receive_msg()` returns `None`.
- Missing sample slot: triggering playback is ignored safely.

## Low-jitter input mapping messages

The low-jitter input mapping work is specified in
`openspec/changes/add-low-jitter-input-mapping/`. MIDI capture is intentionally separated from the
CPAL callback:

- The MIDI backend callback timestamps each supported message with a monotonic timestamp as soon
  as practical.
- The MIDI callback normalizes only Note On velocity greater than zero and Control Change for
  version 1. It drops Note On velocity zero, Active Sensing, MIDI Clock, SysEx, Program Change,
  Pitch Bend, Aftertouch, and MPE-style messages.
- Normalized MIDI input is sent through a bounded queue to a Rust dispatcher thread.
- The dispatcher resolves the latest in-memory mapping snapshot. It does not read JSON.
- Direct audio-safe mappings, such as pad trigger, pad stop, and stop-all, are bridged through the
  existing bounded control-to-audio ring buffer.
- Controller-owned mappings are reported back to Python as small event dictionaries containing
  source, binding key, monotonic timestamp, action key, and dispatch flags.

This path must not simulate mouse clicks, call Python from the audio callback, route MIDI directly
into callback functions, block the callback, log from the callback, or allocate unbounded audio
thread state.

## Gen3 transport and scheduler messages

The Gen3 transport work is specified in
`openspec/changes/add-rust-transport-timeline/`. The initial output-frame transport timeline
now exists inside the audio callback. The fixed-capacity scheduler helper now exists with
unit-tested ordering and rejection semantics, and the callback owns a scheduler for
current-frame `PlaySample`, `StopSample`, and `StopAll` routing. Rust-side trigger
quantization now accepts `immediate` plus fixed grid-step updates `1_16`, `1_32`, and `1_64`
through the existing control-to-audio ring buffer. Legacy `next_beat` and `next_bar` aliases
remain accepted by the Python-facing API for older callers and map to `1_16`.
Per-pad beatgrid/downbeat timing metadata is published as one fixed-size
`SetPadTimingMetadata` request containing the same finite non-negative phase anchor used by the
waveform editor grid and prepared outside the callback. MultiLoop-disabled playback uses a single fixed-size
`PlaySampleExclusive` request, which the audio thread turns into one stop-all-then-play scheduled
command when quantization is enabled. The design keeps the existing SPSC ring-buffer architecture.

Transport and quantized scheduler messages must remain fixed-size and bounded.
Python/control code requests transport or trigger-quantization changes through the existing
control-to-audio path; the audio callback owns the Rust transport timeline, per-pad timing
metadata state, trigger quantization mode, and fixed-capacity scheduler.
The performance UI exposes trigger quantization as a bottom-bar `Q` toggle and a Settings-page
grid selector. The control layer publishes only fixed-size `SetTriggerQuantization` messages:
`immediate` when `Q` is disabled, or the selected grid step when `Q` is enabled.

The active `add-phase-aware-playback-sync` change keeps the same messaging rule. Quantized
scheduled playback now stores a fixed-size optional target bar phase inside the audio-thread
scheduler command, derived from the scheduled output frame. That descriptor is not a new
Python-to-Rust ring-buffer message. BPM-lock phase anchoring uses a fixed-size
`AnchorTransportPhaseFromPad { id }` control message. The audio thread derives transport phase
from already-owned mixer and transport state; Python must not send full beat-grid vectors, file
paths, heap-owned data, or direct scheduler/transport pointers.

Two failure points are distinct:

- Control ring buffer full: the request never reaches the audio callback and follows the
  existing Python-facing error/drop behavior.
- Scheduler full: the request reached the audio callback, but the fixed-capacity scheduler
  rejects it without evicting existing events, stopping currently playing pads, blocking,
  allocating, logging, touching disk, or acquiring the Python GIL.

## Gen3 offline stem-cache messages

The Gen3 stem-cache planning work is specified in
`openspec/changes/add-offline-stem-cache/`. Stem generation is offline/background work for
inactive pads only. Future stem publication must send fixed-size descriptors and immutable
audio-buffer handles through the existing control-to-audio ring buffer; messages must not contain
file paths, Python objects, unbounded metadata vectors, or copied full stem payloads.

The current implementation keeps stem cache work in control-plane/background code. Python
computes a source-version token outside the callback and starts a Python-side background
stem-generation backend only after rejecting active, loading, analyzing, duplicate, or
missing-source pads. The production backend boundary is file/artifact based: the request contains
the source path, pad id, source version, project-local target cache directory, loaded sample shape,
model cache directory, and device policy; the result contains only small backend/model/device
diagnostics for non-audio-thread surfaces.

The first production adapter invokes Demucs outside Rust and outside the CPAL callback. It maps
Demucs `other.wav` to project `melody.wav`, derives `instrumental.wav` from the final aligned
drums, bass, and melody artifacts, and writes the same pad-scoped
`samples/stems/#<pad-number>/` cache files used by the existing prepared-publication path.
User-facing setup commands are in
`docs/stem-generation-setup.md`. Torch/Demucs are not imported during app startup. Model lookup
uses Demucs' standard Torch Hub checkpoint cache outside the repository and outside project
samples. If the expected `htdemucs` checkpoint is missing, the backend reports `no Model
installed` before invoking Demucs. With the default `auto` device policy, CUDA is attempted only
when Torch reports it is available, and a CUDA failure retries the same background request on CPU.
The backend also probes `ffprobe` and `ffmpeg` before invoking Demucs; inaccessible or missing
executables produce the short error `FFmpeg/ffprobe unavailable`. Lookup uses the inherited
process `PATH`, `FLITZIS_FFMPEG_DIR`, or local WinGet `Gyan.FFmpeg*` package installs, and the
resolved directory is prepended to the Demucs subprocess `PATH`. It also probes TorchCodec because
Torchaudio uses it for output writing in the current environment; native-library failures produce
`TorchCodec unavailable` before Demucs starts.
The Demucs request also carries bounded quality parameters. The first production defaults are
`--shifts 10` and `--overlap 0.5`, with app-supported settings ranges of `shifts` 1 through 20
and `overlap` 0.25 through 0.95. The Settings page writes validated values to project state, and
the controller copies those values into the backend request before the background task starts.

Rust still exposes `AudioEngine.generate_stems_async(id, source_version, cache_dir)` as a
deterministic engine-level cache writer used by low-level validation; it is not the production
Demucs path. Both paths keep generation, model download, disk I/O, decoding, and heavy allocation
outside the audio callback; the production UI path no longer starts model download and expects the
model to be installed ahead of time.

After a successful generation task, Python revalidates the source version, inactive pad state, and
complete cache files before calling `AudioEngine.publish_prepared_stems(...)`. Rust validates the
cached WAV artifacts outside the callback for sample rate, channel layout, frame origin by cache
convention, and frame length, then sends one `PublishPreparedStems` control message containing
bounded metadata plus shared immutable buffer handles. If the control ring buffer is full, the
publication request fails outside the callback and full-mix playback is unchanged. If the message
reaches the callback after the pad has become active or stale, the callback rejects the publication
without touching full-mix playback.

The audio callback can now accept already prepared stem handles into bounded audio-thread state.
It also accepts a fixed-size `SetStemMixMode` update containing only the pad id, a
full-mix/all-stems mode, and a source-version hash. New and older projects default to full mix, so
prepared stems are not rendered until the control layer selects all-stems mode for the matching
prepared source version. If the set is missing, stale, incomplete, rejected, disabled, or fails
bounded render-shape checks, rendering falls back to the loaded full-mix buffer. The callback must
not generate stems, read cache files, decode audio, run neural inference, allocate stem buffers,
log, block, or acquire the Python GIL.

The selected-pad sidebar and compact pad-grid badges now render stem availability, progress,
blocked, and error state from controller snapshots, route Generate Stems through
`StemController.generate_stems_async(...)`, and route full-mix/all-stems changes through
`StemController.set_stem_mix_mode(...)`. It also routes Delete Stems through
`StemController.delete_stems(...)`; Unload Audio uses the same controller path to delete tracked
pad stem cache artifacts outside the audio callback. Rendering does not inspect cache
directories, read files, decode audio, run inference, or call the low-level Rust background task
APIs directly.

The follow-up `openspec/changes/add-stem-performance-controls/` planning slice defines performer
stem controls and future momentary solo/mute gestures. The bottom-bar selected-pad stem mask slice publishes
only fixed-size bounded state: pad id, source-version hash, and an enabled component-stem mask.
`V`, `D`, `M`, and `B` toggle component stems, while `I` maps to Drums + Melody + Bass and `A`
maps to Vocals + Drums + Melody + Bass. UI preset display state remains explicit: component clicks
from `I` or `A` publish a custom mask containing only the clicked component, and matching custom
masks do not implicitly publish preset display state. The UI keeps a session-only remembered
component mask for the `V`/`D`/`M`/`B` group; switching between `I` and `A` does not overwrite that
remembered mask, and deactivating the active preset publishes it back as a custom component mask.
Neither preset asks the audio callback to read or select the cached `instrumental.wav` artifact
directly. Right-clicking `V`, `D`, `M`, or `B` publishes a custom mask containing only that
component and keeps that state until the performer changes the buttons again. The messages must
not carry file paths, Python objects, unbounded metadata, or copied audio payloads.
Ring-buffer-full or stale-source failures must leave current full-mix or stem playback unchanged.

The Settings overlay is a UI/control-plane surface only. Opening it, closing it, or changing
Demucs shifts/overlap does not send any new Rust control message and does not call stem generation
from the render loop. The configured values affect the next
`StemController.generate_stems_async(...)` request, which still uses the same inactive-pad gating
and background backend path.

## Not implemented (yet)

- Rich audio → Python event stream (beyond `Pong`).

## Related specs

- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
- `openspec/changes/add-rust-transport-timeline/`
- `openspec/changes/add-phase-aware-playback-sync/`
- `openspec/changes/add-offline-stem-cache/`
- `openspec/changes/add-low-jitter-input-mapping/`
