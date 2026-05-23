## Context
The `add-offline-stem-cache` change established the real-time-safe stem foundation:
offline/background generation, project-local cache identity, prepared WAV validation outside
the callback, publication by fixed-size control message and shared immutable handles, bounded
audio-thread storage, and prepared-stem rendering fallback through the existing voice path.

The remaining user-facing gap is the performance control surface. Current code has controller
state for generation progress and cache availability plus a placeholder "Generate Stems"
button, but no specified UI behavior, no explicit full-mix/all-stems selection, and no
durable mix preference model.

## Goals
- Give performers clear stem availability, progress, blocked, and error feedback.
- Route stem generation from the UI through controller actions and existing background-task
  gating.
- Make full-mix playback and prepared-stem playback explicit performer choices.
- Define future per-stem mute, solo, toggle, and revert-to-full-mix controls without
  implementing them in this planning slice.
- Persist durable stem cache metadata and durable per-pad stem mix preferences.
- Keep momentary performance gestures transient unless a later change explicitly persists them.
- Keep all audio-thread stem control changes bounded, fixed-size, and real-time safe.

## Non-Goals
- Implementing the UI, controller methods, Rust messages, or mixer control state in this
  planning slice.
- Running or choosing a production source-separation model.
- Changing the existing prepared-stem cache writer or replacing placeholder artifacts.
- Performing cache validation, file reads, decoding, generation, or inference in the audio
  callback.
- Changing pad trigger quantization, BPM-lock phase anchoring, loop-region semantics, or
  full-mix fallback behavior.

## Proposed Design

### Availability State
The control layer derives a small per-pad stem status from existing project/session state:

- `unavailable`: no current complete cache for the loaded source version,
- `generating`: a stem-generation background task is running for the pad,
- `available`: the current source version has a complete cache and prepared stems are
  eligible for publication/playback,
- `blocked`: generation or publication is currently blocked by playing, loading, analyzing,
  unloading, or a conflicting per-pad task,
- `error`: the last generation or publication attempt failed outside the audio callback.

The UI renders this state from snapshots. It must not inspect cache directories or compute
source-version identity during rendering.

### Generation Entry Point
The selected-pad sidebar owns the first explicit generation action. Activating "Generate
Stems" emits a controller action for the selected loaded pad. The controller applies the
same inactive-pad and per-pad-task gating already specified by `add-offline-stem-cache`.

When generation is rejected because the pad is playing or running a conflicting task, the UI
shows the controller-provided blocked/error status and leaves full-mix playback usable.

### Full Mix And All-Stems Mode
Each pad has a durable stem mix mode:

- `full_mix`: render the original loaded full-mix buffer,
- `all_stems`: render the complete prepared stem set when available, otherwise fall back to
  `full_mix`.

New projects default each pad to `full_mix` to preserve existing playback behavior until the
performer opts into stem playback. If a persisted `all_stems` preference is restored but the
cache is stale, missing, incomplete, or rejected, the control layer shows the mismatch and the
audio path falls back to full mix until a valid prepared set is available again.

### Future Per-Stem Controls
Future mute, solo, and toggle controls should be represented as bounded per-pad bitmasks over
the known stem kinds: vocals, melody, bass, drums, and instrumental. A momentary solo/mute is a
runtime gesture and should remain session-only unless a later spec explicitly makes it durable.

The first UI should not require a complex mixer panel. A selected-pad stem section can provide:

- generation action and progress/error text,
- availability indicator,
- `Full Mix` versus `All Stems` mode selector,
- disabled placeholder or future-ready controls for per-stem mute/solo/toggle when a prepared
  set is unavailable.

### Persistence
Project persistence stores source-version cache metadata and durable stem mix preferences.
Restore revalidates cache metadata against the current source and existing cache files before
marking stems available. Missing cache files, stale source versions, or rejected publication
must not prevent startup or playback.

Transient generation progress, last errors, blocked reasons, and momentary performance
gestures remain session state and are not persisted.

### Audio-Thread Contract
Stem mix controls must update Rust through fixed-size messages such as pad id, source-version
hash/token, mix mode, enabled stem mask, and solo/mute masks. Messages must not contain file
paths, Python objects, unbounded vectors, or audio payload copies.

The audio callback may update bounded audio-thread stem mix state and read already accepted
prepared buffers. It must not generate stems, read cache files, decode, run neural inference,
log, block, allocate stem buffers, acquire the Python GIL, or perform long-running work.

## Risks And Trade-offs
- Defaulting to `full_mix` preserves legacy playback but means available stems require an
  explicit performer choice before they affect output.
- Per-stem controls are useful but can clutter the selected-pad sidebar; the first
  implementation should keep controls compact and state-driven.
- Persisted `all_stems` preferences can become stale after source replacement; restore must
  degrade visibly and safely to full-mix playback.

## Open Questions
- Whether per-stem mute/solo/toggle controls should live only in the selected-pad sidebar or
  also in a compact performance panel.
- Whether all-stems mode should auto-enable immediately after successful generation or remain a
  separate performer action.
- Whether a later production separation model should revise the supported stem-kind set.
