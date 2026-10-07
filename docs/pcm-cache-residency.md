# Complete PCM cache and finite loop residency

Status: C0 audited design, 2026-10-07, baseline `1be58def`.
**No cache/residency implementation or performance acceptance is delivered by C0.**
The active [OpenSpec change](../openspec/changes/cache-full-pcm-and-resident-loops/proposal.md)
defines the proposed contracts; its unchecked tasks are implementation work.
This document is the maintained engineering reference for C1-C3, not an account
of features already available.

## Audited current behavior

Line references describe the C0 baseline. In the table, native module names
(`mod.rs`, `sample_loader.rs`, `source_reader.rs`, `constant_timing/*`, etc.)
are relative to `rust/crates/looper/src/audio_engine/`; `messages.rs` is in
`rust/crates/looper/src/`. Python controller/UI paths are relative to
`src/flitzis_looper/`; Python `test_*.py` paths are relative to
`src/tests/flitzis_looper/`. OpenSpec paths are relative to `openspec/`.

| Area | Actual contract and evidence | Consequence |
| --- | --- | --- |
| Import/original | `mod.rs:950-1033` hashes path, decodes path, then copies and hashes project file. `sample_loader.rs:508-532,651` copies original bytes, preserving basename/collision suffix. `controller/test_loader.py:689,728` distinguishes restore from import. | Persistent changes are detected, but A-to-B-to-A replacement during decode is not excluded. Copy-first lineage is still missing. No WAV transcode replaces the original. |
| Load work | `mod.rs:950,1066-1098` spawns one thread per pad and guards publication under current request. `controller/loader.py:779-789` filters stale events. | No global load worker limit; cancellation prevents publication but does not stop copy/decode. A stale job can leave a copied orphan. |
| Rates/decoder | `sample_loader.rs:256-420` uses decoded rate/channels, rejects mid-stream changes, converts to device rate/layout. Default format/decoder options carry no explicit retained gapless/delay/padding policy. Malformed packet silence preserves known duration. | Decoder rate, playback rate and device configuration must be explicit; compressed boundary policy needs evidence, not assumed gapless behavior. |
| Playback conversion | `sample_loader.rs:118-231` uses Rubato FFT 1024/one subchunk/FixedSync::Input. Delay trim occurs in the main full-chunk loop; partial/tail calls lack the stronger finite budget and explicit final trim. | Current short/tail origin and phase are unproved. Do not call playback conversion equivalent to the repaired analyzer path. |
| Analysis conversion | `analysis_pcm.rs:274-390` and `analysis_pcm/fft.rs` use integer ceiling, required delay plus length, cancellation and derived tail-call budget. | Reuse these dimension/budget rules for a bounded multichannel playback correction; independently prove samples, not only dimensions. |
| Full shape | `messages.rs:21-24`, `mod.rs:713-738,1063-1064` equate sample allocation with full frames/duration. `source_reader.rs:425,449,533` directly indexes/asserts full extent. | Cropping an Arc without an explicit full descriptor and address translation is invalid. |
| Editor/analysis | `mod.rs:2704-2746` waveform reads full sample; `analysis_pcm.rs:65-91,129-156` snapshots/mono mean use full frames. `controller/transport/waveform.py:36-48` clamps at full duration. | Full navigation, sample zoom and complete analysis need independent full-source leases. |
| View/coordinates | `ui/context.py:711-729,732-820,852-883`; `ui/test_waveform_precision.py:18,46,70`; `ui/test_context.py:1013,1099,1192`. Scalar source seconds are binary64, markers loaded-frame indices, grid origin signed. | Window movement is storage addressing, never a coordinate rebase; view-only jumps cannot seek audio. Preserve view on same-source timing/window changes. |
| Explicit seek | `source_reader.rs:152-237`; `mixer.rs:4118-4227`; `controller/transport/test_playback.py:170-223`. Before-loop intro reaches loop; after-loop tail reaches full source end then wraps. Paused stays paused; stopped seek is a no-op. | A tiny seek-target window is insufficient. Prepare a finite full-track exception before effective nonresident seek. |
| ALL/live edits | `controller/transport/loop.py:112-143`; `controller/transport/test_loop.py:696,1379`; `specs/loop-region/spec.md` promises immediate edits/ALL. ALL is explicit manual 0..full duration. | Nonresident preparation requires a precise modified immediate/readiness contract and requested versus effective state. |
| Accepted authority | `constant_timing.rs:100,127-128,261-268`, `input_runtime_binding.rs:93-120,373-374`, `prepared_source.rs:278-342` bind complete PCM address/extent plus request/generation/digest/rate/accepted projection. | Separate stable complete-source authority from window ownership, without inventing accepted evidence or keeping hidden full-track RAM pins. |
| Fractional reader | `source_reader.rs:98-111,624-681`; `musical_loop_proof_tests.rs:574-709`. Two addressed taps share ramp progress; compatible musical P can differ from physical H by at most one frame. | Preserve actual seam, integer endpoints and <=1-loaded-frame unwrapped long-cycle gate; physical duration is not the musical period. |
| Native history | `prepared_native_history.rs:20-75,126-179`; `key_lock_preparation.rs:323-380`; `prepared_native_mixer_tests.rs:124,203,376,478`. Exact copied SourcePlayback/ReadPlan/target frame/permit and actual native/FIFO state govern adoption. | 4096 processed output frames are a preparation horizon, not a generic source margin or acoustic delay. |
| Stems | `source_reader.rs:363-415,523-557`; `controller/stems.py:658-701,770-805`; `controller/test_stems.py:328,383,668,709`. Full shape/current source/timing and complete artifact set govern availability. | Same-source stems need matching absolute ranges/window revisions. Full duration/shape and resident counts cannot share one ambiguous API. |
| Stem restrictions | `specs/stem-cache/spec.md:34-40,77-85` requires inactive generation/replacement and eager pad-directory deletion; `mixer.rs:532-545` rejects active set replacement. | Add a narrow same-source/same-StemSet window-relocation exception; new generated/complete sets stay inactive-only. Generation-specific cleanup cannot remove newly owned pad files. |
| Pinned replacement | `mixer.rs:479-522`; `productive_history_tests.rs:298,483`; active `changes/adopt-native-constant-timing/specs/audio-analysis/spec.md:459-489` keeps old pinned voice source/timing. App `controller/loader.py:143-144` explicitly unloads first. | Reconcile the old load spec's unconditional stop scenario with native pinned behavior; retain the app's unload-before-replacement stop. |
| Cleanup | `controller/loader.py:198-219,736-744` deletes project path eagerly with lexical checks and no shared-assignment accounting. `buffer_retirement.rs:11-12,76-126` has bounded worker retirement and leak fallback. `audio_stream.rs:781-839` reserves capacity. | Last-user ownership and resolved/link-aware containment are missing; lexical `samples/../` is unsafe. Leak fallback protects RT but does not prove eventual cleanup. |
| Save integrity | `controller/timing_persistence.py:28-53`; `constant_timing/persistence.rs:364-415,658-689` rehashes original twice and reconstructs full mono/44.1-kHz evidence. Tests `controller/test_accepted_persistence.py:95,140,475,523` preserve atomic failure, throttle, fresh ACK and drain. | Save I/O/CPU is unmeasured. Cache reuse cannot turn a historical record into CURRENT or bypass integrity. |

The current native pool has three states per 32 voices, or 96 handles
(`key_lock_preparation.rs:471-503`). Setup/RAM costs for this pool remain
**unmeasured**; older 64-handle observations are not current evidence.

## Data and time domains

Use small cohesive additions to the loader/source descriptors, not a new general
streaming or job framework. Existing control/worker ownership remains authoritative.

| Object/domain | Retained data | Lifetime |
| --- | --- | --- |
| Project original | Byte-exact imported file with non-colliding original name, digest and managed ownership. Persisted assignment points here. | Durable until final owned assignment/readers retire; external source is never deleted. |
| Immutable source snapshot | Actual stable copied bytes, byte count/SHA-256, retained stable read handle, decoder/container metadata. | Held across decode/cache verification/jobs; never a later reopening of the external path. |
| Complete decoder PCM | Full interleaved float32 LE at actual decoded rate/channels, actual digest, exact full extent and decoder-origin policy. | Immutable regenerable disk artifact; no mandatory full RAM pin. |
| Complete playback PCM | Full device-format derivative, actual interleaved digest, mono evidence digest, exact rate/layout/frame count and executed decoder-to-playback transform. | Immutable disk artifact; C1 initially retains existing full-buffer playback. |
| Analyzer input/evidence | Full playback-domain arithmetic-channel-mean-f64-v1 -> f32 mono digest, separately identified 44.1-kHz QM/key and Beat This frontend inputs. | Full-source worker leases/evidence, outside RT; no use of window as full input. |
| Resident view | Complete-source descriptor reference, absolute range(s), storage offsets, resident frame/byte count, window/region revision, proved context policy. | Pad/bank/voice/job handle, retired off-thread. |
| Effective voice | Pinned source descriptor/window plus SourcePlayback, read plan, physical H/musical P, rate history, stem/filter/native/FIFO state. | Continues under its own identity until a matching finite transition or existing stop/unload. |

Decoder/source time begins at the explicitly recorded decoder-origin policy;
playback time retains original source zero through its declared conversion.
Source grid origin is a separate signed timing fact, never the resident start.
For playback rate Fp, an absolute loaded frame n denotes n/Fp source seconds;
resident translation subtracts the storage range start only for memory indexing.
Full duration uses complete playback frames/Fp, never resident count/Fp.
Device/output-frame time, rate ratio, analyzer rate and musical beat units remain
distinct. There is no forced 48-kHz canonicalization.

Preserve the existing output-format playback contract rather than introduce a
new callback resampling owner. A device/layout change selects another versioned
full derivative. If its PCM/rate/extent/provenance differs from saved accepted
evidence, retain that record as historical and reject fresh adoption until
matching evidence is verified; never relabel its digests or assume acceptance
survives. Manual/Tap/Legacy intent and source-relative labels remain independent.

## Stable capture and full artifact commit

1. Admit a job within fixed queue/worker and PCM-byte budgets before allocating.
   Capture pad request/source intent/device configuration. Use bounded chunks,
   cancellation points and checked byte/frame arithmetic throughout.
2. Obtain a source read protocol that excludes mutation for the copy interval
   (on Windows, a retained handle with tested write/delete-sharing exclusion).
   A mutable source that cannot be captured stably fails safely. Before/after
   stat or path hashes do not prove stability. Hash the bytes successfully
   written while copying; handle short reads/writes, errors and cancellation.
3. Flush the owned staging snapshot, verify its digest/extent and retain an
   immutable reader that excludes mutation for all dependent reads. Decode that
   same snapshot/handle. A read-only filename flag alone does not enforce this.
   Import commits the byte-exact original at a non-colliding name. Restore may
   take a transient sealed snapshot but creates no second durable original.
4. Decode complete PCM once, retaining actual selected codec/options/library/
   processing versions, stable decoded rate/channels and exact frame count.
   Version packet-error-silence and codec delay/padding/gapless policy. No
   speculative removal of leading silence, decoder delay or tail padding.
5. Hash complete float32 LE interleaved decoder PCM and complete playback PCM as
   they are produced; also retain the complete playback mono evidence digest.
   Reject invalid dimensions/nonfinite sample policy violations explicitly.
   Record rate/layout conversion and measured/derived delay, phase and tail policy.
6. Write artifacts plus manifest to an exclusively owned staging directory,
   flush files and commit the complete immutable directory/manifest atomically
   on the same filesystem. A committed manifest binds every file/digest/length.
   Readers ignore staging/partial entries. Crash durability/recovery and Windows
   rename behavior need fault-injection evidence, not just a successful rename.
7. Under existing pad request/ownership serialization, check source/request/
   device/intent again and reserve native command/retirement/feedback capacity.
   Publish complete matching source/metadata/handle state all-or-none. Loader
   success is not a substitute for effective callback adoption of later windows.

Proposed owned PCM location: `samples/.pcm-cache/v1/<identity>/`; staging is
inside that root. Project config/originals/stem assets remain distinguishable.
No private test audio or scratch evidence is committed. All managed path checks
use resolved roots and link/reparse-aware ownership; never infer deletion rights
from a string beginning with `samples/`.

Full identity is SHA-256 of a deterministic versioned descriptor: actual original
digest/bytes; decoder ID/options/version; processing policy/version; PCM format/
layout/rate/channels/full frames; actual full PCM digest; source-zero policy and
canonical transform lineage. Store descriptors alongside artifacts. No digest
includes itself. A derivative includes its parent full identity and own full
digest/dimensions/transform. Mono is separately versioned and never replaces the
interleaved digest. Pad/runtime/window revisions are not disk content identities.

A pre-decode lookup index may map original digest + decoder/transform selector
to a candidate full identity. It is only a locator: the committed manifest and
full validation decide reuse. Different paths with identical bytes can share PCM
but retain distinct project-original assignments. Conflicting candidate content,
partial entries, old versions, format/device differences and corrupt manifests
regenerate off-thread; they never overwrite an entry held by a reader.

## Integrity policy and bounded workers

Every fresh source/cache lease hashes the complete project original and complete
required PCM files, checks manifest identity, dimensions, checked expected byte
length and exact file EOF. The lease protects exactly the verified immutable
objects from writes/replacement. Existing verified in-process leases can be
shared across consumers; reopening a mutable path requires verification again.
This deliberately incurs O(full source + required PCM bytes) warm I/O. An index
or size/mtime shortcut cannot weaken it. C3 must report that cost.

Use one bounded preparation lane with explicit maximum active workers, queued
requests and transient PCM bytes, separate from existing realtime/native lanes.
Choose numeric defaults in C1 against measured resource needs; queue admission
failure is caller-visible and leaves effective ownership intact. Coalesce
same-digest preparation without multiplying full temporary PCM, while retaining
per-pad request guards/cancellation. Cancellation of one subscriber cannot cancel
other owners. Shutdown cancels/drains workers and retires readers off-thread.
Do not replace existing offline-job or native-history schedulers wholesale.

Save keeps its current content verification and atomic previous-file/dirty-state
failure behavior. A future reuse of the exact verified immutable source/evidence
lease must prove equivalence; C1 cannot silently skip existing checks to improve
a benchmark. Current save rehash and CPU, plus warm verification, remain separate
measurements.

## Resident geometry and processing context

C2 must first split complete-source metadata/evidence ownership from resident
storage. Current `samples.len()`, pointer equality, `loaded_sample_shape`,
waveform/export and CURRENT/MIDI/prepared bindings need a coherent explicit
descriptor. Merely storing a full descriptor beside a window while hidden
accepted tickets still pin full PCM does not deliver residency.

Each resident view declares absolute readable ranges, channels/rate, backing
storage offsets, full source identity and monotonically checked window revision.
Each requested region/context change has its own revision. Source replacement
changes complete source generation; same-source window replacement does not.
Full-source descriptor/evidence leases authenticate complete immutable disk PCM.
Fresh source/window permits and callback ACK must rebind all consumers together
without changing the accepted revision or admitting historical-only authority.

For dry interpolation, enumerate the actual addressed left/right taps under
SourceReadPlan and copied SourcePlayback. Preserve physical [a,e), H=e-a,
compatible accepted musical P and last knot min(ceil(P)-1,H-1); the seam joins
last knot to first at P and must never access exclusive e. Rate smoothing,
explicit seek mode and stem selection/crossfade participate in the read set.
Add only context derived from those exact addresses.

For Key Lock, the proof must include actual native/FIFO continuation, source-feed
history, retained filter state, rate/smoothing changes, seek/loop/stem transition
and adopted target output frame. The 4096-output-frame worker horizon is not a
universal source-frame margin. A bound must cover all admitted operation ranges
and retained state, not just the initial ratio. If new control intent exceeds
proved context, prepare a new context/finite full-track exception before adoption.
If budget cannot admit it, expose unavailable/error while old state remains.

Use full-track residency as an explicit correctness fallback when a finite DSP
bound is unproved. It can reduce the performance benefit and must be counted.
Do not introduce a continuous disk-to-callback pipeline. C2 acceptance requires
independent full-buffer versus resident output/history parity and actual read-set
coverage, not tests that repeat the address helper's own calculation.

## Finite readiness and controls

State sequence: Requested -> admitted Preparing -> validated Prepared ->
native Enqueued -> effective ACK/Ready, or Failed/Cancelled/Stale. There is no
effective state change at Prepared or Enqueued alone. Requested durable loop
intent can differ from effective audio; show pending/error honestly, retain
effective playhead and preserve explicit Manual/Tap/Legacy/Automatic priority.

A transaction binds complete source, request, intent, region/window revision,
accepted projection where applicable, full-mix/stem windows, read plan, rate/
DSP/native context and adoption checkpoint. Reuse existing request mutexes/
epochs, NativeHistoryPermit, bounded command/ACK admission and reserved retirement.
Late worker results, full rings or missing retirement capacity leave old audio
valid. Recheck at capture, worker completion, enqueue and callback adoption.
Coalesce rapid edits to the latest intent without resetting audio progression.

| Action | Preparation and preserved behavior |
| --- | --- |
| Saved short-loop startup | Complete cache validation and matching loop/context first; report pending until native Ready. Full metadata remains visible; no hidden complete-track playback allocation. |
| Resident loop edit | Existing immediate bounded path if all target context is proved resident. Otherwise finite preparation/ACK. No apply/save button. |
| Editor/view jump | A bounded full-source lease serves existing cached waveform/sample-zoom queries; view start/end uses full duration. Navigation never seeks audio. Initial full lease may transiently retain full PCM; reuse by source and count/release it. |
| Analysis/key/stem job | Full-source non-RT lease; whole PCM/evidence transforms with current source/request guards. Window bytes are never complete input. Stem generation and adoption of a new complete StemSet remain inactive-only. |
| Seek inside admitted context | Existing explicit seek semantics with matching window/DSP readiness; no marker/grid/bars edit. Paused remains paused; stopped seek is still a no-op. |
| Seek before/after resident loop | Selected simple policy: prepare full-track resident exception. After ACK, intro reaches loop; tail reaches actual full end then wraps. Preserve the old effective playhead until ACK. No small target-only window or background continuous streaming. |
| ALL | Requested manual [0,full duration), auto disabled; prepare full-track exception unless already resident. Failure retains prior effective audio. |
| Return to finite loop | Explicit finite replacement prepared against current cursor/history; release full exception after final voice/job reader retires. Never evict an active trajectory's required samples. |
| Stems/masks | Full mix plus the four component windows use matching source/window revisions, range coverage and current accepted projection. A finite window relocation may adopt while active only for the identical complete source and already accepted complete StemSet, with proved history and ACK; it cannot introduce generated audio or new timing evidence. Preserve shared cursor/taps/ramp/filter history and full-mix fallback. Instrumental is not a fifth live component. |

Initially reuse a bounded complete PCM reader/lease for editor and analyzer
integration rather than invent a paged editor framework. Share concurrent readers
of the same derivative. Opening the editor or running analysis is a measured
transient full-source exception, not a change to durable loop intent. Close,
cancel and superseded results release those leases off-thread. View/result keys
bind complete identity and query; same-source window changes do not reset view.

## Ownership, retirement and cleanup

Distinguish original assignment owners, shared digest artifact owners, active
read leases and temporary job ownership. Count pad/bank, editor/analyzer/stem
worker, queued command, prepared native history and active/pinned voice readers.
Unload invalidates all per-pad requests first, admits native stop/unload with
ordered feedback, clears eligibility for restored stems/timing and resets
track-bound intent. It does not delete bytes still held by another owner.

Large handles and reader/file leases transfer to existing off-thread retirement
with reserved capacity. Cache lifecycle must not rely on the last-resort
`mem::forget` fallback as evidence of complete cleanup. Admission/backpressure
and a retained off-thread owner cover saturation; demonstrate eventual drain.
No last Arc payload/file-lease destructor on the callback.

Under a cleanup/admission gate, mark an unowned entry retiring so no new reader
can race deletion, wait for final readers/queued handles to retire, then remove
only resolved owned paths. Missing files are harmless; Windows sharing violations
defer deletion and are retried by bounded cleanup. New readers must either
cancel retirement before deletion or create a new validated immutable entry.
Shared-digest PCM survives until the last assignment and reader; each byte-exact
original survives until its own last assignment and dependent readers/jobs.
Cancelled stale staging files are removed only by their exclusive owner.
Stem eligibility/metadata is revoked immediately, but old generation readers
retain their files until retirement. Keep the pad-labelled `samples/stems/#N/`
container and isolate owned generations within it. An old cleanup job deletes
only its own retired generation; it cannot recursively delete a container now
holding a newer canonical set. Remove the container only after all generations
and readers are gone, under the same cleanup/admission gate.
Crash recovery removes recognized unreferenced staging safely; it cannot treat
unknown files, project config, originals or private audio as garbage.

## Bounded implementation and evidence plan

- **C1a, exact next slice:** bounded cold-load job admission, immutable copy-first
  input, full decoder/playback artifact writer and manifest with complete actual
  digests, exclusive atomic cold commit, request-guarded all-or-none publication
  and snapshot/staging cancellation/queue-failure rollback. Retain current
  full-buffer playback. Resolve/prove decoder boundary
  policy and reuse the stronger resampler dimension/tail rules. Integrate cold
  path with existing guards; do not implement resident windows.
- **C1b:** complete validated warm reuse extending guarded cold publication, shared
  digest/assignment leases, containment, cancellation/unload/shutdown and eventual
  last-owner cleanup. Measure integrity bytes/CPU preliminarily, without startup
  improvement claims. Preserve current accepted save/export behavior.
- **C2a:** complete-source authority versus resident descriptor/address translation,
  proved context and saved-loop startup including matched stems. Rebind CURRENT/
  MIDI/history consumers; eliminate hidden full playback pins for normal loops.
- **C2b:** finite readiness across controls, full editor/analysis access and explicit
  nonresident seek/ALL/full-DSP exceptions, real source/output/history parity and
  <=1-loaded-frame loop gates. No streaming framework.
- **C3:** actual cold/warm measurements and lifecycle/resource/parity acceptance on
  200 occupied pads before claiming a startup/RAM benefit.

For each implementation slice use meaningful changed-area tests, full required
Debug/Release checks for productive audio/persistence/control changes, official
strict validation of affected changes and independent staged-tree semantic/hash
review. C0 uses official strict validation, source/test audit and documentation/
diff/link checks; it does not repeat unchanged runtime suites or run the app.

Required independent fixtures cover 44.1/48/96 kHz, mono/stereo, each supported
decoder, leading/trailing impulses, silence and codec delay/padding, empty/short/
exact/partial/many FFT chunks, source-zero/phase/tail and exact integer ceiling
length. Compare full actual cached digests and decoded byte/sample evidence,
including original replacement/ABA, same-size corruption, version/device change,
partial writes/crashes, cancellation before/during/after publication and shutdown.

Resident proof covers true fractional periods/starts/tails, P above/below H,
75/1000-cycle unwrapped <=1-loaded-frame timing, fractional ratios/rate smoothing,
callback partitions, both interpolation taps, native Key Lock/history, stem/filter/
clear changes, intro/tail/paused/stopped seek, ALL, editor/analysis complete extents,
stale window/source/accepted revision and saturated command/retirement lanes.

C3 freezes identical original hashes, durations, saved loops, processing/device
versions and pad assignments for old/new comparisons. Test 200 occupied pads
with short loops from long sources, shared/unique digests and separate explicit
full-track/Key Lock/editor/analysis/seek/ALL cases. Occupied is not 200 simultaneous
voices; report voice/native-handle limits and 1/2/4/6 active-pad plus bounded stress.
Cold means no compatible committed PCM; warm means validated complete caches.
Record filesystem page-cache conditions separately; do not call an OS-warm run
disk-cold. Measure time to each/all Ready, process and resident/transient PCM RAM,
worker/queue/96-handle peaks, copy/decode/cache/verification bytes, disk capacity,
CPU, save-integrity latency/I/O and final-reader cleanup under cancellation.

Publish outcomes and regressions with source identity, timing revisions and
realtime review. Improvements remain a hypothesis until those measurements.
Numerical/native parity does not prove human hearing or real devices; those
gates remain open for final pre-port acceptance. No part of C0-C3 starts planning
or implementing the full application Rust port.
