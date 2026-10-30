## Why

Grid edits can race the control-side reconciliation of a resident loop already adopted by
the callback. A later timing revision must not strand the physical source mirror. Ordinary
source/readiness rejection also currently escapes GUI gestures and terminates the application.

## What Changes

- Reconcile a genuinely adopted resident window using its exact content, source generation,
  physical window and original publication receipt independently of later timing intent.
- Keep all timing/launch authority checks for new playback; storage mirroring grants none.
- Report synchronous pad/editor admission failures through the existing readiness feedback,
  abort dependent restarts and require a fresh gesture after rejection.
- Preserve signed grid origins, physical loop bounds and the sole scheduler/audio path.

## Non-goals

No new DSP, automatic retries, private-data reset, tempo fallback, relaxed source guards or
device/listening acceptance. Unrelated Key Lock start-mode work remains separate.

## Impact

Resident control-side reconciliation and GUI gesture handling, with deterministic native
source/stem regressions and UI failure/recovery tests. No callback I/O, locks or allocation.
