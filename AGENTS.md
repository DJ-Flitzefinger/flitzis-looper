# Agent Guidelines

## Build/Lint/Test Commands
- Develop: `uv run maturin develop`
- Build Release: `uv run maturin build --release --strip`

### Python
- Lint: `uv run ruff check src`
- Type check: `uv run mypy src`
- Format: `uv run ruff format src`
- Format (only check): `uv run ruff format --check src`
- Test single: `uv run pytest src/tests/foor/test_bar.py -v`
- All tests: `uv run pytest`
- Generate `coverage.md`: `uv run pytest --cov --cov-report markdown`

### Rust
- Check: `uv run cargo check --manifest-path rust/Cargo.toml`
- Lint: `uv run cargo clippy --manifest-path ./rust/Cargo.toml`
- Format (only check): `uv run cargo fmt --manifest-path rust/Cargo.toml --check`
- Format: `uv run cargo fmt --manifest-path rust/Cargo.toml`
- Tests: `uv run cargo test --manifest-path rust/Cargo.toml`

## Dependency Management

### Python
- Install: `uv sync`
- Add
  - `uv add PKG` or
  - `uv add --dev PKG` for dev dependency
- Remove: `uv remove PKG`

### Rust
- Add
  - `uv run cargo add --manifest-path rust/Cargo.toml CRATE`
  - `uv run cargo add --manifest-path rust/Cargo.toml --dev CRATE` for dev dependency
- Remove: `uv run cargo remove --manifest-path rust/Cargo.toml CRATE`

## Code Style

### Python
- Imports
  - Use absolute imports
  - Group in sections (stdlib, third-party, local)
  - Use explicit imports (avoid `import *`)
  - Follow isort configuration in ruff_defaults.toml
  - Imports must only appear at the top of a file.
- Formatting
  - 100-char line limit
  - Use ruff formatter for consistent code style
  - PEP 8 standards
- Types
  - Use type hints consistently
  - Use modern Python 3.14+ type hints
  - Prefer `T | None` over `Union[T, None]`
  - Prefer `list` over `List`
  - Don't add `from __future__ import annotations` (default for modern Python)
- Naming: snake_case for functions/variables, PascalCase for classes
- Error Handling
  - Use Python exceptions
  - Avoid silent failures
  - Catch only those specific exceptions that are expected to be raised in the try block; CRITICAL: Avoid catching broad `Exception`
  - Make use of `with contextlib.suppress()`
- Docstrings
  - Follow Google Python style guide for docstrings
  - Document all public functions, classes, and methods
  - Include parameter types and descriptions
  - Include return value descriptions for non-trivial functions
- CRITICAL: Never silence linter issues like: BLE001, PLR0904, PLR0912, PLR0914, PLR0915, C901
- Keep `__init__.py` free of logic; only re-exports and metadata.
- Tests
  - Tests live under `tests/`, mirroring the package tree.
  - Name tests `test_*.py`; never place tests in `__init__.py`.
  - AVOID: Coverage-driven tests without behavior

### Rust
- Rust FFI
  - Prefer safe wrappers
  - Validate inputs before crossing FFI boundary
- Error Handling
  - Use anyhow and thiserror

## Documentation
Depending on the current task, consider reading specific documentation.

### Project
- User Interface: `docs/ui-toolkit.md`
- Audio Engine: `docs/audio-engine.md`
- Message Passing (between AudioEngine thread and core): `docs/message-passing.md`
- Professional audio/performance architecture audit:
  `docs/audio-performance-architecture-audit.md`

### Python
Fetch documentation directly from the web.
- Dear ImGui Bundle (imgui-bundle): https://pthom.github.io/imgui_bundle/core-libs/imgui/

### Rust
Find documentation for crate `CRATE` under `rust/target/doc/CRATE/index.html`. (You may need to
generate the docs first using `cargo doc --manifest-path rust/Cargo.toml`.)

## Gen3 Audio Architecture Guardrails

For professional Looper architecture work, do not treat "audio safety" as "Rust must not be
touched". The correct rule is:

- The CPAL audio callback and realtime hot path are protected.
- New Rust modules outside the callback are allowed and encouraged when they improve correctness,
  latency, maintainability, or realtime safety.
- The callback must not perform file I/O, JSON reads/writes, Python/GIL access, UI calls, blocking
  locks, logging/printing, neural inference, plugin loading/scanning, unbounded loops, heavy
  allocation, or long-running work.
- Internal Rust DSP modules are the preferred future direction. Do not add VST/LV2/CLAP/AU
  plugin hosting unless a future explicit OpenSpec-backed request changes that product decision.
- Do not implement a new DSP/FX module before consulting
  `docs/audio-performance-architecture-audit.md` and the current handoff context. The realtime
  safety, command/parameter, state-ownership, clock, DSP foundation, and per-pad isolator
  preparation stages have been completed; the next post-isolator target must still be chosen
  explicitly and covered by a focused OpenSpec change before production code changes.
- Keep architecture and DSP work split into small OpenSpec-friendly changes. Do not select a new
  FX module, deck/group/master chain, live loop-edit crossfade, plugin host, or broad rewrite
  automatically.

## CI/CD
- All changes must pass lint, format, and test checks in CI
