## Context
The `add-offline-stem-cache` change established the real-time-safe stem foundation:
offline/background generation, project-local cache identity, prepared WAV validation outside
the callback, publication by fixed-size control message and shared immutable handles, bounded
audio-thread storage, and prepared-stem rendering fallback through the existing voice path.

The remaining user-facing gap is the visible performance control surface. Current code has
controller state for generation progress and cache availability, durable per-pad
full-mix/all-stems mode plumbing, bounded Rust mix-mode control state, selected-pad sidebar
stem status rendering, Generate Stems button wiring through the controller, and selected-pad
full-mix/all-stems mode controls. The next slice adds bottom-bar selected-pad per-stem mask
controls. Pad-grid indicators remain later work.

## Goals
- Give performers clear stem availability, progress, blocked, and error feedback.
- Route stem generation from the UI through controller actions and existing background-task
  gating.
- Make full-mix playback and prepared-stem playback explicit performer choices.
- Define and implement compact selected-pad per-stem toggle and preset controls for prepared-stem
  playback.
- Persist durable stem cache metadata and durable per-pad stem mix preferences.
- Keep momentary performance gestures transient unless a later change explicitly persists them.
- Keep all audio-thread stem control changes bounded, fixed-size, and real-time safe.

## Non-Goals
- Implementing pad-grid stem indicators or per-stem mute/solo/toggle controls in the current
  implementation slice.
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

### Bottom-Bar Per-Stem Mask Controls
The first performance mask UI is a compact bottom-bar cluster immediately to the right of the
trigger-quantization `BAR` button. It targets the currently selected red-outlined pad, not
necessarily the currently playing pad. Right-click pad stops and middle-click pad selection both
update that selected target.

The control order is:

- `V`: Vocals,
- `D`: Drums,
- `M`: Melody,
- `B`: Bass,
- `I`: Instrumental preset,
- `A`: All Stems preset.

`V`, `D`, `M`, and `B` are freely combinable runtime toggles. `I` and `A` are exclusive display
presets backed by the same bounded enabled-stem mask. `I` maps to Drums + Melody + Bass and mutes
Vocals. `A` maps to Vocals + Drums + Melody + Bass. The `I` button does not mean "play only the
cached `instrumental.wav` file", and `A` does not add `instrumental.wav` as a fifth audible layer.

The button cluster is active only when the selected pad is in all-stems mode and has a current
available prepared stem set. If stems are unavailable or the pad is in full-mix mode, the buttons
are greyed out and non-clickable. Rendering derives this from controller/session snapshots only;
it does not inspect cache directories, compute source-version identity, read files, decode audio,
run inference, or call low-level Rust background task APIs.

The per-stem enabled mask is session-only in this slice. Project persistence still stores stem
cache metadata and durable full-mix/all-stems mode only. A later OpenSpec change can make mask
preferences durable if needed.

### Persistence
Project persistence stores source-version cache metadata and durable stem mix preferences.
Restore revalidates cache metadata against the current source and existing cache files before
marking stems available. Missing cache files, stale source versions, or rejected publication
must not prevent startup or playback.

Transient generation progress, last errors, blocked reasons, and momentary performance
gestures, including bottom-bar per-stem enabled masks, remain session state and are not persisted.

### Audio-Thread Contract
Stem mix controls must update Rust through fixed-size messages such as pad id, source-version
hash/token, mix mode, and enabled stem mask. Messages must not contain file paths, Python objects,
unbounded vectors, or audio payload copies.

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
- Whether all-stems mode should auto-enable immediately after successful generation or remain a
  separate performer action.
- Whether a later production separation model should revise the supported stem-kind set.
- Whether momentary solo/mute gestures should reuse the bottom-bar cluster or add a separate
  performer modifier gesture.
