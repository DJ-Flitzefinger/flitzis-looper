<!-- OPENSPEC:START -->
# OpenSpec Instructions

These instructions are for AI assistants working in this project.

Always open `@/openspec/AGENTS.md` when the request:
- Mentions planning or proposals (words like proposal, spec, change, plan)
- Introduces new capabilities, breaking changes, architecture shifts, or big performance/security work
- Sounds ambiguous and you need the authoritative spec before coding

Use `@/openspec/AGENTS.md` to learn:
- How to create and apply change proposals
- Spec format and conventions
- Project structure and guidelines

Keep this managed block so 'openspec update' can refresh the instructions.

<!-- OPENSPEC:END -->

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

### Rust
- Check: `cargo check --manifest-path rust/Cargo.toml`
- Lint: `cargo clippy --manifest-path ./rust/Cargo.toml`
- Format (only check): `cargo fmt --manifest-path rust/Cargo.toml --check`
- Format: `cargo fmt --manifest-path rust/Cargo.toml`
- Tests: `cargo test --manifest-path rust/Cargo.toml`

## Dependency Management

### Python
- Install: `uv sync`
- Add
  - `uv add PKG` or
  - `uv add --dev PKG` for dev dependency
- Remove: `uv remove PKG`

### Rust
- Add
  - `cargo add --manifest-path rust/Cargo.toml CRATE`
  - `cargo add --manifest-path rust/Cargo.toml --dev CRATE` for dev dependency
- Remove: `cargo remove --manifest-path rust/Cargo.toml CRATE`

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
  - Catch only those specific exceptions that are expected to be raised in the try block; Criticial: Avoid catching broad `Exception`
  - Make use of `with contextlib.suppress()`
- Docstrings
  - Follow Google Python style guide for docstrings
  - Document all public functions, classes, and methods
  - Include parameter types and descriptions
  - Include return value descriptions for non-trivial functions

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

### Python
Fetch documentation directly from the web.
- Dear PyGui: https://raw.githubusercontent.com/hoffstadt/DearPyGui/master/docs/source/index.rst

### Rust
Find documentation for crate `CRATE` under `rust/target/doc/CRATE/index.html`. (You may need to
generate the docs first using `cargo doc --manifest-path rust/Cargo.toml`.)

## CI/CD
- All changes must pass lint, format, and test checks in CI
