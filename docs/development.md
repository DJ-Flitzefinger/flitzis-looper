# Development Guide

This guide covers local development, validation, OpenSpec usage, and package
layout for the current project.

## Branch And Repository

Expected development branch:

```text
gen3
```

Before repository edits:

```powershell
git branch --show-current
git status
```

The user handles GitHub pushes manually. Codex must not push or change remotes.

## Setup

Run commands from the repository root:

```powershell
uv sync
uv run maturin develop
```

Start the app:

```powershell
uv run python -m flitzis_looper
```

Use `uv run cargo ...`, not plain `cargo ...`, so the Rust/PyO3 build uses the
project Python environment consistently.

## Validation

Focused changes should run focused tests. Broader Rust/audio, persistence,
OpenSpec, bridge, or UI-control changes should run the full sequence:

```powershell
uv sync
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml
uv run cargo test --manifest-path rust/Cargo.toml
uv run pytest
uv run ruff check src
uv run mypy src
git diff --check
```

Rust formatting:

```powershell
uv run cargo fmt --manifest-path rust/Cargo.toml --check
```

Python formatting:

```powershell
uv run ruff format --check src
uv run ruff format src
```

## OpenSpec

Behavior changes must update OpenSpec before or alongside implementation unless
the existing spec already fully covers a defect correction.

Use active change deltas under:

```text
openspec/changes/<change-id>/specs/<capability>/spec.md
```

Every changed requirement body should start with a direct normative sentence
using `SHALL` or `MUST`, and every requirement needs at least one
`#### Scenario:`.

Official validation:

```powershell
openspec validate <change-id> --strict
```

Fallback:

```powershell
cmd /c npx @fission-ai/openspec@latest validate <change-id> --strict
```

Do not use repository docs as a substitute for OpenSpec requirements.

## Python Packages Under `src/`

There are two Python packages by design:

- `src/flitzis_looper/`: the actual application package. It contains
  controllers, UI rendering, models, persistence, settings, input mapping, stem
  orchestration, constants, and `__main__.py`.
- `src/flitzis_looper_audio/`: the import package for the native Rust extension.
  Its `__init__.py` re-exports the compiled module, `__init__.pyi` describes
  the native API for type checking, and `py.typed` marks the package as typed.

After `uv run maturin develop`, a generated platform extension such as
`flitzis_looper_audio.cp314-win_amd64.pyd` may appear in
`src/flitzis_looper_audio/`. This is a build artifact, not hand-written source.
Do not edit, move, or delete the package directory as a cleanup step.

## Runtime Local Data

The app may create local runtime files:

- `samples/`: project-local copied samples, stem cache, and project config.
- `config/input/`: local input mapping JSON files.
- `.pytest-tmp*`, `.pytest_cache`, `.ruff_cache`, `.mypy_cache`, `.venv`, and
  `rust/target/`: tool/build artifacts.

Do not confuse generated runtime data with source documentation or OpenSpec.

## Documentation Policy

Use `docs/README.md` as the maintained documentation map.

Keep `docs/architecture.md` current for technical architecture and ownership.
Keep product behavior in OpenSpec. Remove or rewrite completed planning prose
when it no longer helps future work.

## TODO List Policy

`docs/todos.md` is the maintained project TODO list for explicit user-requested
notes.

Rules:

- Add items when the user asks to record a TODO.
- Work from it only when the user asks to choose from, review, or complete a
  TODO item.
- Check off or remove completed items when that TODO is implemented or
  intentionally abandoned.
- Do not use TODO items as a substitute for OpenSpec. User-visible behavior
  changes still need specs, tests, and validation.
