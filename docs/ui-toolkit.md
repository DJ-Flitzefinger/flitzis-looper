# UI Toolkit: Dear ImGui Guidelines

Flitzis Looper uses Dear ImGui through `imgui-bundle`. ImGui is
immediate-mode: every frame describes the UI again from current application
state, and ImGui resolves interaction from current input plus stable widget IDs.

This document defines the project-specific UI architecture. It is not a generic
ImGui tutorial. The goal is a responsive performance UI that stays thin,
predictable, and testable while the domain logic, persistence, background work,
and realtime audio engine remain outside rendering code.

## Goals

- Keep the domain core deterministic, testable, and independent from ImGui.
- Keep render functions thin: layout, display, and input-to-action mapping.
- Make every user action explicit through `UiContext`.
- Keep per-frame work bounded and predictable.
- Keep the UI away from the realtime audio callback and live Rust audio state.

## Core/UI Separation Pattern (required)

ImGui works best when the UI is a per-frame adapter around application state and
explicit actions. In this project, the "core" is not one reducer function. It is
the combination of Pydantic state models, Python controllers, persistence,
background workers, and the Rust `AudioEngine` boundary.

```text
+------------------------------------------------------------------+
|                            Domain Core                           |
|                                                                  |
|  ProjectState       durable performer intent, saved to disk       |
|  SessionState       transient runtime/UI projection               |
|  Controllers        validation, orchestration, side effects        |
|  AudioEngine        Rust realtime engine behind bounded APIs       |
+-----------------------------+------------------------------------+
                              ^
                              | read-only selectors and snapshots
                              |
                              | explicit actions / intent
                              v
+-----------------------------+------------------------------------+
|                                  UI                              |
|                                                                  |
|  render/*.py        layout, widgets, visual state, input mapping  |
|  UiContext          read-only state facade + action facade        |
|  ImGui              immediate-mode frame renderer                 |
+------------------------------------------------------------------+
```

Render functions must read through `ctx.state` and emit intent through
`ctx.audio`, `ctx.ui`, or `ctx.input`. They must not reach around `UiContext` to
mutate project state, call low-level audio APIs, or perform slow work directly.

## Current UI Runtime Flow

The UI shell starts in `src/flitzis_looper/ui/run.py`:

1. Create `AppController`.
2. Create `UiContext(controller)`.
3. Configure Hello ImGui/ImmApp runner parameters.
4. Register `render_ui(context)` as the per-frame callback.
5. Register `controller.shut_down` for exit.

`render_ui()` in `src/flitzis_looper/ui/render/render.py` is the canonical frame
entrypoint:

```text
render_ui(ctx)
-> ctx.on_frame_render()
   -> controller.on_frame_render()
      -> controller-owned per-frame callbacks
-> poll keyboard input for Learn/mapped actions
-> ctx.audio.poll.poll()
   -> loader events + audio telemetry messages
-> draw main layout, settings/performance view, bottom bar
-> draw waveform editor and file dialog when applicable
-> ctx.persistence.maybe_flush()
```

This ordering matters:

- controller-owned frame updates happen before drawing,
- keyboard/MIDI mapping intent is captured before widgets are drawn,
- loader and audio telemetry are folded into `SessionState` before visual
  meters/playheads are rendered,
- persistence flushing is outside individual widgets.

## Project File Map

```text
src/flitzis_looper/ui/run.py
    UI runner setup, fonts, Hello ImGui/ImmApp integration.

src/flitzis_looper/ui/context.py
    Public UI boundary. Provides read-only state selectors and grouped action
    facades for audio, UI, waveform, settings, and input mapping.

src/flitzis_looper/ui/render/
    Immediate-mode render functions for the main surface, sidebars, bottom bar,
    settings, waveform editor, file dialog, and control gestures.

src/flitzis_looper/controller/
    Application controllers. Own validation, state mutation, persistence marks,
    background orchestration, audio API calls, and runtime event polling.

src/flitzis_looper/models.py
    Pydantic `ProjectState` and `SessionState`.

src/flitzis_looper_audio/
    Python import boundary for the Rust audio extension.
```

## State Ownership

| Layer | Owns | Must not own |
| --- | --- | --- |
| Render functions | Layout, widget composition, stable IDs, local visual gestures, input-to-action mapping. | Business rules, persistence, file I/O, audio engine calls, background jobs. |
| `UiContext` | Read-only selectors and explicit action facades used by render code. | Hidden behavior that bypasses controllers for core audio/project rules. |
| Controllers | Validation, model mutation, persistence dirty marks, background jobs, audio API calls, runtime event polling. | ImGui layout or widget-specific rendering decisions. |
| `ProjectState` | Durable performer intent: samples, loop regions, BPM/key metadata, dB Gain/Trim and EQ intent, stem cache metadata, settings, mappings. | Live audio truth. |
| `SessionState` | Recoverable runtime/UI projection: active pads, progress, meters, playheads, open overlays, edit buffers. | Durable behavior contracts or realtime audio authority. |
| Rust `AudioEngine` | Live audio truth: loaded buffers, transport, scheduler, voices, stems, playback-rate, Key Lock, DSP, metering telemetry. | UI rendering, JSON persistence, file dialogs, Python object ownership in the callback. |

Some UI-specific state is intentionally stored in `ProjectState` or
`SessionState`, for example selected pad, selected bank, sidebar expansion,
settings visibility, and text-edit buffers. Render functions still should not
mutate those fields directly. Use `ctx.ui.*` actions so persistence marks,
validation, and future behavior changes remain centralized.

## `UiContext` Contract

`UiContext` is the only object render modules should need.

### Read path

Use `ctx.state`:

- `ctx.state.project`: read-only proxy for durable project state.
- `ctx.state.session`: read-only proxy for transient session state.
- `ctx.state.pads`: derived pad selectors such as label, loading state, BPM,
  key, loop region, active/selected state, peak, and playhead projection.
- `ctx.state.stems`: stem availability, mix mode, masks, progress, and errors.
- `ctx.state.banks`: bank selection.
- `ctx.state.global_`: global derived values such as effective BPM.

`ReadOnlyStateProxy` intentionally raises on assignment. If render code needs a
mutation, that is a signal to add or use an action facade.

### Write path

Use explicit action groups:

- `ctx.audio.pads.*`: trigger/stop pads, load/unload, loop editing, analysis,
  BPM/key overrides, pad Gain/Trim in dB, and pad EQ.
- `ctx.audio.stems.*`: generate/delete stems, set stem mix mode, and set stem
  enabled mask.
- `ctx.audio.global_.*`: volume, speed, BPM display, Multi Loop, BPM Lock, Key
  Lock, and trigger quantization.
- `ctx.ui.*`: UI-only actions such as sidebars, file dialog, waveform editor,
  selected pad/bank, global BPM edit state, and pressed-pad projection.
- `ctx.ui.settings.*`: settings overlay actions.
- `ctx.input.*`: Learn mode and keyboard capture.

These actions route through controllers when behavior affects project intent,
audio state, persistence, input mappings, or background work.

## Immediate-Mode Rules

Immediate-mode is not "stateless UI". It means the application owns state and
the UI redraws from that state every frame.

Render code should be readable as:

```text
read state -> draw widget -> if interacted, emit action
```

Rules:

- Assume the draw function runs every frame.
- Keep per-frame work bounded and predictable.
- Derive widget IDs from stable domain identifiers such as pad ID, bank ID,
  stem kind, EQ band, setting key, or action key.
- Do not use shifting display labels as the only ImGui ID when a stable domain
  ID exists.
- Precompute expensive derived values in controllers or helpers.
- Keep validation and clamping in controller/model logic.
- Put meaningful behavior behind testable controller/helper APIs.

## What Belongs In The Core

Core/controller/model code owns:

- state transitions that affect project or audio behavior,
- validation and clamping,
- persistence dirty tracking and flushing,
- sample loading and unloading,
- waveform render-data generation,
- BPM/key/beat-grid analysis,
- loop-region semantics,
- keyboard/MIDI mapping state,
- stem generation and cache validation,
- Rust `AudioEngine` API calls,
- telemetry polling and projection into `SessionState`.

Core code must not call ImGui APIs or depend on widget frame timing for
correctness.

## What Belongs In The UI

UI/render code owns:

- windows, child regions, panels, overlays, and widget layout,
- visual grouping, colors, spacing, and label presentation,
- translating user gestures into `UiContext` actions,
- short-lived visual gesture state when it has no business meaning,
- opening/closing ImGui popups and file-dialog surfaces through `ctx.ui`.

UI code must not:

- perform file I/O,
- scan cache directories,
- decode audio,
- run Demucs,
- call blocking operations,
- own MIDI ports,
- mutate durable project state directly,
- call low-level Rust APIs for work that belongs in a controller.

## Slow Work And Background Tasks

Any interaction that can take noticeable time must be modeled as request,
progress, completion, and error state:

```text
UI gesture
-> UiContext action
-> controller validates request
-> background/audio worker starts
-> controller polls events
-> SessionState records progress/completion/error
-> UI redraws from SessionState
```

Current examples:

- sample loading through `LoaderController`,
- audio analysis through loader/analysis flow,
- offline Demucs stem generation through `StemController`,
- Rust audio telemetry through `AppController.poll_runtime_events()`.

Render functions should never hide slow work behind a button click.

## Audio Boundary

The UI never touches realtime audio state directly. It sends intent to
controllers, and controllers call bounded `AudioEngine` methods.

```text
UI gesture
-> UiContext action
-> controller validation / state mutation
-> PyO3 AudioEngine method
-> bounded Rust command or parameter ring
-> CPAL audio callback
-> telemetry message
-> controller projection into SessionState
-> UI redraw
```

The CPAL callback must remain isolated from:

- ImGui objects,
- Python/GIL access,
- file I/O and JSON persistence,
- plugin loading/scanning,
- Demucs or neural inference,
- blocking locks or waits,
- logging/printing,
- heavy allocation or unbounded work.

For more detail, read [architecture.md](architecture.md).

## Waveform Editor Pattern

The waveform editor is the main UI surface where heavier derived display data is
needed. Its pattern is:

- render code owns layout and visible plot interaction,
- `ctx.ui.waveform` stores editor view state and short-lived cached render data,
- `controller.transport.waveform.get_render_data(...)` supplies waveform render
  data,
- loop edits still go through transport loop actions,
- playback controls in the editor go through playback actions.

Do not move waveform decoding, sample scanning, or source-buffer access into
render functions.

## Input Mapping Pattern

Keyboard and MIDI Learn are action-based. UI code should map gestures to stable
`LooperAction` objects, not directly to ad hoc side effects.

Rules:

- Learnable actions should use existing action factories from
  `flitzis_looper.input_mapping`.
- Text-input focus must be respected before capturing keyboard shortcuts.
- MIDI/keyboard mapping behavior belongs in `InputMappingController`, not in
  render modules.
- `InputMappingController` publishes Learn state to Rust so MIDI Learn capture
  suppresses direct Rust dispatch before any mapped playback command is queued.
- Direct Rust MIDI events with `dispatched=True` must not be executed again in
  Python; direct events with `dispatched=False` use the same controller fallback
  path as non-direct MIDI actions.
- High-rate continuous parameters should be routed through controller/action
  paths that can choose safe Rust parameter-ring updates and smoothing.

## Testing Implications

ImGui is poor at deep black-box pixel testing. The architecture should make most
behavior testable without clicking pixels.

Preferred tests:

- controller tests for validation, state transitions, persistence marks, and
  audio API calls,
- helper tests for derived labels, display values, gesture math, and formatting,
- targeted render-helper tests where a calculation is pure enough to exercise
  without a live UI backend.

If a behavior can only be verified by exact pixel interaction, too much logic is
probably in rendering code.

## Change Checklist

Before changing UI code, check:

- Does the render function only draw and emit explicit actions?
- Is the relevant state read through `ctx.state`?
- Does mutation go through `ctx.audio`, `ctx.ui`, or `ctx.input`?
- Are validation, clamping, persistence marks, and audio calls owned by
  controllers?
- Are widget IDs stable across labels, banks, and pad contents?
- Is per-frame work bounded?
- Is slow work represented by progress/completion/error state?
- Can the behavior be tested without pixel automation?
- Does the change preserve the audio callback boundary described above?
