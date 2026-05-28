# Documentation Map

This folder contains maintained project documentation. It should help a human or
AI contributor understand the current architecture without rereading the entire
codebase.

OpenSpec remains the source of truth for behavior contracts. Documentation
explains architecture, ownership, setup, and development workflow. It should not
duplicate every scenario already expressed in OpenSpec.

## Reading Order

For most code changes:

1. Read `../AGENTS.md` and `../openspec/config.yaml`.
2. Read this file.
3. Read `architecture.md`.
4. Read `development.md`.
5. Read the relevant OpenSpec specs or active change deltas.
6. Read the relevant tests and source files.

For focused areas:

- UI/rendering: `ui-toolkit.md`
- Stems/Demucs setup: `stem-generation-setup.md`
- Key Lock/time-stretch backend: `key-lock-backend.md`
- Rust module details: `../rust/README.md`

## Maintained Files

| File | Purpose |
| --- | --- |
| `architecture.md` | Current Rust/Python runtime architecture and ownership boundaries. |
| `development.md` | Setup, validation, OpenSpec workflow, package layout, and generated-file notes. |
| `ui-toolkit.md` | Project-specific Dear ImGui design and state-flow rules. |
| `stem-generation-setup.md` | External requirements for offline Demucs stem generation. |
| `key-lock-backend.md` | Current Rubber Band Key Lock backend, timing semantics, realtime constraints, and native dependency requirements. |
| `todos.md` | Explicit user-requested TODO notes; not an automatic work queue. |

## What Belongs In OpenSpec Instead

Put user-visible behavior contracts, normative requirements, and acceptance
scenarios in `../openspec/`.

Use docs for explanatory context:

- why Python owns durable project intent,
- why Rust owns live audio truth,
- how the callback stays realtime-safe,
- how packages and generated extension files fit together,
- which setup steps are needed for local development.

## Maintenance Rules

- Keep docs concise and current.
- Remove completed planning prose once the result is reflected in architecture,
  specs, tests, or code.
- Do not keep stale "next step" or backlog text in repository docs.
- Use `todos.md` only when the user explicitly asks to add, review, choose, or
  complete TODO items.
- If behavior changes, update OpenSpec before or alongside implementation.
- If a doc becomes only historical, remove it or move the durable facts into a
  maintained document.
