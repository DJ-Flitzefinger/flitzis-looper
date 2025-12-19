## Context
The current `AudioEngine` runs a CPAL output stream and drains a ring buffer of control messages in the audio callback, but it only logs commands and fills the output buffer with silence.

Audio file loading (disk I/O + decode) must never happen in the audio callback. Large sample buffers should not be copied through control messages.

## Goals / Non-Goals

### Goals
- Load short audio files outside the audio thread (decoded via `symphonia`).
- Trigger sample playback from the audio callback using small, fixed-size commands.
- Keep the audio callback real-time safe (no blocking, no allocations, no Python calls).
- Keep implementation minimal and easy to extend.

### Non-Goals
- Streaming long files from disk.
- Background/async loading (initial implementation may block the Python call).
- High-quality resampling/time-stretching.
- Detailed audio-thread feedback messages back to Python (handled in a separate change proposal).
- Guaranteeing support for every audio format/codec (only what `symphonia` can decode with enabled features).
- Sample-accurate scheduling across buffers.

## Decisions

### Decision: Use shared immutable sample buffers (`Arc<...>`) and send handles, not bytes
- `AudioEngine.load_sample(id, path)` will read/decode the file on the Python thread (or another non-audio thread).
- The decoded samples will be stored in an immutable buffer (e.g., `Arc<[f32]>` or `Arc<Vec<f32>>`).
- The audio callback will only ever see an `Arc` handle (shared memory) and an integer `id`.

This satisfies:
- No disk I/O in the audio callback.
- No copying of full sample buffers through the ring buffer (only a small handle is sent).

### Decision: Keep a fixed-size sample bank in the audio callback
- The audio callback will maintain a fixed-size sample bank with `MAX_SAMPLE_SLOTS = 32`.
- For now, this is a fixed constant; later this is tied to the number of sample banks in the UI.
- `ControlMessage::LoadSample` updates a slot by `id`.
- `ControlMessage::PlaySample` spawns a voice referencing the slot.

This avoids:
- Locks in the audio callback.
- Shared mutable state between threads.

### Decision: Fixed-capacity voice mixer to avoid allocations
- The audio callback will keep a fixed-capacity voice list with `MAX_VOICES = 32`.
- When the polyphony limit is reached, additional triggers are dropped (or replace the oldest voice) deterministically.

This ensures:
- No heap allocations when triggering playback.
- Predictable performance.

### Decision: Decode via `symphonia` and keep compatibility rules minimal
To keep the first implementation straightforward:
- Audio decoding uses `symphonia` and converts decoded audio into normalized interleaved `f32` samples.
- Channel mismatch handling:
  - v1: perform only trivial channel mapping (mono↔stereo).
  - Other layouts error.
- Sample rate mismatch handling:
  - v1: require files to match the output stream config OR error.
  - Resampling is deferred unless it becomes necessary.

## Alternatives Considered

### Alternative: Global shared sample bank (e.g., `ArcSwap`) accessed by the audio callback
Pros: `PlaySample { id }` stays extremely small.
Cons: introduces a shared mutable structure and requires careful lock-free publication patterns.

### Alternative: Send file paths to the audio callback
Rejected: disk I/O and allocations would violate real-time constraints.

### Alternative: Copy sample bytes into messages
Rejected: large messages increase ring buffer pressure and add copy overhead.

## Risks / Trade-offs
- Blocking `load_sample` may pause Python/UI during decoding → acceptable for v1; can be moved to a background loader later.
- Limiting polyphony and sample slots requires choosing conservative defaults.
- Logging inside the audio callback (present today) must be removed/avoided for real-time safety.

## Migration Plan
1. Extend the message protocol to support publishing a loaded sample handle to the audio callback.
2. Add the non-real-time file decode path to `AudioEngine.load_sample` (using `symphonia`).
3. Implement the sample bank + voice mixer in the audio callback.
4. Add deterministic unit tests for decoding and mixing.
5. Update docs (`docs/architecture.md`, `docs/message-passing.md`) to match the implemented data flow.

## Open Questions
- Should resampling be required for usability, or can v1 error on mismatched sample rate?