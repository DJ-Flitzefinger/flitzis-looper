## Why

Qualified source maps can already drive the existing mapped launch and reader,
but the application does not publish a separate user SYNC request. Its existing
pitch projection named `sync` means BPM Lock. Map readiness alone also cannot
prove that a voice actually adopted mapped playback.

## What Changes

- Add independent per-content persisted SYNC intent through the existing GUI,
  Learn/MIDI, controller, runtime, Copy/Move/Bank and restore paths.
- Publish only the original freshly qualified map delivery through existing
  source-map rows. A missing map keeps the request pending and cannot silently
  substitute scalar playback.
- Report actual callback-owned mapped voice identity separately from intent and
  editor readiness, preserving an older effective voice while replacement waits.
- Resolve requested automatic mapped bars with the canonical map and retain the
  exact musical extent separately from rounded resident PCM coverage.

## Non-goals

This delivery does not complete prepared Native/FIFO or continuous live mapped
adoption, broaden the currently admitted render tuple, accept uncertain detector
arrays as quarter-note truth, change analyzer defaults, or close musical,
resource, device, hearing, Human or parent gates. Automatic original-fullmix map
acquisition remains required follow-up. No new clock, scheduler, worker pool,
registry, audio pipeline, implicit BPM Lock or Key Lock change is introduced.

## Realtime

Intent, persistence, parsing and map qualification remain off realtime. Actual
voice feedback uses fixed callback-owned scalars and bounded atomic snapshots;
map identity is prepared off realtime. The callback performs no allocation,
I/O, JSON, Python/GIL calls, blocking or final owner destruction.
