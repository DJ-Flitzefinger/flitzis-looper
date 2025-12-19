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
- Avoids Python/GIL interaction entirely.
- Performs no disk I/O or decoding.
- Mixes audio using fixed-capacity data structures (`MAX_SAMPLE_SLOTS`, `MAX_VOICES`).

Disk I/O and decoding happen in `load_sample(...)`, outside the callback.

## Failure modes and guarantees

- Ring buffer full: `AudioEngine` methods return an error to Python; the audio thread continues unaffected.
- Ring buffer empty (audio → control): `receive_msg()` returns `None`.
- Missing sample slot: triggering playback is ignored safely.

## Not implemented (yet)

- Rich audio → Python event stream (beyond `Pong`).
- Additional control messages (e.g., volume/transport) exposed as stable Python APIs.

## Related specs

- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
