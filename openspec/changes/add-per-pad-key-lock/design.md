## Context

Key Lock is currently exposed as a global performer mode and implemented in Rust
as bounded realtime playback-rate and Rubber Band backed pitch-preservation
state. Project persistence stores only durable performer intent, while Rust owns
live audio truth and callback-safe processing.

The new behavior keeps that ownership split. Python stores and edits durable
per-pad intent for loaded pads. Rust stores bounded per-pad live state and reads
it while rendering active voices. The global button remains a master write
operation over currently loaded pads, while unloaded pads stay at disabled
per-pad intent.

## Goals

- Add an obvious selected-pad `KEY LOCK` control without moving or removing the
  existing global `KEY LOCK` control, and show it only for loaded pads.
- Persist one Key Lock boolean per pad with safe defaults for older projects.
- Make global Key Lock overwrite every loaded per-pad value.
- Make per-pad Key Lock toggles affect only the selected pad.
- Ensure simultaneous pads can render with different Key Lock semantics.
- Keep all callback work bounded and free of Python, I/O, blocking, logging, and
  allocation-heavy operations.

## Non-Goals

- Adding a second Key Lock backend or per-pad Rubber Band tuning.
- Persisting backend handles, native paths, latency, buffers, or callback
  internals.
- Changing BPM Lock, Multi Loop, stem cache generation, stem selection masks,
  loop editing, or audio loading behavior.
- Introducing plugin-hosting infrastructure or realtime source separation.

## Proposed Design

### Persistent State

`ProjectState` gains a `pad_key_lock` boolean list with one entry per pad. New
projects and older project files default every entry to `False`. Model
validation keeps the list length fixed to the pad count, rejects invalid types
through the existing safe project-load behavior, and normalizes unloaded pads to
disabled per-pad Key Lock intent.

`ProjectState.key_lock` remains as the global master button state. It represents
the last global Key Lock setting, not a separate override layer. When the global
button changes, controller logic sets `project.key_lock` and sets only currently
loaded `project.pad_key_lock[*]` values to the same boolean. Unloaded pads are
kept disabled and are not allowed to retain stale per-pad Key Lock state.

### Controller And UI Boundary

Controllers own validation, persistence dirty marks, and audio-engine calls.
Render code reads the selected pad's Key Lock state through `UiContext` and
emits an explicit per-pad toggle action. The left selected-pad sidebar renders a
`KEY LOCK` button at the bottom, under the Stem Mix / stem controls, only when
the selected pad has loaded audio, using the same on/off visual language as the
global Key Lock button.

The per-pad action updates only a loaded target pad and leaves
`ProjectState.key_lock` unchanged. Requests against unloaded pads are ignored
after clearing stale per-pad Key Lock state. This preserves the behavior where a
performer can apply a global baseline and then make individual loaded-pad
exceptions without creating settings for empty pads.

### Rust Audio State

Rust keeps bounded per-pad Key Lock state, for example a fixed-size boolean
array indexed by sample/pad id. `ControlMessage::SetKeyLock(bool)` remains
available as a low-level all-pad overwrite, while the Python global UI path
publishes per-pad updates only for loaded pads. A new per-pad control message
and PyO3 method update one pad's value after validating the id.

During rendering, the mixer selects Key Lock processing from the active voice's
pad id. Pads with Key Lock enabled use the existing Rubber Band backed path.
Pads with Key Lock disabled use varispeed playback. Per-pad state changes do
not stop, retrigger, reload, regenerate stems, reanalyze pads, or move loop
positions.

### Realtime Safety

The callback may read bounded scalar per-pad Key Lock state and update it from
bounded control messages. It must not allocate, resize buffers, touch disk, read
or write JSON, call Python, acquire the GIL, log, block, load plugins, run
neural inference, or wait for Rubber Band output.

Rubber Band handles, staging buffers, block metadata, and dependency discovery
remain outside per-sample callback work, as specified by the existing Key Lock
backend requirements.

## Validation Strategy

- Run official strict OpenSpec validation for `add-per-pad-key-lock`.
- Add Python model and persistence tests for default, round-trip, and length
  validation behavior.
- Add controller tests for global overwrite and independent per-pad toggles.
- Add UI context/render-helper tests for reading and invoking selected-pad Key
  Lock behavior.
- Add Rust tests proving global updates set every pad and per-pad updates allow
  different pads to render with different Key Lock states.
- Run focused Python/Rust tests for touched paths, then full validation before
  considering the feature complete.
