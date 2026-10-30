# Documentation Map

The [complete source/disposition audit](documentation-audit.md) classifies all 43
original text/JSON documents and records current source owners, contradictions,
planned module boundaries and evidence retention. The list below is the original
navigation retained during cleanup: several files mix current facts with retired
SYNC paths and historical programs. Their old execution queues are inactive;
archival does not establish completion or acceptance.

All 104 formerly active OpenSpec changes now have a
[dated disposition and source mapping](../openspec/changes/archive/2026-10-30-archive-index.md)
and a [canonical historical evidence index](../openspec/changes/archive/2026-10-30-evidence-index.md).
Use current top-level specs for supported contracts; archived tasks retain their
original incomplete gates and do not activate old work programs.

The current [architecture overview](architecture.md) links focused
[runtime/transport](architecture/runtime-and-transport.md),
[media/PCM](architecture/media-and-pcm.md) and
[state/input/UI](architecture/state-input-and-ui.md) references. Mixed specialist
reports below retain dated implementation history and unpassed gates while
analysis/pitch/stem facts are consolidated. Use the new core modules for current
ownership, not old pending-program descriptions.

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
5. Read the relevant current OpenSpec specs; consult archived deltas as historical evidence.
6. Read the relevant tests and source files.

For focused areas:

- Independent Windows installation and testing: `portable-windows.md`.
- UI/rendering: `ui-toolkit.md`
- Stems/Demucs setup: `stem-generation-setup.md`
- Key Lock/time-stretch backend: `key-lock-backend.md`
- Rust module details: `../rust/README.md`

## Maintained Files

| File | Purpose |
| --- | --- |
| `architecture.md` | Current Rust/Python runtime architecture and ownership boundaries. |
| `architecture/runtime-and-transport.md` | Current frame clock, scheduler, scalar rendering, ordinary transport, rates and feedback. |
| `architecture/media-and-pcm.md` | Current material/content/source identity, disk-first PCM, finite views, shared custody and last-reader retirement. |
| `architecture/state-input-and-ui.md` | Current state, GUI/MIDI input, persistence, arrangements, restore and close. |
| `constant-bpm-guide.md` | Current BPM Lock, loop, Tap BPM, device labels and fresh-project workflow. |
| `pcm-cache-residency.md` | Productive sealed originals/full PCM cache, validated reuse and reader lifecycle; separate C2 residency and C3 measurement contracts. |
| `readable-material-validation.md` | Readable full-content roots, compatible no-copy/no-inference reuse, M-ID/legacy migration and bounded software evidence with retained final gates. |
| `pad-content-copy.md` | Complete empty-target musical Copy, durable stem intent, own Native acceptance, independent shared custody and close settlement. |
| `pad-owned-pcm-program.md` | Pending R0 shared-material/pitch/rearrangement/full pre-Rust serial program and genuine acceptance boundaries. |
| `pre-rust-program-coverage.json` | Required seven-group detail rows, original56/nine obligations and HC26; retained38-ID/48-edge dependencies plus20 E11 rows/ten deliveries (48 IDs/78 edges/189 rows). Scoped software status stays separate from required final gates. |
| `development.md` | Setup, validation, OpenSpec workflow, package layout, and generated-file notes. |
| `ui-toolkit.md` | Project-specific Dear ImGui design and state-flow rules. |
| `stem-generation-setup.md` | Pinned Demucs/BS-RoFormer assets, offline worker, CUDA setup and separator rollback. |
| `prepared-stem-publication.md` | Content identity, current accepted timing permits, artifact integrity, callback adoption and shared stem trajectory. |
| `native-constant-timing.md` | G3b2a-i actual-pad timing, native/FIFO history, verified persistence/fresh adoption, guarded global batches and explicit publication/derived refresh; separate loop gates. |
| `key-lock-backend.md` | Current Rubber Band Key Lock backend, timing semantics, realtime constraints, and native dependency requirements. |
| `finite-seek-domain-validation.md` | Bounded production k=0 finite Seek/Intro/Tail, physical-edit and complete-exception return evidence; wider gates retained. |
| `offline-analysis.md` | Diagnostic PCM/worker API, resource ownership and limits; default cutover remains gated. |
| `beat-this-setup.md` | Explicit optional Windows CPU worker installation, locked environment and accepted model provenance. |
| `beat-this-reference-evidence.md` | B1b reference parity/resource observations and the remaining quality/live acceptance limits. |
| `beat-this-acceptance.md` | Frozen B2a private quality/resource gates, independent label requirements and the current acceptance decision. |
| `beat-this-reference-inputs.md` | Private independent annotation and paired human correction inputs, validation and sealing; no musical scoring or default adoption. |
| `beat-this-temporal-scoring.md` | Pure bounded B2 timing metrics, exact matching and uncertainty/region denominators; source certification and full musical/default acceptance remain open. |
| `beat-this-scoring-workflow.md` | Private reference-first CLI, explicit content-verified source aliases and approved complete native candidate lineage; acceptance remains open. |
| `beat-this-corrected-legacy.md` | Separate full-array native QM comparator provenance, fixed reader profiles and engineering versus sealed-reference comparison boundaries. |
| `grid-timing-diagnosis.md` | Reproduced endpoint-derived BPM/grid drift, long-file precision evidence and remaining correction limits. |
| `loop-period-proof.md` | G3c independent musical/physical duration, productive fractional seam policy and rendered-feature proof; device/listening remain gated. |
| `device-loop-acceptance.md` | G3c3 productive human-run preparation, real capture/clock comparison and uninterrupted 30-minute listening packet; actual gates remain open. |
| `scalar-source-coordinates.md` | Shared scalar projection, long-file waveform/seek/playhead precision, BPM edit preservation and remaining timing boundaries. |
| `constant-tempo-summary.md` | Offline count/fit API, complete content-bound backend evidence, isolated PCM refinement, independent-count assertions and the G3 adoption boundary. |
| `selected-backend-bpm.md` | Frozen complete-sequence/representative-region BPM metadata, shared G2 fitting, diagnostic integration and unverified evaluation limits. |
| `accepted-constant-timing.md` | G3a immutable accepted-period revision, explicit acceptance/origin evidence, control-only adoption guard and remaining live/loop gates. |
| `../openspec/changes/archive/2026-10-30-probe-signalsmith-canonical-pitch/design.md` | Isolated nondefault Signalsmith canonical pitch prerequisite; pinned sources, natural latency/ending, fixed criteria and remaining audio gates. |
| `todos.md` | Explicit user-requested TODO notes; not an automatic work queue. |

## Retired SYNC recovery

Source SYNC, REALTIME BPM and source-map editing are removed from main. The
[preserved branch recovery architecture](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/f241da3d0bdaf2f8a73adc4600f2773a3f9d7522/docs/sync/architecture.md)
records the accepted constant BPM/Tap baseline, failed SYNC, later unaccepted
probes and remaining gates. This is a recovery locator, not a feature or execution
queue. Ordinary scalar timing, loops and source/ACK ownership remain current.

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

## Required pre-Rust direct extension

[Direct extension2026-10-11](pre-rust-extension-20261011.md) maps all20 E11 requirements to normative OpenSpec contracts, bounded implementation slices, actual source seams and real acceptance gates. The maintained program/coverage retains old38/48 and169/56/9/HC26 while adding10 IDs (48 total) and30 edges (78 total). Bounded software reports record only their actual evidence and pending checks; whole E11, integration, final Human testing and correction gates remain OPEN.
