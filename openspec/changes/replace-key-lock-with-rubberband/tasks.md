## 1. Specification And Research

- [x] 1.1 Bootstrap required local build tools and install `rubberband:x64-windows` through vcpkg.
- [x] 1.2 Create the initial OpenSpec proposal, design, and spec deltas.
- [x] 1.3 Run a minimal Rust build/link probe against the installed Rubber Band C API.
- [x] 1.4 Validate `replace-key-lock-with-rubberband` with official strict OpenSpec validation.
- [x] 1.5 Add cross-platform Windows/Linux dependency and packaging requirements to the plan.

## 2. Rust FFI Boundary

- [x] 2.1 Add a small Rust Rubber Band backend wrapper with manually declared C FFI.
- [x] 2.2 Keep opaque handle ownership and all unsafe calls isolated behind a safe Rust API.
- [x] 2.3 Query and expose channel count, fixed block size, start delay, and backend version or equivalent diagnostic data.
- [x] 2.4 Add wrapper lifecycle tests outside the audio callback.

## 3. Voice-State Integration

- [ ] 3.1 Replace the custom delay-line pitch-compensation active path with Rubber Band processing.
- [ ] 3.2 Preserve Key Lock off varispeed playback.
- [ ] 3.3 Preallocate all Rubber Band staging buffers, channel pointers, FIFO storage, and output buffers before callback rendering.
- [ ] 3.4 Reset or isolate backend state for play, retrigger, seek, stop, unload, and stem-source changes.
- [ ] 3.5 Implement a deterministic bounded fallback for missing shifted output without unbounded callback loops.

## 4. Python And UI Contract

- [ ] 4.1 Remove or replace obsolete custom delay-line Key Lock settings from the performer-facing Settings UI.
- [ ] 4.2 Remove obsolete delay-line settings from active Python/Rust control messages when no longer needed.
- [ ] 4.3 Keep global Key Lock persistence as performer intent without persisting Rubber Band handles, paths, or runtime buffers.
- [ ] 4.4 Update Python type stubs and focused Python tests for the revised API.

## 5. Mixer Behavior Tests

- [ ] 5.1 Cover Key Lock off varispeed vs Key Lock on Rubber Band pitch preservation.
- [ ] 5.2 Cover neutral tempo ratio transparency.
- [ ] 5.3 Cover ratio changes while voices are active.
- [ ] 5.4 Cover loop wrapping, retrigger, stop/unload cleanup, and active Key Lock toggles.
- [ ] 5.5 Cover full-mix and prepared-stem playback through the same Key Lock path.
- [ ] 5.6 Cover Multi Loop with several active Key Lock voices within bounded callback work.

## 6. Documentation And Validation

- [ ] 6.1 Update `docs/architecture.md` and `docs/key-lock-backend.md` after implementation.
- [x] 6.2 Document Windows and Linux native Rubber Band setup in `README.md` and `docs/development.md`.
- [ ] 6.3 Ensure production source has no hardcoded local vcpkg or workstation-specific library paths.
- [ ] 6.4 Verify Windows local development with vcpkg-provided Rubber Band.
- [ ] 6.5 Verify Linux local development with distro/pkg-config-provided Rubber Band.
- [ ] 6.6 Record the later Nuitka installer DLL bundle requirements for Windows packaging.
- [ ] 6.7 Run `uv run maturin develop`.
- [ ] 6.8 Run `uv run cargo check --manifest-path rust/Cargo.toml`.
- [ ] 6.9 Run `uv run cargo test --manifest-path rust/Cargo.toml`.
- [ ] 6.10 Run `uv run pytest`.
- [ ] 6.11 Run `uv run ruff check src`, `uv run mypy src`, `uv run cargo fmt --manifest-path rust/Cargo.toml --check`, and `git diff --check`.
