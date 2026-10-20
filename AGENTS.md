## Direkte Humanregel 2026-10-21: Commit und Push bei ALLEN geplanten Goals

Diese Regel gilt für alle bereits geplanten und künftig gemeinsam geplanten
Goals. Der Human verlangt nach jedem erfolgreich implementierten und passend
geprüften zusammenhängenden Feature-/Bugfix-Schritt einen kohärenten Commit und
normalen Push auf das bestehende `main`, OHNE einzelne Commit-, Nachrichten- oder
Push-Absegnung. Gegenteilige Freigaberegeln in `commit.md`, importiertem
`git-commit.md` oder älteren Goal-/Projekttexten sind dafür ausdrücklich aufgehoben.
Abwesenheit des Humans ist kein Grund, veröffentlichungsfähige Arbeit zu sammeln.

Vor dem nächsten unabhängigen Feature-/Bugfix-Schritt: tatsächlichen Diff und
passende Checks/Review abschließen, eigene kohärente Änderungen committen, seriell
nach `main` integrieren und normal pushen; sauberen Arbeitsbaum sowie HEAD,
Tracking und tatsächlich abgefragtes Remote-main auf Gleichheit prüfen. Nicht
mehrere bereits fertige Funktionen über Tage uncommittet halten. Kein leeres
Timestamp-Commit und kein Vermischen fremder/unbekannter Änderungen. Echte
fehlgeschlagene Prüfungen zuerst beheben oder den konkreten Blocker ehrlich nennen;
keine Freigabefrage anstelle bereits erteilter Autorisierung.

Bestehende Worktree-/Integrationsregeln bleiben. Keine Force-/Reset-/Discard-/
Remoteeinrichtungs-/Credential-/GlobalGitconfig-/PR-Rechte entstehen dadurch.
Diese ausdrückliche Commit/Push-Pflicht in jedes geplante Goal und jede Übergabe
übernehmen. Außerhalb geplanter Goals sowie bei späterem ausdrücklichem Widerruf
gelten die dortigen normalen Absprachen weiter.

## Neuester direkter Humanauftrag — vollständige neue Goal-Übergabe 2026-10-21

Der bisherige Chat schließt nur seinen aktuellen kohärenten Lieferblock inklusive
geprüftem Commit, normalem Push und sauberem Arbeitsbaum/Remote-Gleichheit ab und
stoppt danach. Der Human öffnet selbst einen NEUEN LOOPER-Chat und schreibt `weiter`.
Dann MUSS der Nachfolger tatsächlich get_goal/create_goal mit dem vollständigen
../codex-meta/goal-next-activation.txt verwenden, ohne token_budget; ein schon aktives
passendes FULL Goal wiederverwenden, andernfalls EIN NEUES vollständiges Goal
anlegen und das Tool-Ergebnis prüfen. Nicht bloß ohne Goal im Chat weiterarbeiten.
Setup/Öffnen allein wartet; keine weitere Startbestätigung und keine alten Chats
anschreiben/resumieren. Aktuelle vollständige Zusammenführung und erster Auftrag:
../codex-meta/handoff/pre-rust-successor-20261021.md und ../codex-meta/goal-next-plan.md.
Jetzige reale Followups zuerst, danach ALLE alten tatsächlichen Restpflichten;
48IDs/78Kanten/189Zeilen und sämtliche Original-/Erweiterungs-/Human-/Finalgates
bleiben. Architektur/Implementierung/kompatible Belege wiederverwenden, alte OPEN-
Rezepte nicht blind neu bauen; STOP vor Rust-Port/Slice8. Diese neueste Anweisung
ersetzt ältere aktuelle/warte/Start-/Prioritäts-/Chat-ID-Texte, nicht den Vollumfang.

# Agent Guidelines

## Project-Wide Systems And Quality Standard

This direct Human instruction applies to ALL project work, including future
Goals, bug fixes, planning, refactoring, UI, tests, documentation and maintenance.
It remains binding in managed worktrees and after the current pre-Rust Goal.

- Reconcile each task with the actual working tree, including uncommitted work,
  current architecture, latest applicable Human decisions, normative specs and
  actual evidence. Historical plans/OPEN checkboxes do not prove missing code;
  current code does not override the desired product behavior.
- Understand the affected end-to-end path before changing it: UI/MIDI intent,
  persistence, source/material/content identity, preparation, scheduler/time/rate/
  pitch, renderer/DSP, feedback, retirement and recovery as applicable. Identify
  neighboring functions sharing these paths and the relevant concurrency/lifetimes.
- Keep a brief impact note in the existing plan/log for each coherent change:
  intended behavior, authoritative path/reusable components, affected invariants
  and functions, exact remaining gap, appropriate verification and real limits.
  Refresh the note when scope or architectural assumptions change.
- Reuse established components and authority. Separate missing implementation,
  integration, bounded proof and final acceptance. Do not rebuild delivered
  foundations, restore superseded recipes, duplicate state/schedulers/pipelines,
  or weaken guards/tests/acceptance criteria for a local fix.
- Preserve one audio/time/rate authority, independent mutable PAD/voice state,
  coherent source/content/lifetime/ACK ownership, atomic due-frame effects,
  asynchronous publication/last-reader safety, truthful requested/pending/effective
  state, persistence/restore and realtime boundaries. Verify every invariant touched,
  including relevant failure/cancel/stale paths and effects on other functions.
- Resolve actual architecture/contract conflicts explicitly. Latest Human decisions
  and valid later normative revisions define the target; synchronize affected
  official specs/docs/tasks before or alongside behavior changes. Prefer the
  smallest coherent solution meeting the full intent; justified broader changes
  remain allowed within the authorized scope.
- Review the actual final diff and verify combined behavior with meaningful
  impacted regressions. Use integrated checks and independent review at coherent
  boundaries under the existing workflow; reuse only source-compatible evidence.
  Isolated passes/logs/API-profile checks do not establish whole-system, musical,
  device or hearing acceptance.
- Carry this standard and the authoritative original workspace instructions into
  every handoff/worktree assignment before writing starts, even when the target
  checkout has an older AGENTS.md. Preserve it across chat, Goal and branch changes.
- Scale the review to the impact: docs-only work requires consistency/reference/
  diff checks; small fixes do not require repeating all past audits or builds.
  Preserve required final gates, Human data and running App. Continue ordinary
  authorized work autonomously; this rule adds no approval ceremony and does not
  activate a waiting Goal.

## Continued Local Goals And Validation

- In the local workspace, follow the containing workspace's current `AGENTS.md`
  and `codex-meta/handoff/next-step.md` for continuation and Goal activation.
  In a managed worktree, use the original workspace path from the active handoff;
  local memory and private audio stay outside the repository.
- One active implementing chat owns coherent product blocks. Continue in that
  chat while context is useful; no mandatory new chat after each block. Read task-relevant
  context, use bounded independent helpers, and avoid duplicate audit chains.
- During implementation, use quick compile checks and focused meaningful
  regressions. At a product boundary run the impacted integrated checks and one
  independent final review. Run full workspace, both-profile and long musical/
  resource suites at their relevant integration gates or for concrete uncovered
  risk; do not repeat them after every internal edit.
- Reuse successful unchanged-input checks, preserve required final acceptance
  gates, and distinguish reused results from fresh runs. Workflow/docs-only
  edits need consistency/link/whitespace/diff checks, not audio builds/tests.
- On Windows use `scripts/run-rust-tests.ps1` for native runtime dependencies.
  Native installs/builds remain serial; never replace the extension held by the
  Human App. Use `uv run --no-sync` once dependencies are synchronized.

## Build/Lint/Test Commands
- Develop: `uv run maturin develop`
- Build Release: `uv run maturin build --release --strip`

### Python
- Lint: `uv run ruff check src`
- Type check: `uv run mypy src`
- Format: `uv run ruff format src`
- Format (only check): `uv run ruff format --check src`
- Test single (example): `uv run pytest src/tests/flitzis_looper/controller/transport/test_key_lock_transactions.py -v`
- All tests: `uv run pytest`
- Generate `coverage.md`: `uv run pytest --cov --cov-report markdown`

### Rust
- Check: `uv run cargo check --manifest-path rust/Cargo.toml --workspace`
- Lint: `uv run cargo clippy --manifest-path rust/Cargo.toml --workspace`
- Format (only check): `uv run cargo fmt --manifest-path rust/Cargo.toml --all --check`
- Format: `uv run cargo fmt --manifest-path rust/Cargo.toml --all`
- Tests: `uv run cargo test --manifest-path rust/Cargo.toml --workspace`
- Tests (single crate): `uv run cargo test --manifest-path rust/Cargo.toml -p <crate-name>`

## Dependency Management

### Portable Installer Maintenance

- `INSTALL-LOOPER-PORTABLE.bat` installs the current remote `main` from source.
  It must work by double-click on Windows x64 without preinstalled Git/Python/Rust.
- Changes to dependency locks, Python/Rust ABI, MSVC/SDK/vcpkg requirements,
  native DLLs, repository layout, launch/data/model paths or FFmpeg must review
  and adapt `scripts/portable`, the BAT, its tests and `docs/portable-windows.md`
  in the same delivery. Do not leave installer compatibility to a later task.
- Keep downloaded bootstrap tools pinned/verified and native samples/config/input
  protected. Install one complete current-main checkout and one fixed private
  runtime; starts hold the update lock through the actual application process.
  A running current/legacy App stops updates with a close-and-rerun message.
  Journal swaps and Native identity-preserving legacy relocation, verify both
  actual CPU model loads before activation, and clean only proven owned stages.
  Model-only repair on unchanged healthy source/runtime must not rebuild Native.
  Never require a project Release/local developer bundle for ordinary installs.
- Validate missing-tool bootstrap, unchanged-main/update/failure paths relevant
  to the change. A developer-machine test is not clean-Windows Human acceptance.

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
- Clippy
  - Never use blanket `#![allow(...)]` module-level suppressions.
  - Prefer fixing the warning, or `#[cfg(test)]`-gating test-only code.
  - If a suppression is truly necessary, use a targeted `#[allow(...)]` on the
    specific item with a comment explaining why.

## Documentation
Depending on the current task, consider reading specific documentation.

### Project
- Documentation map: `docs/README.md`
- Architecture: `docs/architecture.md`
- Development workflow and package layout: `docs/development.md`
- User Interface: `docs/ui-toolkit.md`
- Stem generation setup: `docs/stem-generation-setup.md`
- Key Lock backend: `docs/key-lock-backend.md`
- Project TODOs: `docs/todos.md`

### TODO list policy
- `docs/todos.md` is the maintained project TODO list for explicit user-requested notes.
- Add items when the user asks to record a TODO.
- Work from it only when the user asks to choose from, review, or complete a TODO item.
- Check off or remove completed items when that TODO is implemented or intentionally abandoned.
- Do not use TODO items as a substitute for OpenSpec. User-visible behavior changes still need
  specs, tests, and validation.

### Python
Fetch documentation directly from the web.
- Dear ImGui Bundle (imgui-bundle): https://pthom.github.io/imgui_bundle/core-libs/imgui/

### Rust
Find documentation for crate `CRATE` under `rust/target/doc/CRATE/index.html`. (You may need to
generate the docs first using `cargo doc --manifest-path rust/Cargo.toml --workspace`.)

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
- Do not implement a new DSP/FX module before consulting `docs/architecture.md`,
  `docs/README.md`, and the relevant OpenSpec specs/changes. The realtime safety,
  command/parameter, state-ownership, clock, DSP foundation, and per-pad isolator preparation
  stages have been completed; the next post-isolator target must still be chosen explicitly and
  covered by a focused OpenSpec change before production code changes.
- Keep architecture and DSP work split into small OpenSpec-friendly changes. Do not select a new
  FX module, deck/group/master chain, live loop-edit crossfade, plugin host, or broad rewrite
  automatically.

## CI/CD
- All changes must pass lint, format, and test checks in CI
