# Tasks: Bootstrap Dear PyGui UI shell

## 1. Implementation
- [ ] 1.1 Add `dearpygui` as a runtime dependency.
- [ ] 1.2 Add `FlitzisLooperApp` stub in `src/flitzis_looper/` that instantiates `flitzis_looper_rs.AudioEngine`.
- [ ] 1.3 Add Dear PyGui UI runner that creates a fixed-size (960x630), non-resizable viewport.
- [ ] 1.4 Add a single primary content window that fills the viewport and contains a "hello world" label.
- [ ] 1.5 Add a module entrypoint to launch the UI (e.g., `python -m flitzis_looper`).

## 2. Tests
- [ ] 2.1 Add a unit test that `FlitzisLooperApp()` constructs an `AudioEngine` (no GUI involved).

## 3. Validation
- [ ] 3.1 Run `uv run ruff check src`.
- [ ] 3.2 Run `uv run mypy src`.
- [ ] 3.3 Run `uv run pytest`.
- [ ] 3.4 Manual smoke run: `uv run python -m flitzis_looper` (expect a fixed-size window with a "hello world" label).
