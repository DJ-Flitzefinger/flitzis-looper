# Documentation disposition audit

This is the organization decision for the documentation cleanup, audited against
product source at `dbc80d3c117dcec50996feef2e8c59f1649388a5`. It covers all **43
original text/JSON documents, 1,894,623 bytes**, including the large historical
reports. `assets/hero1.webp` is the additional tracked binary asset; it stays in
place. This audit changes no product behavior and establishes no new acceptance.

The [source manifest](audit/source-disposition.json) records every original Git
blob and SHA256, complete classified line spans, all 479 level-one through
level-three headings outside code fences, source/spec owners, and 324 literal
basename reference candidates. Line numbers always refer to the frozen original blobs,
not to a later relocated or rewritten document. The manifest is a disposition
ledger, not a runtime input or a replacement execution program.

## Authority and publication boundary

Current Human scope defines intent; current source establishes implementation
facts; applicable OpenSpec requirements define contracts. A conflicting old
specification is an identified reconciliation task, not permission to restore a
removed feature. Old FULL/pre-Rust/model-stage programs and their counts remain
historical evidence. Their open gates are not silently passed or reactivated.

The current application is constant-BPM. Source SYNC, REALTIME BPM, selected maps,
map editing/review/adoption and phase-following are removed. Ordinary sample-frame
masterclock, `TransportTimeline`, scheduler, quantization, scalar BPM Lock,
fractional loops/grid, accepted constant timing, pitch/Key Lock, stems and
source/content/OwnACK/last-reader ownership remain. In particular, **accepted
constant timing is not a retired map API**.

STEP2 decided classification and destinations without moving originals. STEP3
has now published and verified the branch recovery document at
`f241da3d0bdaf2f8a73adc4600f2773a3f9d7522` before removing six redundant main
SYNC documents and isolated retired sections. The documentation map's
[recovery locator](README.md#retired-sync-recovery) identifies the consolidated
entry; historical source links below retain the full original reports. The
manifest's appended `executed_dispositions` records actual removals and reference
repairs separately from immutable original provenance. No implementation or
acceptance changed. OpenSpec archival and module/manual rewrites remain later steps.

The remaining sequence is explicit:

1. STEP3: recovery documentation is consolidated and normally published on the
   existing `SYNC-REALTIME-BPM` branch, with clean HEAD/tracking/fresh remote
   equality verified before main removal. Its code was not merged.
2. Archive all actual active OpenSpec changes with their full internal structure
   and actual archive-date prefix. At this audit there are **104**, rather than
   the planning observation of 105. Preserve unchecked tasks, FAIL, UNKNOWN and
   pending acceptance. Do not bulk-merge conflicting historical spec deltas.
3. Write current architecture modules, then the detailed manual and shorter
   introductory/install README. Repair inbound references at each relocation.
4. Verify all original dispositions, retained unique facts, links and source/spec
   consistency. Historical pass counts do not substitute for current acceptance.

## Module boundaries decided before relocation

Except for the published branch SYNC entry, these are planned destinations,
not claims that the files already exist. The
manifest uses their short IDs. Setup paths stay stable because contributor and
installer references already use them; no cosmetic directory move is needed.

| ID | Destination | Responsibility and boundary |
| --- | --- | --- |
| HUB | `docs/architecture.md` | Current system overview, Rust/Python owners, signal/control flow and links. No chronological probe log. |
| RT | `docs/architecture/runtime-and-transport.md` | Output-frame clock, scheduler, scalar periods, quantization, ordinary start/stop, MultiLoop/Lock, atomic effects, DSP/metering, capture and retirement boundaries. |
| PCM | `docs/architecture/media-and-pcm.md` | Immutable original/decoder/playback identities, complete versus resident views, finite/ALL/seek, shared custody, Copy/Pair/Bank transactions, config/recovery and exact last-reader cleanup. |
| BPM | `docs/architecture/bpm-analysis.md` | Productive BeatThis policy, corroboration/fallback/mean, admission, precision, Tap/manual/range and device receipts. Separate live scientific diagnostic APIs from productive authority and historical cohorts. |
| KEY | `docs/architecture/key-analysis.md` | Full-source file-backed KeyNet/CQT, dense scientific API, preprocessing/model identity, independent admission, cancellation/status/OwnACK. Detection is distinct from pitch controls. |
| PITCH | `docs/architecture/pitch-and-key-lock.md` | Scalar rate, Key Lock inheritance, Piano/offset/Retrigger, captured action timing, Native/FIFO/filter history and onset/latency limitations. |
| STEM | `docs/architecture/stems.md` | Model/source tickets, offline generation, immutable artifact commit, selected versus pending/resident/effective state, own adoption ACK, batch interests and deletion. Storage lifetime crosslinks PCM. |
| STATE | `docs/architecture/state-input-and-ui.md` | Project intent versus Session projection versus Native truth, controller/UiContext boundary, GUI/MIDI/Learn, confirmation/arrangement/persistence/recovery and truthful feedback. |
| MAN | `docs/user-guide.md` | Detailed actual performer workflow, controls, settings, recording, recovery, limits and troubleshooting. No private probe commands or obsolete controls. |
| DEV | `docs/development.md` | Contributor setup, checks, package layout and supported diagnostic entry points. Private bindings are not public installation defaults. |
| SETUP | Existing `portable-windows.md`, `beat-this-setup.md`, `stem-generation-setup.md` | Public clone and generic self-contained Portable setup, pinned resources and repair. Preserve existing paths and installer references. |
| MAP | Existing `docs/README.md` and root `README.md` | Reading map and concise product/install introduction; retain hero asset. |
| TODO | Existing `docs/todos.md` | Explicit Human notes only; never an automatic queue. |
| SYNC | `docs/sync/architecture.md` on `SYNC-REALTIME-BPM` only | Recovery architecture, preserved seams, rejected approaches, failures and unpassed acceptance. Main keeps only a concise external branch locator after verified delivery. |

Runtime/PCM/Pitch/Stems describe one shared source trajectory and timing authority,
with crosslinks rather than duplicated explanations. STATE owns how requested,
pending, available and effective states are projected; it does not acquire audio
authority. Recording storage/encoding belongs to PCM, sample-frame capture to RT,
and RECORD gestures/status to STATE/MAN.

## Complete original-file disposition

`L` means a source-supported live explanation or diagnostic contract; `R` retired
feature content; `P` a proposed/unaccepted contract; `H` historical observation,
requirement or execution program. Multiple labels mean the file must be split by
section. **H does not erase a failure or an unpassed gate.** Exact spans and
important nested headings are in the manifest; evidence destinations below are
shared, avoiding repeated archives of the same material.

| Original document | Classes | Current explanation / retained evidence destination |
| --- | --- | --- |
| [README.md](README.md) | L/H | MAP; replace obsolete executable-program prose with current navigation. |
| [accepted-constant-timing.md](accepted-constant-timing.md) | L/H/P | BPM/RT; E-TIMING preserves construction, independent assertion and adoption history. |
| [historical accepted-map-render-comparison.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/accepted-map-render-comparison.md) | R/H/P | SYNC; retain negative renderer/acoustic and unsupported-domain results. |
| [architecture.md](architecture.md) | L/R/H/P | HUB plus RT/PCM/BPM/KEY/PITCH/STEM/STATE; retire hidden mapped-Hold paragraph to SYNC and old queues to E-PROGRAM. |
| [automatic-analysis-memory.md](automatic-analysis-memory.md) | H/L | E-MEMORY; reusable exact-tail/admission lessons to BPM/KEY, not its obsolete allocation formula. |
| [beat-this-acceptance.md](beat-this-acceptance.md) | L/H/P/R | BPM diagnostics; E-B2 retains full frozen protocol, failures and missing independent references/correction sessions. |
| [beat-this-corrected-legacy.md](beat-this-corrected-legacy.md) | L/H/P | BPM diagnostics; E-B2 retains complete-array QM comparator provenance and inconclusive quality. |
| [beat-this-reference-evidence.md](beat-this-reference-evidence.md) | H/L/P | BPM/KEY frontend/lifetime facts; E-B2 retains original resource/parity observations and limits. |
| [beat-this-reference-inputs.md](beat-this-reference-inputs.md) | L/H/P | BPM/DEV diagnostic workflow; E-B2 preserves frozen identity/annotation protocol and missing real inputs. |
| [beat-this-scoring-workflow.md](beat-this-scoring-workflow.md) | L/H/P | BPM/DEV scientific reader/CLI; E-B2 retains approved/rejected producer lineage and absent real scores. |
| [beat-this-setup.md](beat-this-setup.md) | L/H | SETUP remains; BPM describes productive policy; E-CUDA retains dated execution receipts/limits. |
| [beat-this-temporal-scoring.md](beat-this-temporal-scoring.md) | L/H/P | BPM diagnostic metric contract; E-B2 frozen numeric policy and remaining gates. |
| [historical beatmap-sync-design.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/beatmap-sync-design.md) | R/H/P/L | SYNC integrated recovery; extract only independently verified current scalar/pitch/RT invariants. |
| [historical beatmap-sync-research.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/beatmap-sync-research.md) | R/H/P/L | SYNC recovery rationale, pins, rejected options and causal limits; current frontend facts to BPM. |
| [constant-bpm-guide.md](constant-bpm-guide.md) | L/H | MAN/BPM/RT; preserve current mode/target/mean/Tap/device/default behavior; update Key-memory explanation from current implementation. |
| [constant-tempo-summary.md](constant-tempo-summary.md) | L/H/P/R | BPM scientific G2 API; E-G2 preserves conditional precision/count evidence; only actual map plans go to SYNC. |
| [development.md](development.md) | L/H/R/P | DEV plus modules; E-PITCH/E-PCM/E-TIMING retain dated probes; source-beatmap section to SYNC. |
| [device-loop-acceptance.md](device-loop-acceptance.md) | L/H/P | DEV diagnostic workflow and RT limits; E-DEVICE retains complete protocol and still-open master/device/listening gate. |
| [finite-seek-domain-validation.md](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/finite-seek-domain-validation.md) | L/H/P | PCM/PITCH/RT; E-PCM keeps failures, final bounded numerical evidence and open wider acceptance. |
| [grid-timing-diagnosis.md](grid-timing-diagnosis.md) | H/L/P | BPM/RT precision explanation; E-GRID preserves endpoint/long-source/failed alternative evidence. |
| [key-detection.md](key-detection.md) | L/H | KEY; E-KEY keeps historical dataset scores without inventing current evaluation or causal proof. |
| [key-lock-backend.md](../openspec/changes/archive/2026-10-30-add-key-transposition-performance/evidence/key-lock-backend.md) | L/H/P/R | PITCH/STATE/RT; E-PITCH retains test-only experiments, negative onset results and unpassed audible gates. Map-specific history to SYNC. |
| [long-source-streaming-validation.md](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/long-source-streaming-validation.md) | L/H/P | PCM/STEM; E-PCM preserves real long/RF64/codec/fault evidence and its limited domains. |
| [loop-period-proof.md](loop-period-proof.md) | L/H/P | RT/PCM; E-LOOP keeps physical-versus-musical proof, original failures and non-device limits. |
| [native-constant-timing.md](native-constant-timing.md) | L/H/P | RT/BPM/PCM/PITCH/STATE; E-TIMING retains live scalar ownership foundation and original staged evidence. |
| [offline-analysis.md](offline-analysis.md) | L/H/R/P | BPM/KEY/PCM diagnostic boundary; E-B2 retains original protocol/resource results; retired map acquisition to SYNC. |
| [pad-content-copy.md](../openspec/changes/archive/2026-10-30-add-rearrange-shared-pad-content/evidence/pad-content-copy.md) | L/H/P | PCM/STATE/MAN for empty/occupied Copy, Pair, Bank, Hold and import classification; E-COPY preserves failures/unreproduced reports. |
| [pad-owned-pcm-program.md](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pad-owned-pcm-program.md) | H/P/R/L | E-PROGRAM keeps whole historical program; extract corroborated current facts only, and route original SYNC priority/history to SYNC. |
| [pcm-cache-measurements.md](../openspec/changes/archive/2026-10-30-cache-full-pcm-and-resident-loops/evidence/pcm-cache-measurements.md) | H/L/P | E-C3 retains complete C3 measurements; PCM/PITCH keep only accurately scoped current architecture/limits. |
| [pcm-cache-residency.md](../openspec/changes/archive/2026-10-30-cache-full-pcm-and-resident-loops/evidence/pcm-cache-residency.md) | L/H/P | PCM with PITCH/STEM crosslinks; E-PCM retains full old measurements/plans and scoped proofs. |
| [portable-windows.md](portable-windows.md) | L/H/R | SETUP remains; remove stale Source Map preflight subsection after SYNC preservation; retain actual installer failure/ownership boundaries. |
| [pre-rust-extension-20261011.md](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pre-rust-extension-20261011.md) | H/P/L | E-PROGRAM preserves original E11 requirements/gates; corroborated current facts to modules, no execution queue. |
| [pre-rust-program-coverage.json](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/pre-rust-program-coverage.json) | H/P | E-PROGRAM retains lossless schema, counts, hashes, crosswalk, statuses and gate catalogs; repair embedded document references on relocation. |
| [prepared-stem-publication.md](../openspec/changes/archive/2026-10-30-extend-shared-stem-generation/evidence/prepared-stem-publication.md) | L/H/P/R | STEM/PCM/PITCH/STATE; E-STEM preserves phased adoption, queue/failure/retirement evidence; obsolete map-specific context to SYNC. |
| [readable-material-validation.md](../openspec/changes/archive/2026-10-30-adopt-pad-owned-pcm-residency/evidence/readable-material-validation.md) | L/H/P | PCM/STEM; E-PCM preserves material/provenance/physical-terminal evidence and remaining limits. |
| [scalar-source-coordinates.md](scalar-source-coordinates.md) | L/H | RT/PCM/STATE; E-GRID retains original precision evidence and dated estimator observations. |
| [selected-backend-bpm.md](selected-backend-bpm.md) | L/H/P/R | BPM scientific metadata, distinct from `app_tempo`; E-B2/E-G2 preserve unsupported fits and sensitivity; map-era plans to SYNC. |
| [historical source-map-guide.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/source-map-guide.md) | R/H/L | SYNC recovery/user semantics only; verified ordinary BPM/device facts to MAN/BPM. |
| [stem-generation-setup.md](stem-generation-setup.md) | L/H/P | SETUP remains; STEM/PCM explain current paths/adoption; historical eager/pad-root claims to E-STEM/E-PROGRAM. |
| [historical sync-archive.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/9f3ca2d19ad11b0218ccc5ef5dc5353c9981f341/docs/sync-archive.md) | H/R | SYNC preserves exact accepted baseline versus unaccepted probe; main external locator only after branch verification. |
| [historical sync-diagnostics.md](https://github.com/DJ-Flitzefinger/flitzis-looper/blob/27ef7f878f55c0955835ca536af8df6ef4a89a28/docs/sync-diagnostics.md) | R/H | SYNC only: removed timing-log/cohort protocol, privacy and incomplete-capture truth. |
| [todos.md](todos.md) | L/H | TODO remains; preserve explicit optional six-PAD note as a note, with its historical priority context. |
| [ui-toolkit.md](ui-toolkit.md) | L/H/P | STATE/MAN plus focused DEV style guidance; E-COPY/E-STEM/E-PROGRAM retain historical validation/open gates. |

## Unique evidence and exact retention decisions

Archive destinations below are **planned**, not extant hyperlinks. Resolve
`<date>-<change-id>` using the actual archive date and existing folder format in
the archival step. Keep one canonical copy and crosslink other associated
changes. Lossless source is also retained by each manifest blob at the audit
commit: `git show dbc80d3c117dcec50996feef2e8c59f1649388a5:docs/<name>`.
Git provenance supplements visible failures/open gates; it must not hide them.

| ID / canonical associated change | Destination and non-negotiable retained facts |
| --- | --- |
| E-B2 / `adopt-beat-this-analysis` | `evidence-index.md` plus original-source blob/section locators. Retain frozen annotation/scoring/producer contracts and private artifact identities; T04/T05/R01 native-cap and envelope failures, v2 bit-exact remediation, R01 zero-output-tail failure/correction, G#m/Abm validator and forced-cleanup failure. T01–T03 v1 lineage rejected; unused installed PYD is not executing EXE provenance. Keep cold-ACK failures and test-only longer guard separately. Sparse anchors are not full independent labels; real ReferenceSeal, balanced groups/classes, six paired Human sessions and real scores remain absent/unpassed. |
| E-G2 / `derive-robust-constant-tempo` | `evidence-index.md`. Preserve explicit verified-count assertions versus inferred ordinals, isolated bit-identical-attack assumption, original/analyzer/loaded hashes, whole-array/region policy and conditional errors. 600s/1200 attacks/1199 intervals, refined exact120 versus legacy120.001289, zero measured slope versus conditional1.498751-frame bound and distinct1438.800999 raw-lattice sensitivity. Five tracks' unsupported global/region fits remain negative evidence. |
| E-GRID / `preserve-scalar-source-precision` | `evidence-index.md`, crosslink E-G2. Keep endpoint telescoping,120.00128936767578 and extrapolated309.44 frames at600s, manual120 control, long-position query precision, failed middle20–80%120.00314085 and all-position119.99999727 experiment. These are dated observations, not current productive detector output. |
| E-MEMORY / `bound-complete-automatic-analysis` | `evidence-index.md`. Preserve complete decoded16,602,624@44100 versus ffprobe2440-frame difference, nonfinite/overflow/tail tests, bad-fixture failures and historical161.6187/unknown-key/OwnACK result. Its old allocation formula is not the post-Key-fix memory model. |
| E-KEY / existing `2026-06-08-musical-key-cnn` archive | `evidence-index.md`. Retain reported604-track scores406/386/347/201 and527 agreement as historical reported evaluation, with the unproved causal explanation marked as such. Current full-source Key file processing and parity are owned by source and `decouple-bpm-key-admission`; no model-output key becomes musical ground truth. |
| E-TIMING / `adopt-native-constant-timing` | `evidence-index.md`, crosslink `bind-accepted-constant-timing` and `persist-source-verified-constant-timing`. Preserve independent count/origin/acceptance assertion, immutable bit-exact identity, historical ticket versus CURRENT source/own ACK, separate derived-refresh ACK, old voice versus current bank, full batch feedback reservation and4096-frame prepared history. Stage counts and old unmeasured-cost statements stay dated and are reconciled with E-C3. |
| E-LOOP / `correct-fractional-musical-loops` | `evidence-index.md`, crosslink `prove-musical-physical-loop-periods`. Keep P versus H, no H/P rate substitution, final PCM knot to loop zero atP, exact period-bit reader plans; original63/63 fractional failures and26/343-frame offsets, corrected234/144/81 scoped cases and IEEE threshold caveat. Software output is not device/listening acceptance. |
| E-DEVICE / `prepare-productive-loop-acceptance` | `evidence-index.md` plus full original protocol locator. Keep current-source/ACK demand snapshots, independent clock/capture calibration, complete unwrapped75/1000 comparison and actual1800s Human listening requirement. The missing-master-confirmation baseline failure in `remove-source-sync/tasks.md` remains open; tooling existence does not mean packet readiness. |
| E-PITCH / `add-key-transposition-performance` | `evidence/key-lock-backend.md`, crosslink `preserve-full-start-onset`, `fix-late-start-filter-history` and finite-seek evidence. Preserve all test-only AR2/WSOLA/phase-gradient/AMFM/certificate/chirplet/noise experiments as historical, not a productive backend. C3678 minus U2080 requires1598 frames/33.292ms at48k/rate2/H16384; translation cannot restore deformed timing/retention. Keep Short/Medium counterexamples,24 offline R2/R3 strict failures, invalid cold-reset probe scope and principal-attack/click/device/listening limits. No renewed20-PAD target. |
| E-COPY / `add-rearrange-shared-pad-content` | `evidence/pad-content-copy.md`, crosslink `fix-copy-assignment-custody` and `import-declared-stem-audio`. Preserve empty/occupied Copy, genuine Hold, Pair, Bank36 all-or-none claim/config/ACK,133 retirement reservation entries, newer-MultiLoop postACK settlement and declared-stem import custody. Keep unreproduced copied-reader/disappearing-PAD reports explicitly unreproduced, and bounded successes separate from whole216/resource/Human gates. |
| E-PCM / `cache-full-pcm-and-resident-loops` | `evidence/pcm-cache-residency.md`; shared `adopt-pad-owned-pcm-residency/evidence/` holds readable-material, long-source and finite-seek reports. Preserve sealed source/decoder/resident identity, exact root/marker birth receipts and physical last-reader terminal, whole incoming/canonical hash reuse without snapshot/decode, UNKNOWN provenance and37/33-of64 guarded-file scope. Retain initial broad failures and later scoped repairs; >4GiB RF64 source/parser evidence does not prove five-RF64 stem inference. Fault injection does not establish real disk exhaustion/power loss. |
| E-C3 / `cache-full-pcm-and-resident-loops` | `evidence/pcm-cache-measurements.md`. Preserve600s512MiB production refusal versus test-only1GiB, finite/full cold/warm comparisons, complete integrity I/O costs,96-handle startup and32-voice internal renderer observations. Allocation admission is not sampled process RSS; old20/32 performance rows are history, not current stress workloads or device guarantees. |
| E-STEM / `extend-shared-stem-generation` | `evidence/prepared-stem-publication.md`, crosslink `bind-prepared-stem-publication`, `fix-stem-effect-receipts`, `fix-restored-stem-settlement`, `guard-stem-resource-pressure` and E-PCM. Preserve separate immutable disk/config selection and effective Native adoption, same-set versus new-set distinction, paused-Intro former rejection versus later supported software domains, queue-full/refused/stale/deadline/retirement failures and batch Done=saved, not audio ACK. UNKNOWN lineage, power-loss limits and installed-inference/device/Human gaps remain explicit. |
| E-PROGRAM / `adopt-pad-owned-pcm-residency` | `evidence/pad-owned-pcm-program.md`, `evidence/pre-rust-extension-20261011.md`, and lossless `evidence/pre-rust-program-coverage.json`; related changes crosslink these once. Preserve all original requirements, aliases, hashes, pending gates and scoped substatuses. JSON has48 slices/78 edges,169 requirement rows,56 original+9 feature obligations,26 HC cases,10 extension slices/20 E11 rows. Top-level169 rows remain `PLANNED_ONLY_NOT_IMPLEMENTED`;16 E11 rows share that status,03 has partial storage/incoming status and08/09/10 retain final gates open. These are historical planning cuts, not current implementation truth. No box/status promotion. |
| E-CUDA / `enable-cuda-analysis-execution` | `evidence-index.md`, crosslink `prioritize-tempo-periods`. Preserve actual bounded device receipts, CPU retry provenance, old-pointer/readers and stage-only versus activation/Native rollback; availability/import checks are not inference, and process network denial is not an OS sandbox. Pins are inspected provenance, not a signed-training-rights guarantee. |
| SYNC / existing branch | `docs/sync/architecture.md` with exact old blob/section locators. Preserve accepted baseline`949cff1dae4b979639dca1cbc1e86c586387cd21` versus uninstalled/unaccepted probe`27ef7f878f55c0955835ca536af8df6ef4a89a28`; the >100-bar positive listening report is for constant-tempo/Tap, not SYNC. Preserve clock/rate/map/pitch/PCM/stem/preparation/retirement seams; logical/raw/actual input ledgers and original future-input ownership. Preserve Native64/64 failures, OfflineR3 14/16 failures, processed-stem residual. Final72 C-ABI/partition-consistent streams still had0/60 timing passes,40 empty timing and44 empty joint intervals. Keep hop-units cast-to-zero/downbeat defects,57.5ms asymmetric window overlap, source-rate bounds and integral phase-debt warning. No unexecuted Windows ONNX/neural/warp claim. |

Non-SYNC full historical reports move only after their current facts have verified module
destinations. Do not duplicate their chronological narrative inside current
architecture. Historical private paths are provenance strings, never instructions
to scan, move or delete the Human's files.

## Contradictions that later steps must resolve

| Conflict | Source-grounded resolution and owning step |
| --- | --- |
| Old default-disabled BeatThis/QM-primary and future CUDA prose in architecture, development, B2/G2/diagnosis | `app_analysis.rs`/`app_tempo.rs` use configured productive BeatThis with `neural-stable-regions-v4`; separate scientific QM/diagnostic APIs still exist. BPM module must describe both without treating old independent-reference gates as completed. |
| Old shared full-vector Key/concurrent allocation formula; documented KeyDetectionResult/logits API | Current `detect_key_file`, Signal/Scratch and anonymous octave files share the complete CQT kernel with dense APIs; public result is `KeyResult { key_name, confidence }`. Keep512MiB admission distinct from ORT/model/OS cache/RSS and independent BPM outcomes. KEY module updates implementation prose; historical formula/evaluation stays E-MEMORY/E-KEY. |
| Source Map editor/preflight commands in development and Portable | APIs are absent; `scripts/portable/bootstrap.py` rejects `SourceBeatmap` and requires current BPM-only runtime. Preserve recovery in SYNC before removing old instructions. Source-build/Portable remain generic and independent of private developer routing. |
| Dry mapped Hold/Pause paragraph hidden in architecture lines782–791 | Retired original-map trajectory/claim behavior goes to SYNC. Ordinary content-bound Hold/Pause/Resume and their ACK/retirement remain current and must be documented separately. |
| Top-level transport spec still mandates Start/Stop/Heard Drift timing logs | `transport-timeline/spec.md` lines398–429 conflicts with removed timing-log/cohort UI/source and `remove-source-sync/design.md`. STEP4 explicitly reconciles; no false implementation claim or feature restoration. |
| Base stem-cache inactive-only generation/pad-root/unload-delete language | Delivered shared readable material, offline generation during playback and separately guarded active adoption use current `committed_stem_residency`, source/asset leases and extended change deltas. STEP4 reconciles the valid contract; STEP6 explains current domains and last-reader retirement. |
| Eager/slot-bound/pending R0/Copy/Bank/stem statements versus later delivered code | Source-supported current content lifetimes, Copy/Pair/Bank, lazy residency, material sharing and active stem paths replace early plan prose. Do not reopen delivered work because an old coverage status is PLANNED. Preserve the old requirement and actual bounded/unpassed evidence. |
| Same-value Global Key Lock overwrites local overrides | Current nullable inheritance preserves explicit per-pad overrides; returning a pad to GLOBAL clears its local override. PITCH/STATE/manual use source behavior, preserving old prose only historically. |
| Old BPM Lock dependency on MultiLoop, editor selection as START target | Current ordinary transport follows last genuinely played content, source-qualified peer stopping and a stable Lock reference independent of MultiLoop/selection. RT/manual use `fix-ordinary-transport` and current sources. |
| Old native f32 BPM statement in UI toolkit | Current native scalar period/BPM/rate consumers retain binary64; nearest-even physical display rounding does not reduce analytic precision. STATE/RT remove the stale claim. |
| RECORD pending / WAV selected by default | RECORD and codec/recovery paths exist; fresh selected format is FLAC24/compression8, quantization step1/16 initially disabled, separator BS-RoFormer. Saved valid choices remain authoritative. STATE/manual describe actual controls and unresolved device/Human gates. |
| Old unmeasured96-handle/save costs or old full-resident NoHint warning | Later C3/residency evidence and current disk-first ordinary incoming/finite8bar preparation supersede those slice-local statements. Keep measured resource limits and default-budget refusals accurately scoped. |
| Old20-PAD / full pre-Rust/model-stage programs | Productive optimization2/4/6; synthetic stress at most10; internal32 ownership only where affected. Historical data and Human optional TODO remain, without reactivating an old queue. |
| Generic backward-compatibility/migration prescriptions | No new compatibility/tombstone/private-data deletion work is authorized. Existing current recovery/material code is described as implemented; historical prescriptions do not create new implementation duties. |
| Software pass versus readiness/acceptance | No hearing/device/MIDI/full216/quality acceptance is inferred. Device packet missing-master failure, independent-reference gaps and exact experimental failures remain visible in their evidence destinations. |

## Inbound links, schemas and safe relocation

The manifest lists tracked **literal basename candidates** at the baseline,
with line numbers. These are not all resolved incoming links. Documentation,
`rust/README.md`, contributor rules and OpenSpec contain real references, but
upstream license/readme URLs, `workers/metrical_prior/README.md` and historical
references to the root README must not be rewritten as links to `docs/README.md`.
The manifest explicitly excludes those unrelated README matches. No actual
runtime/script reader of these documents was found; the source match in
`analysis/artifacts.py` is an upstream license URL. Protocol identifiers such as
`accepted-constant-timing-v1` are not document paths either. Resolve remaining
candidates before any relocation rewrite.

Important non-Markdown handling:

- The coverage JSON is a historical data artifact, not a generated current status
  report. Keep its full schema/lineage/statuses. Its device-packet `protocol`,
  finite-seek `evidence_document` and other embedded `docs/` references need
  explicit relocation mapping. Record original versus repaired hash if references
  must change; do not regenerate or promote statuses.
- Its consumers include `adopt-pad-owned-pcm-residency/tasks.md`,
  `add-key-transposition-performance/proposal.md`,
  `add-rearrange-shared-pad-content/proposal.md`, and its documentation companions.
- Preserve complete OpenSpec internals, including `.openspec.yaml`, unchecked
  tasks and relative spec links. Moving a change one level deeper alters relative
  paths. Shared evidence uses one canonical destination and explicit crosslinks.
- Keep Portable/model setup paths stable. Inspect installer/source documentation
  references when changing layout; no installer/build/model run is warranted by
  this audit-only change.
- Scan map-following content semantically, not just filenames or the word SYNC:
  development814–894, Portable320–330, architecture782–791, offline-analysis5–36,
  selected-backend94–98/122, program1–49, coverage JSON and nested pitch/stem
  histories are review targets. `uv sync`, asynchronous work, MIDI mapping,
  scalar grid/phase and accepted constant periods are not removed feature paths.

## Audit verification and remaining limits

The audit used bounded complete document reads and independent read-only timing,
analysis and media domains, plus Systems PRE and Methods review. Source owners,
contracts, important section groups, original hashes/headings and inbound
references are checkable from the frozen Git tree. The manifest explicitly labels
future destinations; it does not link to nonexistent archives/modules.

Appropriate checks are full43-file coverage, exact blob/hash/line identity,
complete nonoverlapping classification spans, heading/reference consistency,
existing owner/spec paths, new Markdown links, whitespace and independent final
diff review. No App, device, listening, model inference, Native build or audio
suite was run for this documentation decision. Earlier evidence remains dated
and source-scoped. Later cleanup must verify actual retained facts and references
before deleting an original, and final acceptance remains distinct from archival.

## STEP4 executed archival — 2026-10-30

The [archive disposition index](../openspec/changes/archive/2026-10-30-archive-index.md)
maps all 104 actual active changes to dated complete records. No active change
directories remain. The [canonical evidence index](../openspec/changes/archive/2026-10-30-evidence-index.md)
crosslinks exact immutable original reports and the eleven retention overrides.
Mixed reports remain in place until STEP5/6 verify their current-module destinations.
The coverage JSON remains byte-identical; no checkbox, failure or acceptance gate
was promoted, and conflicting historical deltas were not bulk-merged.

The manifest appends actual relocation/disposition references under
`executed_dispositions`; its original source hashes, spans, headings and inbound
line identities remain frozen provenance. Old change paths in those frozen fields
resolve through the archive mapping rather than being rewritten as current paths.
Current top-level contracts were reconciled for removed timing logs, offline/live
stem generation versus own-ACK adoption and last-reader cleanup, and integer
physical bounds versus fractional scalar trajectories/bounded stem transitions,
plus ordinary single-mode START/STOP and atomic MultiLoop disable semantics.
This documentation step changes no product behavior or acceptance.

## Executed STEP5 core extraction — 2026-10-30

The [current overview](architecture.md) now delegates frame/scheduler/rendering
ownership to [runtime and transport](architecture/runtime-and-transport.md),
identity/disk-first PCM/finite views/copy/last readers to
[media and PCM](architecture/media-and-pcm.md), and controllers/input/persistence/
restore/close to [state, input and UI](architecture/state-input-and-ui.md).
Actual source links and current specs support these references. Queued intent,
prepared state and current own acknowledgement remain distinct.

The manifest appends all eleven original architecture section mappings under
`executed_dispositions/STEP5`. Original file identities, hashes, line ranges,
headings, candidate referencers and retention overrides remain frozen. Sections
1/3/4/5/7/10 still have specialist BPM/Key/pitch/stem consolidation in STEP6;
current summaries link the retained expert documents. Retired section6 was
already handled in STEP3. Historical queues remain accessible through the exact
immutable original and the canonical evidence index, without a new copied log.

No whole mixed report or coverage JSON moved. All eleven full-report retention
overrides and every historical failure/UNKNOWN/pending acceptance remain intact.
The original architecture remains available through its immutable locator; its
historical claims are not current implementation authority. Copy's startup link
now points to the actual state module. Adjacent UI toolkit prose now agrees with
binary64 scalar timing and stable source-qualified BPM Lock. These changes add
no product behavior or acceptance. Full analysis/pitch/stem extraction, manual/
root installation navigation and all-original final consistency remain separate
sequential steps.

## Executed STEP6 specialist extraction and report retention — 2026-10-30

Current [BPM](architecture/bpm-analysis.md), [Key](architecture/key-analysis.md),
[pitch and Key Lock](architecture/pitch-and-key-lock.md) and [stems](architecture/stems.md)
modules are source-grounded productive references. Shared clock/source/content/
OwnACK/reader rules link the core modules. Setup now distinguishes the complete-file
Key frontend, live leased generation, material-root disk sets, residency and own
audio adoption. Key evaluation retains its reported historical scores without an
unproved causal explanation or a new musical-ground-truth claim.

All eleven original architecture spans now have completed dispositions in the
APPENDED STEP6 record. STEP5's partial/deferred entries remain honest prior state:

| Original span ID | Actual current destination / retained history |
| --- | --- |
| 1 | Hub/runtime plus BPM productive and diagnostic boundaries; E-B2/E-TIMING. |
| 2 | Runtime/media clock/loading/source/adoption; E-PCM/E-TIMING. |
| 3 | BPM complete admission/selection and Key complete-file frontend; E-B2/MEMORY/GRID. |
| 4 | Media/runtime and stem artifacts/own adoption; E-PCM/STEM/TIMING. |
| 5 | Pitch scalar rate/Rubber Band and runtime; E-PITCH. |
| 6 | Retired mapped Hold already removed in STEP3; branch-only recovery. |
| 7 | Pitch captured SET/Retrigger, inheritance and state/input; E-PITCH. |
| 8 | Core runtime/media/state DSP/input/tasks/persistence; E-COPY/STEM/PROGRAM. |
| 9 | Media/state content/custody; inactive prior program remains E-PROGRAM. |
| 10 | Media last readers, stem cancellation/batch and state recovery; E-PCM. |
| 11 | Inactive queue retained via immutable original/E-PROGRAM; no execution. |

The [canonical evidence index](../openspec/changes/archive/2026-10-30-evidence-index.md)
now links the EXACT eleven complete reports under their reserved associated
archives. Before moving, 57 original report sections were mapped, with concrete
module anchors for their live facts. Bodies/headings/statuses/failures are retained
with only relative-target repairs; original immutable Git locators remain exact.
Pre-move current and relocated hashes distinguish later reference edits from the
frozen audit originals. Shared evidence is retained once and crosslinked.

Original43 file identities/196 spans/479 headings/324 basename candidates and
eleven retention overrides remain frozen. Prior executed records are unchanged;
only actual mappings/reference repairs are appended. Coverage JSON bytes/schema/
lineage/status/counts/requirements/failures are unchanged, including embedded
original paths as historical lineage; STEP6's from/to map resolves them. No runtime
or tooling consumer requires the former location. No task box, acceptance or old
program is promoted. Current source/spec ownership and diagnostic boundaries are
distinct from historical numeric/software observations and UNKNOWN provenance.

STEP7 retains the extensive performer manual/root product and installation map;
STEP8 retains ALL-original coverage, raw-reference, source/spec/docs and final
consistency. This step establishes no new audio/device/listening/model/full216 or
clean-Windows acceptance and runs no product audio/build/inference for docs.

## Executed STEP7 manual, installation and navigation — 2026-10-30

The [user guide](user-guide.md) explains the current performer workflow from actual
UI/actions/defaults, including selection versus transport target, pending/effective
audio, analysis/correction, loops, pitch/Key Lock, mix, stems, Learn, recording,
persistence/recovery and resources. No device or playback-mode selector is invented;
musical classification and disk/config state are not audio acceptance.

The [root README](../README.md) is now a concise product/public-source/Portable
entry, retaining the hero asset. Stable setup paths and all installer/launcher
code stay unchanged. [The map](README.md) has performer/setup/contributor reading
orders and all seven architecture modules. Contributor instructions retain their
technical guards and original history while explicitly disabling old activation
programs. Development keeps original diagnostic facts/gates, qualifies historical
Nuitka/R0/pre-port/FULL prose and points to current installation/performance docs.

The APPENDED STEP7 manifest record maps original README sections1–2 and all ten
development sections, including the STEP3-retired section9. Original43 identities,
196 spans/479 headings/324 basename candidates/exclusions, eleven overrides and
all prior executed records remain unchanged. The root README and AGENTS pre-edit
versions also have exact immutable locators. No report moves/copies, coverage
regeneration, task-status change, behavior implementation or acceptance promotion.

STEP8 retains the full ALL-original fact/disposition and raw-reference/source/
spec/manual/install/English consistency audit. Current docs and proportional
reference/source checks do not establish fresh audio/device/listening/model,
clean-Windows or whole-216 acceptance.
