# UI Toolkit: ImGui Development Guidelines

This project’s UI is built with ImGui (an immediate-mode GUI). This document defines the development guidelines for keeping the UI responsive and predictable while keeping the application core testable and independent.

The key idea: **ImGui makes the UI a per-frame “view” of application state.** If we lean into that paradigm, we naturally get a clean Core/UI split, explicit event flow, and deterministic behavior.

## Goals

- Keep the **domain/core** deterministic, testable, and free of UI concerns.
- Keep the **UI layer thin**: layout + input mapping only.
- Treat **rendering as a constant redraw loop** with predictable per-frame cost.

## Core/UI Separation Pattern (required)

ImGui’s “redraw every frame from state” model works best when the UI is a pure adapter around a core state machine.

```text
+-------------------------------+
|           Domain Core         |  ← state, reducers, validation, rules
| (deterministic, testable)     |
+---------------+---------------+
                ▲
                | state snapshot
                |
                | actions/events
                ▼
+-------------------------------+
|              UI               |  ← layout + input mapping (ImGui)
|   (thin adapter, per frame)   |
+-------------------------------+
```

### What goes into the core

The core owns:

- **State**: the full application state needed to make decisions.
- **Actions/Events**: inputs the application can handle (e.g., “pad triggered”, “bank selected”).
- **Reducer / update function**: deterministic logic that applies events to state.
- **Commands/Effects** (optional but recommended): side effects described as data (e.g., “send message to audio engine”), executed outside the reducer.

Core code MUST NOT:

- Call ImGui APIs.
- Block on I/O.
- Depend on frame timing for correctness.

### What goes into the UI

The UI owns:

- **Layout**: windows, panels, widget composition.
- **Input mapping**: translate user input into core actions/events.
- **Ephemeral UI state** only (e.g., temporary text input buffers, which popup is open).

UI code MUST NOT:

- Contain business rules (validation, state machine logic).
- Perform file I/O, decoding, networking, or other long-running work.
- Mutate core state directly in ad-hoc ways (everything goes through explicit events).

### Recommended event flow

Per frame:

1. Take a **snapshot** of core state for this frame.
2. Build the ImGui UI from that snapshot.
3. Collect **UI-produced actions/events**.
4. Apply events to the core reducer.
5. Execute any resulting commands/effects (outside rendering).

A minimal structure (Rust-like pseudocode, not tied to a specific binding):

```rust
// Domain
struct AppState { /* ... */ }

enum Action {
    SelectBank { bank: u8 },
    TriggerPad { pad: u8, velocity: f32 },
    StopPad { pad: u8 },
}

enum Command {
    SendToAudio(/* ... */),
    PersistConfig,
}

fn reduce(state: &mut AppState, action: Action) -> Vec<Command> {
    // Pure, deterministic logic.
    // Returns commands to execute outside the reducer.
    vec![]
}

// UI
fn draw_ui(ui: &imgui::Ui, state: &AppState) -> Vec<Action> {
    // Draw widgets based on state snapshot.
    // Return user intent as Actions.
    vec![]
}
```

## Immediate-mode for our use case

ImGui is immediate-mode: you don’t create a persistent widget tree and mutate it over time. Instead, **every frame you describe the UI** and ImGui computes interactions based on current input + widget IDs.

For this application (real-time controls, fast feedback, lots of “current status” display), this fits well:

- **Frame boundaries are explicit**: you can reason about “what happened this frame” and bound per-frame work.
- **State is explicit**: UI behavior depends on state you pass in, not hidden widget internals.
- **Event handling is explicit**: clicks/changes become actions you can log, test, replay, and validate.

Immediate-mode is not “no state”; it’s “you own the state”. That ownership is what keeps Core/UI clean.

## Thin UI layer (what it means here)

“Thin UI layer” means the UI is an adapter between input/output (widgets) and the core (events/state).

### Rules of thumb

- UI functions should be easy to read as: **layout → emit actions**.
- If a piece of logic is important enough to test, it belongs in the core.
- UI should not “decide”; it should **ask** (emit intent) and the core should decide.

### Handling non-UI work

If a UI interaction needs a slow operation (file access, decoding, heavy computation):

- UI emits an action like `RequestLoadSample { … }`.
- The core schedules work (via a command/effect).
- Completion comes back as a new event like `SampleLoaded { … }`.
- UI simply reflects progress based on core state.

In Flitzis Looper, async sample loading follows this pattern via `AudioEngine.load_sample_async()` and per-frame polling with `AudioEngine.poll_loader_events()`.

This avoids UI stalls and keeps the frame loop predictable.

## Constant redraw (and how to budget it)

ImGui expects a render loop that rebuilds the UI every frame. In this project, **constant redraw is a feature**, not a bug:

- It keeps the UI responsive without relying on complex invalidation logic.
- It makes “live” UI (meters, play state, timing indicators) straightforward.

### Guidelines

- **Assume every frame runs**: do not gate correctness on a “redraw only on change” mindset.
- Keep per-frame work **bounded and predictable**:
  - Avoid per-frame allocations in hot UI paths.
  - Precompute expensive derived values outside `draw_ui`.
  - Cache formatted strings where it matters.
- Use **stable widget IDs** derived from domain identifiers (not fragile indices that may shift when lists change).
- Do not perform blocking operations in the render loop.

### Concurrency + real-time safety

Even with constant redraw, UI must never interfere with real-time audio processing. Keep strict boundaries:

- The UI runs on its own thread (or at least outside any real-time callback).
- Communication with the audio engine (and other subsystems) happens via explicit messages/commands.
- The UI renders from snapshots of state, not from live data structures shared with real-time code.

## Testing implications

ImGui is not designed for heavy black-box UI automation. The architecture above embraces that reality:

- Test the core reducer/state machine thoroughly (high coverage, fast, deterministic).
- Keep UI testing minimal and focused on “wiring” smoke tests.

If a behavior can’t be verified without clicking pixels, it usually means too much logic lives in the UI.

## PR checklist

- UI code only draws and emits actions; no business rules.
- Core logic is independent of ImGui and easily unit-tested.
- No blocking work or heavy computation in the render loop.
- Widget IDs are stable and derived from domain identifiers.
- Actions/events are explicit and flow through a reducer/update function.
