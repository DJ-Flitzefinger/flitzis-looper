# Change: Bootstrap Dear PyGui UI shell

## Change ID
add-dearpygui-ui-shell

## Why
We want to integrate the Rust audio engine into the actual Flitzis Looper project and start porting the existing Tkinter UI step-by-step. A minimal Dear PyGui (ImGui) shell provides a stable starting point for incremental UI and feature migration.

## What Changes
- Add a Python UI entrypoint that opens a Dear PyGui application window.
- Configure the window with a fixed size (matching the legacy Tk window size: 960x630) and disable resizing.
- Render a single full-size sub-window/panel that fills the viewport and shows a "hello world" label.
- Add a minimal application logic class (stub) that instantiates the Rust `AudioEngine` via `flitzis_looper_rs`.

## Impact
- Affected specs: `bootstrap-ui` (new)
- Affected code:
  - `src/flitzis_looper/` (new UI + app stub)
  - `pyproject.toml` (add runtime dependency on `dearpygui`)
- No changes to Rust engine behavior; this proposal only wires startup/instantiation.
- `old-project/` remains read-only and is used for reference only.

## Dependencies
- Python: `dearpygui` (runtime dependency)

## Risks
- GUI startup is hard to test headlessly in CI. Mitigation: keep UI wiring minimal and add unit tests only for non-GUI app construction.

## Success Criteria
- Running the UI entrypoint opens a non-resizable 960x630 window.
- The window contains exactly one full-size content panel with a visible "hello world" label.
- UI startup constructs the application stub, and the stub constructs `flitzis_looper_rs.AudioEngine` without starting playback.
