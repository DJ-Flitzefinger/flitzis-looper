# Media and PCM

The media path separates durable immutable material, independent content intent,
complete disk artifacts and the PCM actually admitted for rendering. Start with
the [overview](../architecture.md); [runtime](runtime-and-transport.md) owns
sample-frame effects, [state and UI](state-input-and-ui.md) owns application intent.

## Identity and custody

| Identity or owner | Meaning and source |
| --- | --- |
| Slot | Fixed PAD/bank position. A number alone cannot authorize an effect after replacement or rearrangement. |
| Content instance/lifetime | Independent mutable musical content. [content_lifetime.rs](../../rust/crates/looper/src/audio_engine/content_lifetime.rs) reserves runtime lifetimes; [content_placement.rs](../../rust/crates/looper/src/audio_engine/content_placement.rs) maps content to slots and captures placement snapshots. Moving content does not create new source/timing authority. |
| Material | Immutable original and its bound derivatives. [material_paths.rs](../../rust/crates/looper/src/audio_engine/material_paths.rs) resolves typed contained paths; [readable.rs](../../rust/crates/looper/src/audio_engine/material_paths/readable.rs) verifies full-digest binding. A readable folder name or shortened material ID is a locator, not full identity. |
| Source generation | Current Native audio source incarnation, geometry and playback transform. Same path/content bytes do not prove the current generation or acceptance. |
| Request/revision/permit | Captured load, timing, window, pitch, stem or arrangement intent. Each operation rechecks the current source and relevant revisions at publication/effect. |
| Reader/lease | Actual custody held by saved assignments, jobs, queued work, buffers, voices and preparation/history. Logical detachment is not physical release. |

Python [project_materials.py](../../src/flitzis_looper/project_materials.py)
delegates path resolution to Native. Resolution is not ownership. The readable
material binding records full SHA256, byte count and original leaf; material
selection verifies complete candidate originals and retains root/binding handles.
[cold_store/material.rs](../../rust/crates/looper/src/audio_engine/cold_store/material.rs)
serializes full-content claims before derivative preparation, bounds candidate
inventory and preserves unknown/corrupt roots. Exact root birth/immutable identity
is stronger than pathname existence after `mkdir`.

## Import and complete disk artifacts

[loader.py](../../src/flitzis_looper/controller/loader.py) tracks the accepted
application request. [cold_load.rs](../../rust/crates/looper/src/audio_engine/cold_load.rs)
owns request-serialized Native preparation:

1. Capture immutable original custody using
   [ColdTransaction](../../rust/crates/looper/src/audio_engine/cold_store.rs).
   Source hashing and complete manifests bind actual bytes, decoder identity
   and executed playback conversion separately.
2. Reuse a fully verified artifact, or stream complete decoding/conversion through
   [sample_loader/cold/stream.rs](../../rust/crates/looper/src/audio_engine/sample_loader/cold/stream.rs).
   Complete decoder PCM and transformed playback PCM are disk artifacts. Bounded
   packet/FFT/I/O storage avoids a full-track RAM decode as the prerequisite.
3. Finish complete-input analysis and resolve current incoming intent before
   allocating the selected live PCM. [incoming_load.rs](../../rust/crates/looper/src/audio_engine/incoming_load.rs)
   initially requests an automatic eight-bar loop. Failed analysis cannot invent
   a finite loop end; any complete-source demand still needs admission.
4. Reserve/read the actual window, register its owners, publish source work and
   wait for matching callback adoption before reporting current success.

Changed incoming intent reuses the verified artifact for bounded reselection
(at most four attempts); repeated change becomes an explicit failure, not an
unbounded preparation loop or speculative success.

[cold_store/warm.rs](../../rust/crates/looper/src/audio_engine/cold_store/warm.rs)
performs complete integrity verification of fresh warm admissions, including
original/manifest/decoder/playback artifacts. A cache hit avoids decoding and
another snapshot copy; it does not eliminate complete verification I/O. Sealed
handles protect the verified object against pathname replacement during use.

## Complete geometry and finite resident views

[SampleBuffer, CompleteSourceIdentity and ResidentSourceView](../../rust/crates/looper/src/messages.rs)
keep complete source geometry separate from physically held PCM. `frame_count()`
describes the full source; resident start/end describe the current view. Finite
windows copy their PCM without retaining a hidden full backing buffer. Absolute
source taps outside the actual view fail closed; a descriptor is not residency.

[cold_residency.rs](../../rust/crates/looper/src/audio_engine/cold_residency.rs)
selects/attaches source-coordinate views;
[cold_store/residency.rs](../../rust/crates/looper/src/audio_engine/cold_store/residency.rs)
reserves before reading. [native_source_coverage.rs](../../rust/crates/looper/src/audio_engine/native_source_coverage.rs)
proves the whole physical union required by the reader domain:

- Normal needs the loop's physical context.
- Intro needs the prefix through loop end.
- Tail needs loop start through the real source remainder/end and loop return.
- Complete/ALL is a separately admitted exception, not a general assumption.

No guessed DSP halo replaces this proof. Returning from complete to finite
provisioning is another real transaction. Integer physical ranges coexist with
fractional scalar musical trajectories; readers/history/stems share
[source_reader.rs](../../rust/crates/looper/src/audio_engine/source_reader.rs).

[resident_relocation.rs](../../rust/crates/looper/src/audio_engine/resident_relocation.rs)
prepares replacement views through the cold lane. [resident_seek.rs](../../rust/crates/looper/src/audio_engine/resident_seek.rs)
captures actual effective voice state with bounded preallocated handoff. A saved
cursor, address marker or history snapshot cannot grant a new source/timing permit.
Prepared source/history validity and resident window ownership remain separate.

## Streaming, analysis, history and stems

[complete_context.rs](../../rust/crates/looper/src/audio_engine/complete_context.rs)
provides sealed full-source readers for waveform, export and offline analysis.
[analysis_pcm/streamed.rs](../../rust/crates/looper/src/audio_engine/analysis_pcm/streamed.rs)
supports file-backed complete-input preparation. Productive source/window
preparation and stream producers still require their current accepted-timing/
source ownership. An off-thread reader cannot adopt itself into the live voice.

Playback streaming, disk artifact generation and offline model input are distinct
uses of bounded readers. They do not mean the callback can read arbitrary files.
Key Lock source/history preparation uses the same physical source policy and
actual feed chronology. It retains its own prepared reader and history permits,
with no second transport or saved-history authority.

Stem generation holds source/material leases; compatible jobs may share immutable
material and one ready result while retaining individual interests and ACKs.
Cancelling one interest does not erase other users. Saved stem selection, verified
disk eligibility, component residency and actual Native mode/mask are distinct.
Generation during playing/paused states is separate from state-specific adoption
permits. Details and historical limits remain in [prepared stem publication](../prepared-stem-publication.md).

## Copy, rearrangement and memory admission

Copy reuses immutable material while capturing independent target musical intent
and fresh Native ownership. Pair/Bank rearrangement changes placement through
guarded transactions. [pad_copy.rs](../../rust/crates/looper/src/audio_engine/pad_copy.rs),
[pad_pair.rs](../../rust/crates/looper/src/audio_engine/pad_pair.rs) and
[pad_copy/control/bank.rs](../../rust/crates/looper/src/audio_engine/pad_copy/control/bank.rs)
keep source/target, selection, lifetime, publication and all-or-none effect guards.
No source's ACK can certify another content instance merely because PCM is shared.

[asset_lifecycle.py](../../src/flitzis_looper/controller/asset_lifecycle.py)
prepares a complete assignment publication image before durable commit, then
publishes that image and retires old owners under reserved custody. Retry cannot
overwrite a later occupant. [project_assets.rs](../../rust/crates/looper/src/audio_engine/project_assets.rs)
retains saved/job/file owners and weak PCM registrations for real readers.

[cold_jobs.rs](../../rust/crates/looper/src/audio_engine/cold_jobs.rs) has two
workers, 32 queued jobs and a 1 GiB transient PCM ceiling. Live
[resident_accounting.rs](../../rust/crates/looper/src/audio_engine/resident_accounting.rs)
instead accounts unique scoped PCM, held views, pending promises and host headroom.
A configured zero budget means automatic policy, not unlimited allocation.
[resident_proposal.rs](../../rust/crates/looper/src/audio_engine/resident_proposal.rs)
reserves before I/O/allocation/publication. Admission accounting is not a sampled
process-RSS guarantee and does not include every model/OS cache/native cost.

## Publication, cancellation and retirement

Cold publication registers exact file/PCM owners before enqueue, reserves the
original lease and reader registration, and publishes guarded source work.
`LoaderEvent::Success` follows actual adoption plus a current source/generation/
publication/rate recheck. [loader_events.rs](../../rust/crates/looper/src/audio_engine/loader_events.rs)
resolves content placement: pending delivery is retained; stale/retired delivery
cannot update a replacement occupant.

Claimed/incomplete ACK, superseded metadata or uncertain cancellation can retain
possibly active artifacts. They must not roll back an active bank or fabricate
success. Current cold publication/adoption/metadata waits have 30-second guards;
those guards report uncertainty/failure rather than proving no reader exists.
Queue, reservation and retirement exhaustion are real refusals.

Retirement shares the reader-admission asset gate and rechecks immutable hashes/
FileIDs. Releasing one assignment, finishing a worker component or receiving an
ACK does not establish physical last-reader release. Voices, queued publications,
history and another PAD can keep material alive.
[buffer_retirement.rs](../../rust/crates/looper/src/audio_engine/buffer_retirement.rs)
moves large callback owners to a non-audio consumer.
[cold_store/lifecycle.rs](../../rust/crates/looper/src/audio_engine/cold_store/lifecycle.rs)
processes bounded fair cleanup; unknown/changed ownership is preserved.

Existing [material_migration.rs](../../rust/crates/looper/src/audio_engine/material_migration.rs)
and [material_migration.py](../../src/flitzis_looper/controller/material_migration.py)
provide implemented reference/recovery machinery. Durable receipts still need
current inventory and adoption proof. This description authorizes no new migration,
tombstone, compatibility or private-data deletion work.

Recording has its own raw f32le spool, live file witness and codec/finalization
custody in [recording_worker.py](../../src/flitzis_looper/recording_worker.py).
The spool is not an encoded finished recording. Actual worker join and Native
reader release remain necessary before outer retirement.

## Evidence and limits

The existing [PCM report](../pcm-cache-residency.md),
[measurements](../pcm-cache-measurements.md), [readable material](../readable-material-validation.md),
[finite-seek](../finite-seek-domain-validation.md) and
[long-source](../long-source-streaming-validation.md) reports retain exact
source/decoder/resident identity, warm/cold costs, rejected budgets, initial
failures and scoped repairs. Mixed reports remain until every current fact has
a verified module destination; full-report retention is mapped in the
[evidence index](../../openspec/changes/archive/2026-10-30-evidence-index.md).

Allocation admission is not RSS; bounded fixture/integrity passes are not power-loss,
real disk-exhaustion, device or listening acceptance. RF64 parser/source support
does not establish five-RF64 stem inference. Optional historical producer lineage
can remain UNKNOWN. Historical wider programs/statuses are provenance, not new
work authorization or evidence that delivered foundations are absent.
