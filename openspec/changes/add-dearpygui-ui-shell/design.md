# Design: Bootstrap Dear PyGui UI shell

## Context
- The legacy application UI is Tkinter-based and tightly coupled to a Python audio engine.
- The new architecture uses Rust for real-time audio and Python for UI/control logic.
- Dear PyGui (ImGui) is the chosen toolkit for the new UI.

## Goals / Non-Goals
- Goals:
  - Provide a runnable Dear PyGui UI shell (fixed-size window).
  - Establish a minimal separation between application logic and UI wiring.
  - Instantiate the Rust `AudioEngine` from the Python app layer (no playback yet).
- Non-Goals:
  - Port any existing Tkinter widgets or workflows.
  - Implement audio controls, sample loading, or message passing.
  - Add automated GUI tests or headless rendering.

## Decisions
- **Window size**: Use 960x630 pixels to match the legacy Tkinter window (`old-project/flitzis_looper/ui/main_window.py`).
- **UI structure**:
  - Create a Dear PyGui viewport with `resizable=False`.
  - Create one primary content window (treated as the "sub-window") sized to the viewport, with minimal decorations (no move/resize/title bar).
  - Place a single label with the text "hello world" in that primary content window.
- **Module layout (Python)**:
  - `src/flitzis_looper/app.py`: `FlitzisLooperApp` stub that instantiates `flitzis_looper_rs.AudioEngine`.
  - `src/flitzis_looper/ui.py` (or `src/flitzis_looper/ui/` package): Dear PyGui context/viewport creation and initial layout.
  - `src/flitzis_looper/__main__.py`: thin entrypoint that constructs the app and starts the UI.
- **Lifecycle**:
  - This change only requires instantiation of `AudioEngine()`; no calls to `run()` are performed.
  - Future changes can add explicit lifecycle management (start/stop/shutdown hooks) once UI events exist.

## Risks / Trade-offs
- Dear PyGui uses a real window and GPU context; CI environments may not support opening a viewport.
  - Mitigation: keep the UI entrypoint manual-smoke-testable; add unit tests only around app construction.

## Migration Plan
1. Land this UI shell and app stub.
2. Introduce a stable app state model and event dispatch mechanism.
3. Port legacy UI components incrementally (toolbar → loop grid → dialogs), keeping UI as a thin adapter over app state.

## Open Questions
- Should the window size become configurable (config file / CLI / environment), or remain constant?
- Should the app stub start the audio engine (`run()`) by default in a future step, or require explicit user action?
