# Documentation Map

Start with the [user guide](user-guide.md) for performer controls and workflows,
or the [root introduction and installation guide](../README.md) for setup.
Current architecture, contributor references and dated evidence have separate
reading paths below.

The [complete source/disposition audit](documentation-audit.md) classifies all 43
original text/JSON documents and preserves original identities, source owners,
section mappings and evidence retention. Its appended executed dispositions record
actual cleanup; its original paths and line numbers remain frozen provenance.

All 104 formerly active OpenSpec changes now have a
[dated disposition and source mapping](../openspec/changes/archive/2026-10-30-archive-index.md)
and a [canonical historical evidence index](../openspec/changes/archive/2026-10-30-evidence-index.md).
Use current top-level specs for supported contracts; archived tasks retain their
original incomplete gates and do not activate old work programs.

The current [architecture overview](architecture.md) links focused
[runtime/transport](architecture/runtime-and-transport.md),
[media/PCM](architecture/media-and-pcm.md) and
[state/input/UI](architecture/state-input-and-ui.md),
[BPM](architecture/bpm-analysis.md), [Key](architecture/key-analysis.md),
[pitch/Key Lock](architecture/pitch-and-key-lock.md) and [stems](architecture/stems.md)
references. Current ownership is separate from the dated reports and inactive
programs retained through the canonical evidence index.

This folder contains maintained project documentation. It should help a human or
AI contributor understand the current architecture without rereading the entire
codebase.

OpenSpec remains the source of truth for behavior contracts. Documentation
explains architecture, ownership, setup, and development workflow. It should not
duplicate every scenario already expressed in OpenSpec.

## Reading Order

For performers and installation:

1. [Introduction and installation](../README.md).
2. [Full user guide](user-guide.md), including defaults, gestures, recording,
   recovery and troubleshooting.
3. [Windows Portable installation/update/repair](portable-windows.md), or
   [public source setup](development.md#setup).
4. [BeatThis worker setup](beat-this-setup.md) and optional
   [stem model/FFmpeg setup](stem-generation-setup.md).

For most code changes:

1. Read [contributor instructions](../AGENTS.md), current task authority and
   [OpenSpec configuration](../openspec/config.yaml). Old activation programs
   retained in instructions are historical and do not start work.
2. Read this map and the [architecture overview](architecture.md).
3. Read [Development](development.md) and the affected architecture modules.
4. Read relevant [current OpenSpec specs](../openspec/specs/), tests and source.
   Consult archived deltas as historical evidence, not an implementation queue.

For focused areas:

- Independent Windows installation and testing: `portable-windows.md`.
- UI/rendering: [UI toolkit](ui-toolkit.md).
- Stems/BS-RoFormer/Demucs setup: [stem setup](stem-generation-setup.md).
- Key Lock/time-stretch backend: [pitch and Key Lock](architecture/pitch-and-key-lock.md)
- Rust module details: [Rust workspace guide](../rust/README.md).

## Maintained Files

| File | Purpose |
| --- | --- |
| `user-guide.md` | Current performer manual: startup, controls/defaults, loops, tempo/pitch, stems, Learn, recording, recovery and limits. |
| `architecture.md` | Current Rust/Python runtime architecture and ownership boundaries. |
| `architecture/runtime-and-transport.md` | Current frame clock, scheduler, scalar rendering, ordinary transport, rates and feedback. |
| `architecture/media-and-pcm.md` | Current material/content/source identity, disk-first PCM, finite views, shared custody and last-reader retirement. |
| `architecture/state-input-and-ui.md` | Current state, GUI/MIDI input, persistence, arrangements, restore and close. |
| `architecture/bpm-analysis.md` | Productive Beat This policy, complete-source admission, scalar precision, corroboration/mean, manual/Tap and diagnostic separation. |
| `architecture/key-analysis.md` | Complete-file CQT/KeyNet, memory/outcomes, current source/correction epochs and classification limits. |
| `architecture/pitch-and-key-lock.md` | Key/Piano/Retrigger, nullable inheritance, exact rate/pitch, prepared Native/history and onset limits. |
| `architecture/stems.md` | Shared offline generation, immutable artifacts, saved/resident/effective state, model execution and last readers. |
| `constant-bpm-guide.md` | Current BPM Lock, loop, Tap BPM, device labels and fresh-project workflow. |
| `development.md` | Setup, validation, OpenSpec workflow, package layout, and generated-file notes. |
| `portable-windows.md` | Generic self-contained Windows source installer, update/repair, isolation, locks and validation limits. |
| `ui-toolkit.md` | Project-specific Dear ImGui design and state-flow rules. |
| `stem-generation-setup.md` | Pinned Demucs/BS-RoFormer assets, offline worker, CUDA setup and separator rollback. |
| `native-constant-timing.md` | G3b2a-i actual-pad timing, native/FIFO history, verified persistence/fresh adoption, guarded global batches and explicit publication/derived refresh; separate loop gates. |
| `key-detection.md` | Model background and historical 604-track evaluation; current productive path is in the Key module. |
| `offline-analysis.md` | Explicit diagnostic PCM/worker API and limits; separate from productive configured Beat This. |
| `beat-this-setup.md` | Required productive BPM worker setup, CPU/CUDA profiles, pinned environment/model and execution-receipt limits. |
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

## Historical reports

The [canonical evidence index](../openspec/changes/archive/2026-10-30-evidence-index.md)
links eleven complete reports now retained with their associated archived changes:
Key Lock, prepared stems, Copy, PCM/residency/measurements, finite Seek, readable
material, long-source validation and the inactive program/extension/coverage JSON.
Use current modules above for architecture. The archived bodies preserve every
original failure, UNKNOWN and unpassed gate; moving a report does not accept it.
Frozen JSON path literals remain original lineage, resolved by the manifest's
appended relocation map; its schema, status and bytes are unchanged.

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

## Authority and evidence conventions

Current Human instructions define the task and intended scope; current source
establishes implementation facts; applicable top-level specifications define
behavior contracts. Resolve contradictions explicitly. An old unchecked task is
not proof that current implementation is absent, and existing code does not
override an explicit product requirement.

Current modules explain supported ownership and behavior. Diagnostic references
describe explicit tools and their limits. Dated reports, archived changes and
immutable Git links preserve original evidence, including failures, UNKNOWN and
unfinished acceptance. Archival, software tests, model classifications and
configuration choices do not establish musical/device/Human acceptance or current
audio adoption. FULL/pre-Rust/model-stage/waiting Goal programs are inactive.
Explicit TODO notes remain notes until requested; they do not authorize execution.

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

## Historical pre-Rust extension and open evidence

[Direct extension2026-10-11](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pre-rust-extension-20261011.md)
retains all20 original E11 requirements, historical implementation slices/source
seams and acceptance gates. The archived program/coverage preserves its original
counts and lineage, not an active execution queue. Bounded software reports retain
only their actual evidence; unpassed whole E11/integration/Human testing/correction
gates remain open as recorded. Current modules explain implemented behavior.
