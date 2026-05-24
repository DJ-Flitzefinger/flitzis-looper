## Context
The `add-offline-stem-cache` change established the real-time-safe stem foundation:
offline/background generation, project-local cache identity, prepared WAV validation outside
the callback, publication by fixed-size control message and shared immutable handles, bounded
audio-thread storage, and prepared-stem rendering fallback through the existing voice path.

The remaining user-facing gap is the visible performance control surface. Current code has
controller state for generation progress and cache availability, durable per-pad
full-mix/all-stems mode plumbing, bounded Rust mix-mode control state, selected-pad sidebar
stem status rendering, Generate Stems button wiring through the controller, selected-pad
full-mix/all-stems mode controls, bottom-bar selected-pad per-stem mask controls, and compact
pad-grid stem indicators. This slice also adds the first Settings overlay surface so Demucs stem
quality can be adjusted without changing the backend boundary.

## Goals
- Give performers clear stem availability, progress, blocked, and error feedback.
- Route stem generation from the UI through controller actions and existing background-task
  gating.
- Make full-mix playback and prepared-stem playback explicit performer choices.
- Add selected-pad stem deletion without unloading the full-mix audio.
- Define and implement compact selected-pad per-stem toggle and preset controls for prepared-stem
  playback.
- Add non-momentary right-click solo setting for `V`/`D`/`M`/`B` without adding a separate mute
  feature.
- Add a Settings page overlay with bounded Demucs stem-quality controls.
- Persist durable stem cache metadata and durable per-pad stem mix preferences.
- Persist Demucs stem-quality settings.
- Keep momentary performance gestures transient unless a later change explicitly persists them.
- Keep all audio-thread stem control changes bounded, fixed-size, and real-time safe.

## Non-Goals
- Implementing a separate stem mute feature in this slice.
- Running or choosing a production source-separation model.
- Changing the existing prepared-stem cache writer or replacing placeholder artifacts.
- Downloading Demucs models from the Settings page.
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
source-version identity during rendering. The pad grid renders compact badges from that same
in-memory status, while detailed status text remains in the selected-pad sidebar.

### Generation Entry Point
The selected-pad sidebar owns the first explicit generation action. Activating "Generate
Stems" emits a controller action for the selected loaded pad. The controller applies the
same inactive-pad and per-pad-task gating already specified by `add-offline-stem-cache`.

When generation is rejected because the pad is playing or running a conflicting task, the UI
shows the controller-provided blocked/error status and leaves full-mix playback usable.

### Stem Deletion Entry Point
The selected-pad sidebar exposes "Delete Stems" directly under "Generate Stems". Activating it
routes through the stem controller, deletes the selected pad's tracked project-local stem cache
directory, clears its cache metadata, returns the pad to full-mix mode, and leaves the loaded
full-mix sample available. The render loop decides button availability from controller/session
snapshots and does not inspect cache directories.

### Full Mix And All-Stems Mode
Each pad has a durable stem mix mode:

- `full_mix`: render the original loaded full-mix buffer,
- `all_stems`: render the complete prepared stem set when available, otherwise fall back to
  `full_mix`.

New projects default each pad to `full_mix` to preserve existing playback behavior until the
performer opts into stem playback. The selected-pad full-mix/all-stems buttons are disabled when
the selected pad has no current prepared stems, so stale or deleted stems cannot persist a new
all-stems preference. If a persisted `all_stems` preference is restored but the cache is stale,
missing, incomplete, or rejected, the control layer shows the mismatch and the audio path falls
back to full mix until a valid prepared set is available again.

### Bottom-Bar Per-Stem Mask Controls
The first performance mask UI is a compact bottom-bar cluster immediately to the right of the
trigger-quantization `Q` button. It targets the currently selected red-outlined pad, not
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
Preset display state is explicit. Clicking `V`, `D`, `M`, or `B` while `I` or `A` is active leaves
the preset display state, enters custom mode, and starts a new custom mask containing only the
clicked component stem. Once in custom mode, component buttons toggle freely; custom masks that
match Drums + Melody + Bass or Vocals + Drums + Melody + Bass do not automatically reactivate `I`
or `A`. `I` and `A` form one exclusive preset group; `V`, `D`, `M`, and `B` form the component
group. Clicking `I` or `A` from custom mode remembers the current component mask before entering
the preset group. Switching between `I` and `A` preserves that remembered component mask. Clicking
the currently active preset again deactivates the preset group and restores the remembered
component mask in custom mode.
Right-clicking `V`, `D`, `M`, or `B` sets custom mode to only that component and remains active
until the performer changes the component or preset buttons again.

The button cluster is active only when the selected pad is in all-stems mode and has a current
available prepared stem set. If stems are unavailable or the pad is in full-mix mode, the buttons
are greyed out and non-clickable. Rendering derives this from controller/session snapshots only;
it does not inspect cache directories, compute source-version identity, read files, decode audio,
run inference, or call low-level Rust background task APIs.

The per-stem enabled mask is session-only in this slice. Project persistence still stores stem
cache metadata and durable full-mix/all-stems mode only. A later OpenSpec change can make mask
preferences durable if needed.

### Settings Overlay And Stem Quality
The Settings page is opened from a bottom-right gear icon and closed from the same bottom-right
location with an `X` icon. The toggle is part of the center bottom bar so its right edge aligns
with the bank-button row rather than the collapsed sidebars. When open, it replaces the main
Looper display area rather than adding a second floating window. The first controls expose Demucs
stem quality:

- `demucs_shifts`: integer 1 through 20, default 10,
- `demucs_overlap`: float 0.25 through 0.95, default 0.5.

These values are persistent project settings. The Settings renderer only reads current project
state and emits controller actions. It does not inspect cache directories, compute source-version
identity, call the Demucs backend, download models, decode files, or call low-level Rust
background-task APIs. `StemController.generate_stems_async(...)` copies the validated project
settings into the existing `StemGenerationRequest` immediately before the backend task is
scheduled.

### Persistence
Project persistence stores source-version cache metadata and durable stem mix preferences.
Restore revalidates cache metadata against the current source and existing cache files before
marking stems available. Missing cache files, stale source versions, or rejected publication
must not prevent startup or playback.

Project persistence also stores Demucs stem-quality settings. Older project files that lack those
fields load with the default high-quality settings, shifts 10 and overlap 0.5. Invalid persisted
values fail model validation and fall back through the existing safe project-load behavior.

Transient generation progress, last errors, blocked reasons, and momentary performance
gestures, including bottom-bar per-stem enabled masks and remembered component masks, remain
session state and are not persisted.

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
- Whether future momentary solo/mute gestures should reuse the bottom-bar cluster or add a
  separate performer modifier gesture.
