# Pad-owned PCM delivery program

Status: official P0 plan, 2026-10-09; feature implementation and measured acceptance
remain pending. This extends the existing hybrid app and the delivered complete
PCM/resident-loop and prepared-stem foundations. The separate resident ready-trigger
clickfix remains accepted within its engineering scope.

The [OpenSpec proposal](../openspec/changes/adopt-pad-owned-pcm-residency/proposal.md),
[design](../openspec/changes/adopt-pad-owned-pcm-residency/design.md) and
[serial tasks](../openspec/changes/adopt-pad-owned-pcm-residency/tasks.md) define the
target and acceptance. Current-runtime descriptions in other docs remain current;
the new directory/lazy/finite-DSP policy is not yet implemented.

Each occupied pad will physically own its original filename/encoding, complete
FullMix PCM and stems under `samples/#1` through `samples/#216`. Short visible
folders coexist with immutable internal generation names and full lineage.
Migration copies/verifies before atomic references and fresh native source/timing
ACK; old referenced or leased assets and rollback records survive. Content sharing
in RAM can remain safe while disk ownership is per pad. Disk duplication is a cost
to measure, not hide behind permanent global storage.

Five WAV outputs in each pad's `stems` will have five complete aligned f32 disk
derivatives in that pad's `.pcm-cache`, together with FullMix PCM. A joint verified
descriptor binds both immutable generations; two area renames do not constitute
one filesystem transaction or expose a half-complete selected set. ALL STEMS
still renders four coupled components; instrumental stays disk/on-demand offline
data. Direct resident-range reads reuse verified retained descriptors rather than
repeating full WAV conversion/alignment. Dry tap coverage already exists; end-state
finite KEYLOCK continuation still needs actual context proof and bounded work.
The current labelled full-track fallback cannot complete that requirement.

Startup and FULL MIX generation will retain FullMix windows by default. Valid disk
stems alone will not fill RAM. An explicit ALL STEMS action prepares matching
windows even if the same desire was saved. During first lazy activation on a
playing pad, FullMix continues until guarded native residency and effective-mode
feedback permit the existing continuous crossfade. Generation/different-content
replacement stays inactive-only. Ordinary toggles keep valid windows warm within
budget; active readers retire only through actual ownership.

Settings will offer project-persisted startup preload (default off) and an explicit
aggregate resident PCM budget (default512MiB, finite128..16384MiB), with a clear RAM
warning, conservative estimates and exact admitted counts/costs. This new budget
is separate from existing512MiB timing/analysis preparation and1GiB/cold-job limits.
All216 occupied FullMix and eligible demanded four-component slots must actually
be usable at a supported sufficient budget. Partial scheduling is not all216 ready;
the32-voice limit and96 voice-preparation handles are separate concepts.

| Serial slice | Concrete deliverable |
| --- | --- |
| P0 | Official design/deltas/docs and maintained handoff; no product code |
| P1 | Central pad paths, new-write destinations and safe old/new readers |
| P2 | Transactional idempotent migration, fresh native identity and rollback |
| P3 | Complete aligned durable stem PCM and descriptor integrity |
| P4a | Direct range loading and changed-pad-only dry residency |
| P4b | Actual finite DSP/KEYLOCK coverage and continuation |
| P5a | Guarded first active residency and truthful effective-mode feedback |
| P5b | Lazy startup/generation, Settings, resource reservations and all216 admission |
| P6 | Full migration/lifecycle/216/resource/performance engineering acceptance |

Every implementation slice includes affected docs, genuine hardware-free native/
controller/worker/drain/render tests, official strict OpenSpec, independent final
semantic and raw/index/blob/tree review and bounded publication acceptance.
Production is serial. P1's exact scope is paths/new writes/dual readers; no mass
migration or DSP work is bundled into it.

Performance claims need actual paired source/runtime-bound observations: cold,
new-process warm, in-process warm, first live switch, repeated ready starts, one
loop edit and all216 unique/shared content with preload off/on. Record complete
integrity I/O, decode/alignment/cold-job counts, disk bytes, PCM/native ownership,
whole-process working set/commit/peaks, individual repeats and negative outcomes.
PCM bytes are not RSS. Directory organization alone proves no latency/RAM benefit.

Original B2 references/six balanced paired human sessions/music/default/remediation,
B3-B8/K1/Slice7, B6 terminal-owner ordering correction, independent pitch/RubberBand/
nonzero-k/rate/unity/latency and1/2/4/6-pad/30-minute device gates stay OPEN. Final
human visual/hearing/device acceptance and required corrections remain before the
port boundary. Permitted exact-fixture B3 offline B/S preparation can follow this
feature without fabricating B2 closure. STOP BEFORE full Rust application port
planning or implementation/Slice8.
