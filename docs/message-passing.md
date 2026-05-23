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
- `ping()`/`Pong` exists as a minimal end-to-end messaging check.

## Real-time safety rules

The CPAL callback:

- Drains pending messages without blocking.
- Owns and advances the Rust transport timeline by rendered output sample frames.
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
unit-tested ordering and rejection semantics, while callback ownership and message routing
remain planned. The design keeps the existing SPSC ring-buffer architecture.

Planned transport and scheduler messages must remain fixed-size and bounded. Python/control
code will request transport changes or quantized triggers through the existing
control-to-audio path; the audio callback will own the Rust transport timeline and the
fixed-capacity scheduler.

Two failure points are distinct:

- Control ring buffer full: the request never reaches the audio callback and follows the
  existing Python-facing error/drop behavior.
- Scheduler full: the request reached the audio callback, but the fixed-capacity scheduler
  rejects it without evicting existing events, stopping currently playing pads, blocking,
  allocating, logging, touching disk, or acquiring the Python GIL.

## Not implemented (yet)

- Rich audio → Python event stream (beyond `Pong`).
- Callback routing for quantized triggers and scheduler-specific control messages.

## Related specs

- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
- `openspec/changes/add-rust-transport-timeline/`
