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

## What gets sent

Messages are intentionally small and allocation-free on the audio thread:

- Playback triggers are referenced by `id` and `velocity` (no file paths in the callback).
- Loading publishes decoded sample data via a shared handle; the large sample buffer is not copied just to cross the thread boundary.
- Beatgrid/downbeat publication sends one bounded per-pad timing anchor, not full beat-grid vectors.
- `ping()`/`Pong` exists as a minimal end-to-end messaging check.

## Real-time safety rules

The CPAL callback:

- Drains pending messages without blocking.
- Owns and advances the Rust transport timeline by rendered output sample frames.
- Owns the fixed-capacity scheduler used for current-frame playback commands and future
  quantized events.
- Avoids Python/GIL interaction entirely.
- Performs no disk I/O or decoding.
- Mixes audio using fixed-capacity data structures (`MAX_SAMPLE_SLOTS`, `MAX_VOICES`).

Disk I/O and decoding happen in `load_sample(...)`, outside the callback.

## Failure modes and guarantees

- Ring buffer full: `AudioEngine` methods return an error to Python; the audio thread continues unaffected.
- Ring buffer empty (audio → control): `receive_msg()` returns `None`.
- Missing sample slot: triggering playback is ignored safely.

## Gen3 transport and scheduler messages

The Gen3 transport work is specified in
`openspec/changes/add-rust-transport-timeline/`. The initial output-frame transport timeline
now exists inside the audio callback. The fixed-capacity scheduler helper now exists with
unit-tested ordering and rejection semantics, and the callback owns a scheduler for
current-frame `PlaySample`, `StopSample`, and `StopAll` routing. Rust-side trigger
quantization now accepts `immediate`, `next_beat`, and `next_bar` mode updates through the
existing control-to-audio ring buffer. Per-pad beatgrid/downbeat timing metadata is published as
one fixed-size `SetPadTimingMetadata` request containing a finite non-negative phase anchor
prepared outside the callback. MultiLoop-disabled playback uses a single fixed-size
`PlaySampleExclusive` request, which the audio thread turns into one stop-all-then-play scheduled
command when quantization is enabled. The design keeps the existing SPSC ring-buffer architecture.

Transport and quantized scheduler messages must remain fixed-size and bounded.
Python/control code requests transport or trigger-quantization changes through the existing
control-to-audio path; the audio callback owns the Rust transport timeline, per-pad timing
metadata state, trigger quantization mode, and fixed-capacity scheduler.
The performance UI exposes the current supported trigger modes through controller actions that
publish only fixed-size `SetTriggerQuantization` messages.

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
computes a source-version token outside the callback and calls
`AudioEngine.generate_stems_async(id, source_version, cache_dir)` only after rejecting active,
loading, analyzing, duplicate, or missing-source pads. Rust models `stem_generation` as a per-pad
background task kind and reports started/progress/success/error events through
`poll_loader_events()`. The current task writes deterministic aligned WAV cache artifacts under
`samples/stems/<source-version-hash>/` outside the audio callback: `instrumental.wav` contains the
full mix, while the other expected stem files are silence placeholders.

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

The selected-pad sidebar now renders stem availability/progress/error state from controller
snapshots, routes Generate Stems through `StemController.generate_stems_async(...)`, and routes
full-mix/all-stems changes through `StemController.set_stem_mix_mode(...)`. Rendering does not
inspect cache directories, read files, decode audio, run inference, or call the low-level Rust
background task APIs directly.

The follow-up `openspec/changes/add-stem-performance-controls/` planning slice defines performer
stem controls and future momentary solo/mute gestures. The bottom-bar selected-pad stem mask slice publishes
only fixed-size bounded state: pad id, source-version hash, and an enabled component-stem mask.
`V`, `D`, `M`, and `B` toggle component stems, while `I` maps to Drums + Melody + Bass and `A`
maps to Vocals + Drums + Melody + Bass. Neither preset asks the audio callback to read or select
the cached `instrumental.wav` artifact directly. The messages must not carry file paths, Python
objects, unbounded metadata, or copied audio payloads. Ring-buffer-full or stale-source failures
must leave current full-mix or stem playback unchanged.

## Not implemented (yet)

- Rich audio → Python event stream (beyond `Pong`).

## Related specs

- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
- `openspec/changes/add-rust-transport-timeline/`
- `openspec/changes/add-phase-aware-playback-sync/`
- `openspec/changes/add-offline-stem-cache/`
