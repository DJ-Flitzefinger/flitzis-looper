# Complete PCM cache and finite loop residency

Status: C1a/C1b cold/warm loading, C2a finite saved-loop residency, C2b control
readiness and C3 measured acceptance, 2026-10-08.
The existing `AudioEngine.load_sample_async` now runs the copy-first path below.
Validated warm reuse and last-owner lifecycle remain authoritative. C2a separates
complete source authority from resident storage and restores saved finite loops.
[C3 measurements](pcm-cache-measurements.md) record actual cold/warm readiness,
PCM ownership, process resources, exceptions and lifecycle costs with paired
finite/full results and their limits. Actual human/device/listening acceptance
remains open. The active
[OpenSpec change](../openspec/changes/cache-full-pcm-and-resident-loops/proposal.md)
records the contracts; its
[task list](../openspec/changes/cache-full-pcm-and-resident-loops/tasks.md)
records the completed bounded implementation tasks.

C3 preparation releases an unused converted intermediate Vec after channel mapping,
before playback Arc creation and its declared PCM checkpoint. Complete-cache
[validation waits cancellably](../openspec/changes/cache-full-pcm-and-resident-loops/specs/pcm-cache-residency/spec.md#requirement-cancellable-concurrent-cache-validation)
off-thread for admission, so transient validation contention cannot trigger
redundant cold decoding or a new cache generation.

Resident-window failure/cancellation settles its exact control-intent generation
before publishing `WindowState.terminal` with Release ordering. An observer of that
terminal state therefore cannot still observe that failed job's pending owner;
a newer replacement intent remains protected by the existing generation CAS.
This applies to queued shutdown, panic, worker errors, cancellation, timeout and
supersession. It clears only scalar control readiness: worker/queued PCM and
immutable-reader pins retain their actual lifetimes. Native publication/ACK status
and claimed callback-tail behavior remain separate from this failure settlement.
The deterministic terminal-observation tests freeze the real guard before join;
this bounded J0 correction supplies no full B6 or shared-subscriber acceptance.

## Delivered cold path (C1a)

### Complete-extent streaming (X11-LONG)

FullMix imports retain one sealed original, stream decoder packets into complete
`decoder.f32le`, then continuously resample that disk-backed PCM into complete
`playback.f32le`. Rubato state survives block boundaries. Integer-ceiling extent,
delay trimming, partial input and bounded zero-tail flushing retain the decoder
origin and transform policy. Fresh warm reopening scans every original/decoder/
playback byte, checks finite samples, interleaved and signed-f64-mean mono hashes,
and exact EOF before selected residency can be published.

| Productive caller | Complete storage and live demand |
| --- | --- |
| Public Native import with finite bounds and automatic analysis | Stream complete storage; retain the requested loop. Attempt separately admitted analysis, then return matching analysis or a visible analysis error with source success. |
| Public Native import without analysis | Stream complete storage; read supplied finite bounds, or the complete source when bounds are absent. |
| Ordinary UI import | Prepare and fully verify complete disk storage first; resolve genuine incoming activity/analysis and Auto8bars or newer addressed demand before the selected live read. Finite ON/OFF selection, actual initial BPM lock and source-bound controls have a [passing bounded software gate](#ordinary-incoming-loader--2026-10-13). |
| Canonical restore / fresh warm | Fully verify sealed complete storage; restore the actual finite hint or genuine ALL demand. Known physical saved ON demand selects KeyLockFiniteLoop for a proper partial range and KeyLockFullTrack for a complete range; its separate passing bounded software gates are [reported below](#initial-saved-finite-keylock-on--2026-10-12). |
| Noncanonical migration | Prepare a one-frame identity seed and complete lease. Read the subscriber's captured current finite range when adopting. |
| Complete stem preparation | Use the complete source lease, full WAV scans and bounded alignment/conversion; the metadata seed is never an analysis or playback choice. |
| Complete pair reopening | Fully verify all five WAV/PCM leaves and the common descriptor; prepare four matching selected component views. |

Explicit ALL reads retain their real live allocation/overlap admission and may
fail for a large source. Complete ON demand and absent trustworthy saved geometry
still read the complete source under its actual live budget. Known proper partial
physical ON demand uses the shared finite selection; no unknown geometry is guessed.
One-frame material metadata preparation grants no ALL
residency. Warm verification scratch is 64 KiB, independent of disk extent.
Ordinary UI/default-eight-bars preparation now resolves the physical selection
before live allocation. Its [bounded incoming report](#ordinary-incoming-loader--2026-10-13)
records bounded ON/OFF, actual initial BPM-lock evidence and its final passing
software gate. Complete storage, the historical OFF
paths and the separate saved ON delivery do not close full E11-03/X11-LONG,
whole P4b/P5b or accepted-musical/resource/Human gates.
Stem alignment performs full global-maxima and first-threshold scans before
retaining a four-second onset/correlation window. Previous absolute envelopes
cross scan blocks; ordered f32 component summation, ascending lag ties, PCM16
normalization, signed shifts, zero padding and the exact final frame are retained.
All five aligned digests determine generation identity before publication.

Small PCM16 output keeps the byte-exact 44-byte RIFF format. Large output uses
RF64 with checked ds64 u64 geometry and actual EOF across project/model writers,
Python readers, native stem parsing and an explicitly registered Symphonia RF64
demuxer. The source manifest records `rf64-wave-ds64-flitzi-demux-v1`; it never
claims RIFF provenance for RF64 input. Unknown ds64 table entries remain rejected.

Demucs keeps its original CLI/model/inference path. Its PCM16 save adapter bounds
host conversion and uses FFmpeg RF64 output; model tensor/activation memory can
still scale with inference. BS-RoFormer and project conversion use bounded PCM16
writers with exclusive attempt leaves, flush/reopen/full verification and captured
identity cleanup. These storage bounds do not claim constant-memory inference.
The callback receives prepared resident PCM only; every file/decoder/resampler/
hash/model operation remains outside realtime. This delivery does not complete
new active StemSet adoption, P4b/P5a, aggregate P6 resources or Human listening.

The pending [pad-owned PCM program](pad-owned-pcm-program.md) extends this delivered
foundation. It replaces obsolete legacy physical containers with canonical immutable material
versions and equal all-bank content users, with #1..#216 only stable membership,
adds persistent aligned stem PCM/direct range reads, and requires actual216-slot
residency plus finite DSP proof. Existing measured200-pad evidence and historical
full-track KEYLOCK fallback do not complete those new requirements. SourceVersion
path migration needs verified lineage and fresh native source/timing ACK; existing
cache/manifests/readers are not edited in place. P1a now supplies typed canonical
import/restore storage and durable material/content identity; equal subscribers,
migration, arrangement and further residency remain pending.

The first P4b vertical prepares stopped/current Normal-loop KEYLOCK ranges through
the retained readers and existing WindowWork transaction. Actual tap coverage,
complete source/window/timing/selection binding and native history permits guard
real native/FIFO catch-up and continued callback input. A storage-only ACK keeps
already adopted native/FIFO/filter chronology and old reader ownership intact;
pending native work remains exact-window-bound. The bounded resident-transition
context checks both actual FullMix/stem sides for current-source NormalLoop voices
with an already ACKed identical resident committed set before native mode/mask
mutation and on productive feed. New history preparation waits for the ramp to
settle while the wet native/FIFO/filter owner continues; later actual catch-up and
adoption retain their own permits. The bounded lifecycle/source-custody
implementation covers fixed physical
NormalLoop geometry with genuine production-k=0/non-unity pause/resume/rate and
storage-ACK continuity, same-source accepted-period refresh/clear and old active/
paused voice custody through bank replacement. Pending native work keeps exact
captured-window/projection/checkpoint fences; only already adopted history may
retain the proved storage-only effective-source permit with its original readers.
Wet output must continue past the first resumed callback through further real
preparation/adoption. New old-voice preparation needs fresh actual Voice/Source/
Timing/Selection/History capture and matching own permits; a PCM pin cannot revive
a stale current-bank candidate or substitute the replacement bank's authority.
Future-bank ranges need actual WindowWork admission and own ACK, then fresh public
capture and independent native preparation permits. Retrigger cuts DSP/history
into that acknowledged source after reservation; failure preserves old playback,
parameters and owners. Existing readers/retirement paths count actual old/new held
backing/history peak and each distinct final use. Finite seek/intro/tail, physical
cuts, remaining full P4b resource/music/history/causality and P5a remain guarded or
open. Historical saved-load fallback evidence remains scoped to its original source;
the bounded current saved ON selection and pending proof are described below.

WindowWork peak admission now snapshots the existing same-pad 128-entry weak PCM
history and resolves its actual original assignment readers and corresponding
paired/shared component allocations in ProjectAssets. Equal file paths or content
on another pad alone confer no membership. When pads share the identical complete
PCM Arc, the existing Weak history cannot distinguish its assignments: admission
conservatively includes every actual assignment reader and associated component
backing linked through that allocation. It may therefore reject earlier near the
1 GiB operation limit; it does not claim exact pad-exclusive accounting. Arc
identity deduplicates allocations; the snapshot holds old source, voice, job and
history readers through admission. A new
range needs a free live history slot before superseding the previous intent and
again before allocating. Reusing an already registered backing needs no new slot.
The existing resident-stem ownership mutex serializes WindowWork peak capture,
source/component preparation and reader registration across the two workers and
pads. A cancelled partial component allocation is destroyed before that guard
opens, so another WindowWork cannot admit against an incomplete snapshot. The
existing history mutex spans source-range read and registration, then is released
before the asset gate to preserve collector lock order. These worker locks add no
callback lock or new registry. Per-operation
admission and the existing two-worker transient bound remain distinct from a
measurement of total concurrent process RAM.

New imports preserve the first verified exact filename and bytes under
`samples/<Originalfilename>/<Originalfilename>`. Hidden `.material.json` binds
the complete SHA-256, byte length and canonical original leaf; the durable
32-hex material ID is a compact identifier, not the full content witness.
Equal bytes under other names reuse that root and leaf. Windows namespace,
case, occupied-name and path-length collisions use a readable directory prefix
and an extended full-content SHA suffix without overwriting unknown data.
Complete PCM generations live in that material's
`.pcm-cache/v1/.ready-<32-lowercase-hex>/`; stem generation uses its `stems/` root.
Explicit import and restore intent prevents an absolute canonical restore from
being recopied as an external import. Safe legacy flat originals, global PCM
generations, M-ID originals/caches and slot stem sets remain readable during
verified serial journal/config/reference migration. See
[readable material validation](readable-material-validation.md).

The native typed resolver rejects traversal before normalization, reparse ancestors,
Windows leaf aliases, invalid IDs and wrong artifact kinds. Directory ancestors are
guarded before child creation. Native Success reserves an original owner before
enqueue; Python validates and acknowledges the delivered exact original lease
before publishing its project path and durable content ID. Registry saturation,
stale/no-receiver events and rejected adoption retain prior assignments and retire
only the attempt's creations. Cleanup still waits for actual readers and removes
only exact known files and empty recognized material containers. Readable binding
metadata/root cleanup requires the typed receipt of the actual native directory
and marker birth, including exact root/marker FileIDs, marker extent and full SHA.
Durable receipt recovery additionally verifies current config/journal, current
inventory and fresh Native adoption before restoring existing ownership. Native
inventory remains retained through physical terminal cleanup after the last
reader; Python caller/GC release is insufficient. Unreceipted or changed markers
and unknown roots remain protected. A freshly recognized schema or replacement
marker never creates deletion authority. Final restart/partial-retirement and
physical terminal evidence is tracked in the MATERIAL report.

A recovery-only typed retired-original route resumes an own root after verified
binding retirement. Exact native birth root FileID, absent original and genuinely
empty root are checked under custody against the unchanged latest created-target
journal's actual original-terminal record, current config/content intent and
registered inventory. A present marker still requires its complete birth
FileID/extent/SHA. Ordinary lookup never adopts an absent binding. Unknown
children, replacements/reparse roots, reappeared original/marker or missing
terminal/birth evidence remain protected; inventory/capacity and real root-reader
protections extend through physical root terminal after caller release. The
six-mode process-exit/fresh-child exercise and current complete Debug/Release
suites pass at this bounded software scope. Full predecessor/integration,
resource/music and Human gates remain open in the MATERIAL report.

`cold_jobs.rs` admits at most two active workers and 32 queued/reserved jobs.
The queue bound includes reservations made before request mutation. Each active
job admits at most 1 GiB of transient PCM. Complete decoder and playback PCM are
written to disk in bounded blocks; their total extent is not a scratch allocation.
Admission covers decoder packet/workspace, continuous FFT resampling/channel
mapping and the actual requested resident view, including Vec-to-Arc overlap.
Decoder workspace admits conservative supported-codec packet
maxima; ALAC cookie block length/rate/channels are checked before constructor
allocation, with actual codec configuration SHA256 recorded. Automatic analyzer
inputs have a separate conservative 512-MiB admission calculation, with the held
selected view included in the operation peak. An inadmissible automatic analysis
produces a source-bound `analysis_error` with the successful native source ACK;
it does not reject an otherwise admissible selected loop. The controller displays
that error after clearing analysis-task state. Stale timing/request/source results
cannot replace newer timing or its errors.
Thus the cold lane's admitted transient PCM is at most 2 GiB, excluding previous
resident pad/voice PCM and old-source request checkpoints (including pins retained
after unload), existing native preparation pools and non-PCM analyzer
model/DSP storage. These are limits, not measured process peak RAM. Default
analyzer behavior is retained; opaque native analysis kernels finish their bounded
input before a subsequent cancellation check. Other stages check cancellation
between packets/chunks and before commit/publication.

`cold_store.rs` opens actual files with Windows FILE_SHARE_READ exclusion of
writes/deletion, verifies opened leaf/ancestor reparse attributes and hashes
the complete incoming reader in bounded chunks. It retains that same sealed
reader through lookup and `sample_loader/cold.rs` decode. Compatible hits also
retain and fully verify the canonical original, without a temporary snapshot or
source copy; only a genuine miss publishes one exact durable original. External
replacement or A-to-B-to-A after capture cannot change decoder input. Inputs with
an existing incompatible writer fail safely. This tested stable-capture protocol
is Windows-only; other platforms fail Unsupported until an equivalent protocol
is proved. Imports retain a byte-exact collision-safe original; restoration
captures a contained existing original without another durable original.

Each cold attempt creates complete `decoder.f32le`, `playback.f32le` and
`manifest.json` under its material's `.pcm-cache/v1/.staging-<pid>-<generation>/`;
restored legacy flat originals retain the existing global `samples/.pcm-cache/v1/`.
Canonical versioned descriptors bind actual original digest/bytes, decoder
container/codec/library/options/packet error policy, full decoder interleaved and
arithmetic-channel-mean-f64-to-f32 digests, actual rate/channels/full frames/zero,
and the full playback parent identity, own digests/dimensions and executed
resampler/channel transform. No window or historical accepted flag supplies
these identities. Original decoder rate/layout is retained independently of the
selected playback/device rate, with no forced 48-kHz conversion.

Files are flushed, the exclusive complete directory is renamed on the same
filesystem to `.ready-<32-lowercase-hex>` for canonical materials or the existing
`<full-identity>-<pid>-<generation>` for legacy storage, and every committed file is
reopened with immutable sharing and fully reverified before native publication.
Existing partial or corrupt generations are never overwritten or reused.
Fault/cancellation cleanup deletes only this attempt's creations,
after closing dependent readers; external sources/private evidence survive.
Sealed successful original/PCM/manifest/directory leases stay outside realtime
ownership. Successful entries remain durable until explicit assignment retirement
and the last dependent reader. File flush plus atomic rename
and post-rename verification prove atomic complete visibility and injected failure
rollback, not a Windows power-loss durability/recovery guarantee. Crash-left
staging/partial entries are ineligible. Recovery recognizes only owned staging
markers with matching root, generation and dead process creation identity;
live matching or unqueryable process identities, unknown files and unrelated
directories survive. A reused PID with a different process creation time proves
the original creator has exited and permits recognized unreferenced recovery.

`cold_load.rs` retains previous source/digest/generation/lease through admission
and preparation. Source/request/device format and timing-intent epoch are
rechecked before enqueue. Normal cold selection and Automatic restore reject
intervening timing edits; initial empty Legacy restoration admits its existing
startup settings through enqueue and native adoption. Reserved single-producer capacity makes source,
digest, generation, intent and queued handle publication one transaction. A fixed
source/epoch guard and scalar acknowledgement govern `LoadColdSample`; the
callback reserves MAX_VOICES+4 retirement slots and one feedback slot before
stopping the application's old voices and adopting the complete new bank. The application explicitly requests `replace_assignment=True`; direct native
`load_sample_async` defaults to false and retains active voices pinned to their
previous source, as does `LoadSample` bank replacement.

The worker registers retained sealed ownership before native enqueue. Pad adoption
and the Success event wait for matching native ACK.
Blocked retirement/feedback keeps old audio and project metadata effective.
Pending adoption has a 30-second deadline; cancellation/rejection restores only
its still-current proposed control source, preserving a newer unload/load or
Manual/Tap intent. A matching ACK linearizes adoption, and later timing edits are
reported as timing-stale source success without resetting their authority.
The callback's claimed tail is bounded and nonfallible after a second source/epoch
check. An exceptional backend stall after claim reaches a finite timeout (or
shutdown), retains complete files/sealed ownership while still current, and
reports unconfirmed adoption without Success or an unsafe source rollback. A
superseding generation cannot overwrite a job's unique fixed scalar ACK.
Unconfirmed current source is fenced from new starts until explicit unload/restart
or a fresh successful assignment. Off-thread reader ownership now reconciles
subsequent explicit retirement and shutdown without inventing adoption.
Prior-audio preservation is proved for rejection before
this irreversible callback tail, not a stalled backend after it starts. ACKed
files remain protected through metadata delivery. A stale delivered Success
retires its exact original; a pending delivery retains ownership even when the
native bank has already unloaded. Engine shutdown reconciles undelivered orphans.
Only prebuilt PCM and a bounded per-job atomic token allocated by control enter
the callback; even the atomic token is retired through the reserved off-thread
queue. No filesystem/hashing/decoding/JSON/GIL/locks or heavy allocation/
retirement was added there.

While cold enqueue is pending, the control cache contains the proposed source,
so CURRENT/MIDI capture can be temporarily unavailable; it cannot promote the
new source's historical timing. Old bank/voice/accepted handles remain effective,
and rejection restores the prior control source. Existing input-runtime refresh
rebinding continues to use native source/authority signatures.

Python now keeps assignment/duration/analysis/settings/stems on admission or
pre-adoption worker failure. Superseded in-flight analysis is explicitly settled
as cancelled rather than left busy. Successful matching source replacement
records complete source metadata and then refreshes defaults/derived control intent; a derived setter
failure is visible while that new source and its matching metadata stay valid.
Native unload admission now precedes Python source/task/settings mutation, so a
full native queue preserves the previous project/session and deferred restore.
Explicit unload/replacement revoke Python eligibility and schedule last-reader
cleanup for the exact original and stem generation. Saved Automatic intent remains unresolved even after an
initial admission/preparation error. Only existing full-content verification
and fresh timing ACK can establish CURRENT. Historical acceptance is never upgraded
by the PCM manifest. Existing save-integrity verification remains in force.

Startup restoration uses a private deferred-admission table bounded by the
216 available pad IDs. Exact queue-full defers remaining saved sources rather
than losing them; native event polling retries at most eight admissions and
stops at the first repeated full queue. Stopped/invalid preparation failures
remain terminal and visible. New admitted selection, unload, shutdown or changed
saved path cancels the corresponding deferred entry; retry uses the latest timing
intent. Equivalent Windows slash/backslash assignments restore canonical paths
without resetting saved timing/mix/loop settings. Full-pad mocked admission tests
prove scheduling/control behavior, not a 200-pad RAM/readiness measurement.

## Decoder and playback boundary evidence

Symphonia 0.5.5 explicitly keeps gapless disabled, verification disabled, and
origin at the first decoded frame. Declared delay/padding and actual decoded
extent are recorded; no leading silence, priming or padding is heuristically
removed. Independent encoded fixtures and FFmpeg/ffprobe references prove the
policy rather than assuming decoder equality. For example the 11023-frame MP3
fixtures decode to 12672 frames: 1105 priming plus 544 padding remain. AAC retains
1024 priming frames; Vorbis retains its actual decoder extent; lossless source
samples and first/last impulses are checked at 44.1/48/96 kHz. MP3 supports only
44.1/48 kHz in this fixture encoder and is not relabelled as a 96-kHz source.

Playback uses the existing Rubato 1.0.0 FFT Input/1024/one-subchunk transform,
with the stronger analyzer integer-ceiling, delay-plus-length and bounded tail
rules reused for multichannel conversion. Output is exactly
`ceil(source_frames * output_rate / source_rate)`, after one integer algorithmic
delay trim and a bounded zero-padded tail. Independent fully padded raw FFT
oracles compare every sample for all nine 44.1/48/96-kHz rate pairs, mono/stereo,
1/17/1023/1024/1025/4095/4096/11023 frames, and zero/tail impulses. Integer delay
compensation retains declared source zero; tested impulse peaks stay within one
output frame of rounded source positions, while full samples match the independent
fully padded FFT oracle within 2e-6. No analytic subframe phase bound, zero acoustic
delay or identical peak-frame placement is claimed.

Headless productive tests use the same admission/preparation/native command drain,
not a disconnected helper-only path. They exercise complete original/artifact
identity, queue failure, native backpressure/ACK, cancellation, intent races and
old-source preservation. Full Debug/Release checks retain the G3 numerical,
current accepted, pinned source and native-history tests. Six Windows headless
integration tests first perform actual cold-worker/native adoption ACK, then call
the public `OfflineAnalysisJob` PyO3 wrappers through embedded Python. They prove
full export/playback ownership, source replacement and busy retirement, exact
staging/key accounting, sealed-file retirement, missing-parent failure before
key work, and complete packed-result publication/readmission.

Pre-stream Python validation/poll tests use a real uninitialized `AudioEngine`.
The remaining public-API integration fixture requires `run()` to initialize its
CPAL stream/input runtime, including tests whose logical assertions are offline;
these stay explicit `--audio-devices` opt-in. Their native core contracts are
tested headlessly; the skipped Python service-to-stream glue is not counted as
passed. This step runs offline checks.
Actual human/device/hearing gates remain OPEN until final pre-port acceptance.

## Delivered warm reuse and ownership (C1b)

Each load still captures and verifies the complete actual original before reuse.
The bounded candidate scan (4096 entries, 256-KiB manifest) selects the actual
sealed decoder configuration and output rate/channel transform. Fresh immutable
file leases rehash every decoder/playback sample, recompute each mono digest,
check checked byte extents/EOF and canonical versioned descriptors. Invalid,
partial, old-version or incompatible candidates cause ordinary cold regeneration
in a new exclusive generation. A complete compatible candidate skips decoding
and conversion. Verified live PCM can be shared across pads; a per-digest/device
preparation gate serializes preparation through commit and binding, then releases
before native ACK. Every subscriber keeps its own request/cancellation guard.

Failed publication revokes only its own assignment ID. Its rollback can request
physical cache deletion only for a generation created by that attempt; a
preexisting complete warm generation survives a failed fresh-process restore.
A saved original with matching captured file identity keeps one bounded
validated descriptor and a separate durable file-only ID after such failure,
with no dead PCM backing pin. A transient source job cannot create that saved
ownership, and the failed native ID stays retired without Success. Later explicit
last-owner unload still retires the known original and cache off-thread; ordinary
shutdown preserves them even if another original sharing the cache has retired.
Within the existing 1024-record bound, ordinary token/engine shutdown retains one
saved file-only descriptor per original/cache relation until explicit retirement
or process exit. Repeated restores compact that descriptor instead of accumulating
unreachable durable IDs; dead PCM backing storage is still released.

Same-pad source identity remains distinct even for identical content: if an old
same-pad queued/bank/voice/job/history PCM reader still lives, off-thread admission
makes a complete new Arc within the checked 1-GiB overlap budget. The bounded
128-entry live same-pad history rejects further admission rather than evicting a
live reader. Cross-pad readers can share PCM and immutable cache files. Rollback
requires the exact request/generation and proposed source, preserving newer loads.

`project_assets.rs` keeps a process-wide off-thread registry of exact assignment
tokens, pending deliveries and weak native PCM readers (1024 native records,
1024 retirement requests, 4096 path owners and 1024 registered history books).
Dead historical duplicates compact under the gate while live/pending readers
remain protected. The worker removes dead PCM Weak references from history,
reader metadata and cache objects so they cannot retain backing allocations after
the last reader. Python `asset_lifecycle.py` acquires every surviving
assignment before releasing removed ones and pins active separator jobs.
Removing one of several current assignments to the exact same original releases
only that token; path retirement starts only after its last project assignment
is removed. A stale delivered Success for a still-assigned original acknowledges
metadata ownership without scheduling that path for deletion. Native
queued commands, banks, pinned voices and prepared/offline jobs keep actual PCM
alive. A cache has separate retirement ownership for each concrete original;
one failed subscriber or retired pad cannot delete another live validated cache
assignment or its original. A different saved original whose digest has not yet
been prepared owns its original path; if its optional PCM is retired before that
preparation, its next load safely regenerates from the untouched original.
Shutdown releases Python assignment tokens and marks undelivered native metadata
orphaned. Actual queued/job/PCM readers retire as they drain; a stopped engine may
retain loaded PCM for restart. Saved originals and caches survive ordinary shutdown.

Stem generation writes a private `.generation-<uuid>` and publishes an immutable
`.ready-<uuid>` only after all five stems and the complete marker exist. Metadata
selects that exact set after native publication admission; failure restores the
previous set. Cleanup removes only known leaves from its own retired generation.
Validated legacy canonical sets remain restorable and retire only declared files.
Reacquiring a saved assignment revokes its older deferred retirement before retry
admission. A canonical legacy pad container revokes only its six declared direct
files; cleanup of a failed newer generation remains scheduled.
Unknown files and newer generations are preserved. A final empty pad container
can be removed nonrecursively under the gate only when no owner, reader or pending
retirement remains; an empty container may also remain as harmless metadata.

Windows cleanup excludes rename/delete of checked ancestors and the generation,
checks captured file identity, and deletes the validated opened file by handle.
Traversal, reparse points, external originals and reserved project metadata are
rejected. Missing files are harmless; sharing violations retry off-thread with
at most eight eligible deletions per poll. Python retains ownership and retries
native retirement admission failures. The store reserves three cleanup slots
before any exclusive capture writes; at most 4096 slots cover live file owners
and deferred deletions, so saturation cannot create untracked rollback files.
Python separately bounds pending targets, retained owners and future reservations
to 4096 weighted slots (16 per source admission, 18 per separator job, two
transferred to a pending stem ACK). Backpressure precedes intent mutation.
Retry selection rotates, and terminal preservation errors are capped at 16
reported paths per service while leaving the affected bytes intact.
Recognized exclusive failed/crash staging uses the same contained ownership rules.
No file handle, JSON, lock, hash or filesystem destructor enters the callback.

The historical C1b long-source warm and native export/project-save probes
described in [development](development.md) record preliminary integrity costs.
They preserve complete save verification, fresh timing ACK and atomic
previous-config/dirty-state failure behavior. The separate
[C3 measurements](pcm-cache-measurements.md) cover current 200-pad readiness,
resources and actual long-source save costs; device/listening acceptance stays open.

## Finite saved-loop residency (C2a)

`CompleteSourceIdentity` retains complete rate/channels/frame count, source zero
and immutable original/playback/mono/canonical-transform digests without owning
PCM. Each new load assignment gets a fresh runtime identity even when its full
cache bytes or PCM backing are shared. `ResidentSourceView` shares that identity
and declares absolute start, resident extent, guarded revision and context.
CURRENT/loaded shape/accepted evidence describe complete source geometry;
MIDI/prepared permits additionally fence the effective resident allocation and
revision. A same-source view replacement does not create another accepted record.
Old voices retain their actual source, timing and loop geometry.

Python startup resolves existing durable physical loop intent through the shared
marker/grid projection and passes absolute seconds to `load_sample_async`.
Historical Automatic period/origin are geometry hints only; fresh complete
evidence verification and timing ACK still establish CURRENT. Missing/unsupported
finite geometry keeps an explicit complete view. Source Success follows actual
native source/window ACK, and complete duration remains source-relative.
An identical saved setting applied during preparation is allowed; a different
admitted loop or DSP context cancels the captured window through callback adoption.

Normal dry playback needs the admitted physical `[a,e)` and no guard PCM. Physical
`H=e-a` and compatible accepted musical `P` remain distinct. The actual reader's
last knot is `min(ceil(P)-1,H-1)` and its seam joins that knot to `a` at `P`.
Both taps stay inside `[a,e)`; source rate/smoothing alter phase frequency rather
than this read set. Per-pad filters consume rendered samples and retain their
fixed history. Historical C2a labelled unproved finite native continuation as
`full-track-key-lock-continuation-unproved-v1` and admitted complete saved Key Lock
PCM. That historical complete allocation was not a streaming pipeline. The bounded
current physical saved ON selection below admits a verified proper partial range;
it does not guess source margins from the 4096-output-frame worker horizon.

Warm restoration still verifies the complete original, decoder and playback
bytes. It then reads only the selected absolute resident range into playback RAM.
Cold loading writes and verifies complete artifacts before copying the admitted
window and releasing temporary full PCM. The shared warm PCM locator accepts
only exact complete extents, so a cropped window cannot become another load's
complete PCM. The bounded same-pad reader history registers the final window.
Fresh complete timing readers compare the full descriptor and the already sealed
FileID, admit their overlap budget, and register their real weak PCM ownership
with the existing assignment. Accepted tickets retain finite references; complete
temporary evidence PCM retires when that work finishes.

Stem preparation aligns complete artifact PCM against a bounded complete source
reader, hashes the complete set, then retains matching resident component views.
Full mix and stems share absolute geometry/revision and exact accepted projection.
Only four component bits can feed the live mixer; cached instrumental is not a
fifth live component. A new complete set remains inactive-only. Storage relocation
can adopt while active only for the same complete source and already accepted
complete set, with current trajectory read coverage and matching history context.
The old effective handles remain until guarded native ACK; actual native/FIFO,
filter, fraction and rate history stay with their source. Queued/prepared readers
continue pinning old allocations until final off-thread retirement.

`relocate_resident_window` uses the existing two-worker/32-queued preparation lane
with checked 1-GiB transient PCM admission. Its opaque ticket separates preparing,
pending/adopting, accepted, rejected, failed and cancelled state. Callback adoption
claims the transaction, reserves retirement, swaps matching fullmix/components,
publishes the resident fence and acknowledges. Control reconciliation observes
ACK without driving audio progression. Complete-source identities/reader records
remain within the existing bounded ownership registry.

C2a supplied the saved-loop foundation. C2b adds the control/editor/analysis/
nonresident-seek/ALL matrix below. Unprepared context cannot silently become
missing-sample output: admission or native guards preserve effective audio.
[C3 measurements](pcm-cache-measurements.md) report actual 200-pad startup,
process resources, I/O/CPU, fallback cost and lifecycle separately from logical
source/window byte counts. Hearing and devices
remain open until the final human-run acceptance stage.

Saved Automatic geometry remains only a hint: unsupported complete evidence
cannot establish CURRENT. The isolated productive probes and their invocation
are recorded in [development](development.md).

The productive 600-second PCM24 fixture restores the saved absolute 42.0–42.5
second loop as 24,000 stereo frames: 192,000 resident PCM bytes, while complete
source metadata remains 28,800,000 frames with source zero at frame zero. Cold
import and fresh warm restore both receive actual native ACK and render the same
bit-exact output against an independent raw-integer PCM24 oracle. Complete
decoder/playback digests still cover 115,200,000/230,400,000 bytes. Warm validation
verifies those full extents, then materializes only 192,000 playback bytes; the
complete cached PCM weak locator has no live backing after either finite load.
These are logical PCM/integrity byte proofs, not measured process RAM or startup
performance. The separate productive five-file stem probe checks complete-set
identity, finite component geometry and bit-exact active continuation across
native window ACK and cancellation; it does not evaluate separation quality.

<a id="initial-saved-finite-keylock-on--2026-10-12"></a>

## Initial saved finite KEYLOCK ON — 2026-10-12

Status: bounded software runtime, integrated, private Release/preflight, static
and independent final review gates pass. Exact publication is recorded in the
local delivery receipt.
The initial saved ON task is separate from the open whole-parent gates. Initial focused public attempts failed
before the productive output proof. The first had released canonical preparation
custody before acquiring the actual saved assignment, allowing the saved asset to
retire; unloaded-pad reset also reached scalar methods without a headless stream.
The fixture now acquires real saved-asset custody before releasing its preparation
lease and forwards those actual scalar kernels. The next bare restore-ID assertion and its
diagnostic rerun found both project paths cleared before Native admission despite
existing originals. A canonicalized extended-prefix Windows cwd differed from the
ordinary Win32 spelling returned by the public asset resolver, so Loader's lexical
`relative_to` rejected both paths. Using the existing ordinary Windows cwd helper
preserved real Loader parsing and progressed through restore admission/source ACK.
The next run failed a bare active-ID assertion after Play. The fixture now waits
for both pads' genuine readiness, delivers actual callback enum values through the
real feedback queue/`receive_msg` conversion and checks their public handlers with
rich state/event diagnostics. The corrected public wrapper passes all four histories
in 6.12 seconds and passes again in the larger integration. The combined readiness
and enum-conversion repair does not isolate which seam caused the last failure.
All four failed receipts remain retained.

`ResidentLoadHint::selection` is the single incoming verified physical selection
policy for cold selection, streaming preparation, fresh verified warm range reads
and prepared-material subscribers. A proper partial ON range receives
`KeyLockFiniteLoop`; a resolved complete `[0,N)` ON range receives
`KeyLockFullTrack`. Missing geometry retains complete demand, and malformed public
hints reject before request/cache/job mutation. Complete source identity, full
original/decoder/playback verification and EOF checks remain authoritative. Complete
and unknown demand retain actual complete live allocation and overlap admission.

The source-current and actual Load checks share one incoming ON geometry rule, so
an own source ACK cannot certify a source that Load rejects. Both finite contexts
initialize physical markers from that incoming view before a predecessor's markers
can affect admission. Genuine admitted finite ON may be reapplied idempotently;
an OFF `FiniteLoop` view still needs its existing guarded OFF-to-ON window/mode
transaction. Captured intent cancellation, fresh source generation, Play, timing,
Native coverage/history, queue and retirement guards remain in force.

The public fixture starts with a fresh Engine and captured ON before Load, using
real `LoaderController.restore_samples_from_project_state`, canonical saved-asset
custody, own native source ACK and ordinary public pad Play. Its four histories are
inherited global ON or explicit pad ON, each with public SPEED 0.73 or manual source
BPM 120 plus display target 87.6 deriving that same nonunity SPEED. All four describe
physical NormalLoop continuation at production k=0; the display setter is not a
BPM-lock admission or accepted-musical timing proof. The selected physical range is
`[17,1517)` of a verified 4096-frame stereo source, with complete duration/identity
retained and actual selected PCM captured before ACK. No OFF preload or after-load
mode flag supplies initial ON authority.

Required oracles compare every rendered sample against independent complete PCM16
integers, complete-PCM callback output, raw RubberBand Native/FIFO and uninterrupted
EQ. They retain an independent consumed-source clock, actual public Pause/Resume,
later own 4096-frame source-history preparation/adoption, same generation/finite
PCM identity, independent pad215 assignment/voice/clock and unchanged original
bytes/requested inheritance. Low-level cases retain cold/fresh-warm/prepared
selection, complete/unknown demand, late corruption/EOF, malformed hints,
stale/cancel/pressure and prior-reader controls. These are required current-source
gates, not substituted for runtime receipts.

| Current saved ON delivery gate | Receipt |
| --- | --- |
| Verified selection and incoming geometry regressions | PASS within whole cold-load (199 passed), cold-store (87 passed) and residency (68 passed) integrations |
| Four public restore/Play/Native histories | PASS: one public wrapper, all four histories, initially 6.12 seconds and final focused rerun 3.76 seconds; four initial failed receipts retained above |
| Impacted integration | Cold-load: 199 passed, four ignored, 681.48 seconds. Cold-store: 87 passed, 17.98 seconds. Residency: 68 passed, eight ignored, 72.64 seconds. Separate actual paired relocation: one passed, 0.32 seconds. Separate stem publication/mouse/sharing: three passed, 11.87 seconds |
| Build-only Release and private preflight/Python | PASS: fresh Release build, 23.92 seconds; exact private Native API/controller/UI import; six impacted Python suites, 285 passed and two skipped, 16.58 seconds. No installed Native replacement or App/device start |
| Independent final review and static checks | PASS: one independent whole-block review, no remaining actionable findings; official strict specification, JSON semantic preservation, local links/anchors and whitespace checks. Workspace/tests Clippy PASS with unchanged diagnostic inventory; Rust fmt PASS. Probe syntax and Ruff lint/format changes match the existing baseline without new diagnostics or suppressions |
| Coherent publication and clean main/remote equality | Exact reviewed commit and fresh clean main/remote equality are recorded in the local delivery receipt |

The first affected cold-load integration retains its actual FAIL receipt:
196 passed, three failed and four ignored in 897.29 seconds. Two adoption fixtures
offered fewer retirement slots than the unchanged callback envelope requires.
Their one-below/exact-bound checks now pass with the complete 16-case adoption
suite in 16.07 seconds. The real 216-pad paired-window case exhausted the bounded
reader registry: a verified pair registered its four component readers and the
worker then registered those same allocations generically. Paired custody already
holds all paired areas, source lease and actual readers; legacy unpaired windows
still need generic custody. The scoped correction preserves the 1024-record limit;
the unchanged real 216-pad test passes in 35.54 seconds with every both-endpoint,
unaffected-215 and usable-output assertion retained. The whole cold-load rerun
passes 199 tests with four ignored in 681.48 seconds, including that unchanged
216-pad case after preceding tests. The prior 295 records' live versus file-only classification was not
observed, so this receipt does not establish a global leaked-reader diagnosis.

The first whole residency integration retains its separate FAIL receipt: 64
passed, four failed and eight ignored in 95.20 seconds. A full-bank test had
requested a partial ON range while asserting complete residency; its actual
complete-range hint is now explicit. Three generation fixtures still waited for
legacy pending publication. At the unchanged ten-second deadline, the diagnostic
rerun observes a completed error-free worker, an available paired disk selection
and a current pending committed registration. The fixture must acknowledge that
actual registration and subsequent Native adoption; it cannot create a legacy
pending ticket or bypass disk generation. Its next integration retained 65 passed,
three failed and eight ignored in 71.18 seconds: the full-bank correction passed,
while generation reached registration and failed an old UI availability assertion.
`stems_available` reports verified disk metadata for controls. The corrected probe
requires that disk availability while separately checking pending own registration,
no resident receipt and no effective output. Actual Register/Capture/Worker/Adopt
and own ACK precede its accepted assertions; stopped residency still has effective
`waiting` until rendered output. All three stem tests pass in 11.87 seconds, then
the whole residency suite passes 68 tests with eight ignored in 72.64 seconds.
Legacy restore/stale-ticket, tamper, mouse, shared PCM and own-subscriber assertions
are preserved. Both integrations and the diagnostic failure remain retained.

The successful production build, cold-load/cold-store integrations and private
Native/Python checks are reused after these test-only corrections; their production
inputs are unchanged. The paired-reader correction is covered by the unchanged
ordered 216-pad test and the separate actual paired relocation test. Ignored long
resource/model helpers remain required at their respective final gates; these
bounded results do not claim those runs or process RAM, hearing or device acceptance.

The separate [ordinary incoming follow-up](#ordinary-incoming-loader--2026-10-13)
now implements preparation before live selection and actual initial BPM lock,
with a passing bounded software gate. Accepted-
musical admission, whole P4b/P5b, X11-LONG/MATERIAL, aggregate resources/music and
Human/device/hearing gates remain OPEN. Historical
2026-10-11 OFF storage measurements remain unchanged in the
[complete-source streaming report](long-source-streaming-validation.md).

<a id="ordinary-incoming-loader--2026-10-13"></a>

## Ordinary incoming Loader — 2026-10-13, bounded software gate PASS

Ordinary external import now uses a disk-only preparation seam before selecting
live PCM. Cold conversion and fresh-warm reuse retain the complete original,
decoder and playback artifacts with the existing schema, digest, exact EOF,
finite-sample and actual FileID checks. Dirty or corrupt candidates still take
the normal cold fallback. The activity scan reads verified playback in bounded
chunks and shares the finite any-channel strict `abs(sample) > 0.01`, preceding-
frame rule with the existing detector. Complete analysis uses its separate
existing admission and releases its temporary complete evidence buffer before
the selected live read. A complete disk artifact is never a hidden complete
live backing or permission to exceed live/overlap/analysis budgets.

Incoming defaults use the genuine activity/BPM result and Auto eight-bar intent,
or a newer demand addressed to that incoming request. The adopted physical end
is bounded by the verified complete source extent; Auto and eight bars remain
intent. Python's effective/requested projection applies that same physical
source-end bound. The shared verified selection policy admits partial finite
ON and OFF views, with genuine complete/ALL demand retaining its real budget.
No-BPM, no-activity and short-source cases retain their declared fallbacks.

One request/revision owner carries the existing global/pad-override resolution,
latest incoming edits and source/loop/scalar-period/grid-origin transaction.
Own callback ACK precedes the Python defaults projection. Receipt ordering keeps
the prior confirmed assignment and its settings separate until that ACK; a
matching replacement retires only its predecessor override. A source ACK proves
source/default installation. It does not prove wet output or later Native
history/window adoption: public first Play, actual rendered continuation and
later own 4096-frame history adoption have separate assertions.

Live loop, BPM, timing-origin, timing-intent, Key Lock, resident preparation,
normal/exclusive Play and Seek accept an optional expected source generation.
Captured controls require an actually current binding. Native checks that
generation under pad-request admission before intent notes, revocation or queue
publication and checks the actual source again at callback consumption. BPM
coalescing rejects stale entries before replacement and application; quantized
normal/exclusive triggers retain the guard until due execution, before stopping
other voices. Saved/unbound calls retain their explicit legacy `None` path.

Python projects the new assignment only after its matching Loader receipt, which
can follow Native source ACK. Until that projection, generic mode/loop/timing/
Start/Seek and Gain/EQ gestures reject before reading or mutating predecessor
Project state, even when the incoming Native binding is already current. MIDI
readiness/retry and atomic global start use the same receipt barrier. Explicit
request-addressed incoming edits remain valid; the incoming resolver alone owns
pending Global inheritance. This barrier grants no additional Gain/EQ Native
source-generation API contract.

The public Native wrapper executes 22 cold/warm first/replacement and negative
histories, including inherited/explicit ON/OFF, held latest demand, initial
actual BPM lock and nonunity SPEED, genuine source-bound admission and deferred
trigger rejection. Complete-PCM/raw-Native/FIFO/EQ/source-clock and independent
pad215 checks remain distinct from ACK/default metadata. The older saved ON
report above still uses manual source BPM plus a display target to derive SPEED;
its four histories are not relabelled as initial BPM-lock evidence.

| Current ordinary incoming gate | Software receipt |
| --- | --- |
| Public Native wrapper | PASS: one registered wrapper executes all 22 histories; initial pass 18.80 seconds, current integrated rerun 16.64 seconds. These are not 22 Rust test functions |
| Verified disk seam | Reused unchanged cold-store receipt: 95 passed; complete integrity, bounded activity and disk-only/selected-read separation remain covered |
| Private Release and API import | PASS: private Release build, 20.22 seconds, and full current API/controller/UI preflight (private Native identity prefix `e8395d91`); no installed Native replacement or App/device start |
| Python controller coverage | Current private-Release 11-file integration: 716 passed, two device cases excluded, 32.69 seconds. Earlier receipt-barrier profile: 714 passed/two excluded; separate focused control/Gain/EQ profile: 155 passed. These totals are separate profiles, not additive unique cases or a whole-Python claim |
| Current integrated Native results | Audio-stream: 63 passed with device coverage excluded; scheduler: nine passed; messages: 14 passed |
| Resident mixer and source coverage | PASS: resident mixer 16 cases/9.99 seconds; source-reader mixer 11 cases/6.88 seconds. First resident-mixer result remains 15 passed/one failed: its ownerless oracle omitted the actual complete-set owner. The independent fixture-owner expectation now strengthens identity without changing render/transition/retirement assertions; prior negative-oracle attempts remain retained |
| Remaining impacted Native integration | PASS with declared reuse: basic mixer 97; constant-timing parent 111/four explicit ignored gates; resident relocation 12. Cold residency covers 68 cases using its unchanged 66 PASS and three-case adapter rerun, including both failures, with eight explicit ignored gates. Cold load covers 200 cases using its unchanged 191 PASS and all nine failed-case reruns, with four explicit ignored gates; this is not a fresh 200-case parent run |
| Static and official strict gates | PASS: production Python Ruff/format/Mypy, Rustfmt, both affected strict changes and current workspace/all-targets Clippy exit zero with warnings. The included legacy Python probe retains its unchanged HEAD Ruff/format/Mypy baseline; its three-line forwarding change passes actual Native execution |
| Independent final review | PASS: one independent read-only review of the actual production, specification, test and final report deltas, including runtime/static receipts, closed with no remaining actionable finding. The four ordinary incoming child tasks are accepted at this bounded software scope |

The Native callback/history proofs run in the Release Rust test binary with
hardware-free test-channel injection into the actual public Engine and callback
kernels. The production private wheel supplies the independent API/import
preflight; the Python suites exercise controller flow with their declared test
doubles. Neither profile certifies a production device callback or Human hearing.

The two resident-mixer negative attempts exposed omitted complete-set identity
in the expected selection and distinct fixture owners in the exact transition
comparison. One complete set now supplies independent complete/finite readers;
all exact output, transition and retirement checks remain, with added identity
retirement. Cold-residency and eight cold-load failures exposed test adapters
missing the new keyword or actual incoming API; faithful forwarding and actual
queued defaults/matching receipts replace obsolete post-ACK defaults expectations.
One unchanged saved-ON case timed out in the integrated run, then passed its
focused four-history gate twice in 3.68 and 3.72 seconds. Its first timeout cause
remains unresolved; no assertion, timeout or production behavior was changed to
make it pass. These earlier outcomes remain part of the evidence.

The initial public attempt retained four successful cold histories before an
invalid warm-seed fixture stopped the run. The corrected seed uses actual
copy-first disk preparation, commit and sealed lease; the later 22-history PASS
does not erase that failed attempt. These bounded receipts close no whole
P4b/P5b, X11-LONG/MATERIAL, E11-03, accepted-musical or aggregate all216 resource
gate. Human App/device/visual/hearing and final program gates remain open, with
the existing stop before full Rust-port planning/implementation or Slice8.

## Public complete-source Play and unchanged ALL return — 2026-10-13

The public Loader/Transport first-Play subset passes eight actual histories:
cold/warm, first assignment/replacement and OFF/ON. The public waveform ALL action
requests the real complete `[0,N)` window; own source/default projection, resident
ACK, guarded launch and actual START feedback remain separate. Before source ACK
and before the Python Loader receipt is consumed, the assignment barrier prevents
derived Project reads, mutations and Native calls. The first unassigned Play is
an unchanged no-op; a replacement with a confirmed predecessor is rejected.

This exposed a production classification error: a genuine complete ON window
was retagged `KeyLockFiniteLoop` after its resident ACK. Incoming selection and
public loop/storage/mode preparation now share the existing physical ON policy:
exact `0..N` is `KeyLockFullTrack`, otherwise `KeyLockFiniteLoop`. Geometry,
rounding, byte/overlap admission, OFF hint interpretation and finite Seek's
captured prefix/loop/tail lowering remain unchanged. Classification adds no
callback work, allocation, I/O or new ownership mechanism.

The Release Rust test binary passes the four OFF histories in 2.71 seconds and
the four ON histories in 2.91 seconds. It runs actual headless public Engine and
Python controllers with the callback kernels; it is not a device or installed
wheel callback test. Independent complete PCM, Raw Native/FIFO, continuous EQ,
full-source wrap, exact source phase, own later 4096-frame Native adoption and
unrelated Pad 215 checks pass. ON obtains its own armed receipt before launch,
then reports waiting/dry until its first independently determined Native block
has warmed; actual chosen wet output settles its own readiness.

Earlier negative attempts remain recorded: Windows path-prefix projection failed
before product execution; the initial ON test reproduced the wrong complete
context; the first corrected-context render exposed an untracked reference's
incorrect immediate-wet startup expectation. Only the child reference gained its
own tracked request and independent startup boundary. Exact audio tolerances,
parent incoming histories and production startup behavior were preserved.

The separate actual scheduled-renderer subset passes 24 histories in 4.06 seconds
OFF and 4.16 seconds ON. Four current-authority ordinary/exclusive starts produce
their actual due START, including ordered exclusive STOP215 then START0. Twenty
stale starts cover replacement source, a newer complete intent, a newer finite
window, ticket launch cancellation and the public pad-launch revision fence.
They reach due through real rendering across an in-buffer boundary without
recapturing source/window/launch authority, creating Pad0 or stopping Pad215.
Independent complete PCM/phase and Native/FIFO checks accompany valid starts;
stale starts retain their actual replacement state and unrelated voice chronology.
The initial stale test incorrectly expected a no-BPM default's end to be `N`;
it now retains `None` while the separate explicit complete ticket still requires
`0..N`. This child-only correction leaves Native behavior unchanged.

The complete unchanged committed StemSet subset passes OFF in 1.53 seconds and
ON in 1.72 seconds. Actual public Transport starts the complete `0..4096` source;
actual callback START/STOP objects feed the normal Python handlers. ALL STEMS,
FULL MIX and fresh ALL retain the saved disk selection and registration, while
each action obtains its distinct current residency/effect receipt. Pause partway
through the 128-consumed-source-frame component ramp freezes its progression,
source/FIFO/Native/filter history and both voices until public Resume.

Independent complete component allocations prevent the reference from pinning
actual readers. FULL MIX withdraws the bank components; ON's actual ALL Native/
history owners survive that withdrawal and retire only after own later FullMix
Native adoption. After real final-reader release, fresh ALL keeps the unchanged
complete-set identity but obtains new component allocations and its own effect
confirmation. Independent complete PCM/Raw Native/FIFO/EQ, nonzero ALL versus
FullMix output, later 4096-frame preparation after Resume, fractional full-source
wrap and Pad215 chronology checks remain exact.

A final bounded public loop excursion acknowledges `17..N`, then `17..1517`,
then `0..N`, checking actual bytes/context/current window, voice phase and retained
Native/FIFO guards at each ACK. This excursion does not claim finite rendered
effect; the existing finite continuation suites retain that separate obligation.
The test's initial enum adapter created a base object instead of the actual START
variant. It now uses the same variant conversion as public return values, with a
strong type guard. The OFF oracle also inspects every bounded actual scratch
chunk while retaining the caller's complete output partition and continuous EQ.
Both earlier fixture failures remain recorded; no product startup, guard or
audio tolerance was changed for either correction.

Affected integrated Release test-binary checks pass 113 Rust cases: control
workers 43, resident mixer/parity 16, finite context proofs 4, Native source
coverage 9, relocation unit admission 7, four selected hint/saved/warm/budget
cases, five ordinary/public wrappers covering 54 histories, committed-set parent
23 and two finite component continuation/retirement cases. The committed parent
excludes its unchanged L3 controller/Native and offline-generation siblings;
this is not a fresh complete cold-load, all216 or whole-workspace run.

Fresh locked private production Release passes in 23.07 seconds, followed by
Native API/controller/UI import preflight without App/device start or installation.
The unchanged 716-case Python controller test-double result/two device exclusions
from the preceding ordinary delivery is reused, not relabelled as a new binary
test run. Clippy workspace/all-targets exits zero in 13.54 seconds with the exact
unchanged warning-message/file counters: 149 literal warnings, 77 message kinds;
warnings remain. Rust formatting and official strict validation pass. The
installed Human extension's length and exact UTC timestamp remain unchanged.

The one independent final source/evidence review passes with no unresolved
actionable findings. The normative first-Play scenario preserves the effect-free
unassigned no-op and the confirmed predecessor assignment barrier explicitly.
These bounded software checks close no whole P4b/P5b/E11,
accepted-musical, all216 resources/music or Human/device/hearing acceptance.
Full Rust-port planning/implementation and Slice8 remain out of scope.

## Public active KEY LOCK ON-to-OFF — 2026-10-13

Four bounded Release Rust test-binary histories pass in 6.52 seconds: FullMix
and selected ALL STEMS, each running or paused. The tested voice is born through
the actual public Transport and emits its own nonneutral-rate Native output
before the public pad controller requests OFF. Actual Python controllers and
headless Engine/callback kernels form this profile; it is not a production
wheel callback, CPAL/device or hearing test.

Requested OFF remains separate from prior effective wet truth while the own
resident command waits for the collecting callback sink's retirement admission.
The matching source/window/request ACK settles dry OFF without START, STOP,
retrigger, voice-generation, source-phase or loop changes. EQ history continues.
The actual Native FIFOs and productive wet history clear; all current wet-ready
flags are false. The reusable Native handle and historical adopted request may
remain, so neither is treated as current wet readiness.

Independent complete PCM and an uninterrupted EQ oracle match every dry output
sample at rate 0.75, across partitions `[1,31,137,512,777]` and at least three
fractional complete-source wraps. Pausing only Pad0 freezes its consumed source
and EQ history. Native/FIFO state stays fixed within each paused phase, with own
OFF ACK performing the declared retirement/reset. Public Resume continues
source/EQ chronology.
Unrelated running Pad215 keeps its own generation, source phase/rate, mode,
Native/FIFO and source identity. Saved assignment, disk selection and registration
remain current, and OFF starts no new stem-generation worker.

Same-range OFF deliberately shares admitted immutable PCM and selected ALL
components. The old ON `ResidentSourceView` differs from the own OFF view and
reaches zero actual strong readers after the collecting callback sink is cleared,
control reconciliation and real Native preparation-worker retirement. Current
PCM remains owned. ALL components stay retained through dry use; a subsequent
own public FULL MIX receipt and consumed-source-frame ramp releases their bank
ownership. Their actual component Weaks reach zero after final-reader retirement,
before common STOP cleanup. This proves those readers, not production retirement
queue saturation, every owner family or disk-generation deletion.

The existing two public complete-set histories retain their chronology through
a test-only optional follow-up seam. All modified Rust is reachable only through
`cfg(test)`; production Python/Rust/build inputs are unchanged. The preceding
private production Release/API preflight and 716-case Python controller
test-double profile with two device exclusions retain their declared scopes and
are reused, not relabelled as new runs.

Affected integrated Release test-binary checks pass 86 cases: control workers
43 in 52.12 seconds, resident mixer/parity 16 in 9.83 seconds and committed-set
parent 27 in 35.32 seconds, including all four OFF cases and the unchanged two
public complete histories. Unchanged L3 controller/Native and offline-generation
siblings are excluded; this is not a fresh whole-parent/workspace run. Clippy
workspace/all-targets exits zero in 8.45 seconds with exact unchanged diagnostic
message/file counters (149 warnings, 77 kinds); warnings remain. Rustfmt and
official strict validation pass. The one independent final source/evidence
review passes with no unresolved actionable findings.
Broader global/inheritance/override, finite/candidate-domain, accepted-musical,
resource216, E11-01/P4b/P5a/P5b and Human/program gates remain open. Ordinary
Loader B replacement preserves A until its own assignment ACK and then stops A;
the separate native non-replacing frozen-A proof does not change that contract.
Full Rust-port planning/implementation and Slice8 remain out of scope.

## Complete access and acknowledged controls (C2b)

`complete_context::CompleteSourceReader` captures the immutable complete playback
descriptor, current source generation/rate and sealed committed lease. A finite
playback window remains the alignment/identity reference, never the complete
analysis input. Waveform projection and offline mono export stream fixed chunks
from the retained sealed reader; source zero, full end, binary64 X coordinates
and exact sample zoom remain absolute. The lease stays owned through the actual
visitor/read return even after cancellation or unload. These reads do not seek
audio or retain a hidden complete playback allocation.

Wider waveform views share the existing two-worker/32-reservation lane. There is
one latest projected result per pad, at most 16384 columns, 48-KiB reader scratch
and no complete
PCM result cache. View supersession rejects older results. The editor shows
pending/error and continues polling the same requested view. A failed identical
view stays terminal until explicit Retry or a new view/source. Normal complete
analysis uses that bounded lane with the existing complete conversion/analyzer
admission under 1 GiB per job. The optional diagnostic export/key route retains
its separate one-job/512-MiB staging policy and explicit analyzer/rate identities.
Neither route changes the analyzer default or promotes historical timing.

`prepare_resident_control` prepares source-bound loop, seek and Key Lock intent
under one native transaction. It admits queue and PCM capacity before replacing
pending ownership. Full mix and an already accepted identical complete StemSet
are prepared together; callback guards recheck complete source, actual window,
request/source epoch, timing authority and latest intent. The old effective
handles, loop and processing state remain valid until actual native ACK. The
callback reserves retirement/feedback capacity before claiming and changing
state. Preparation, large owners and file readers retire off-thread.

Pure KEYLOCK changes keep that transaction and the current physical extent,
without manufacturing a loop edit. The supported k=0 finite NormalLoop path
admits actual current/paused/retained voices against their own source, timing,
fractional feed and selected-set ramp coverage. Source cursor, loop, voice
generation and EQ remain continuous; mode-specific Native/FIFO retirement stays
on the existing ownership path. Own WindowWork ACK and source-bound mode
application/readiness feedback are separate. Active ON waits for usable current
wet processing, while later exact source-candidate adoption remains independently
guarded. Full-source scalar commands also carry their own source/generation and
mode-request ACK rather than settling from a successful enqueue. See
[live mode feedback](key-lock-backend.md#live-mode-transactions-and-feedback).

When a start requests the same immutable source, exact resident range and DSP
context, the transaction reuses the existing full-mix PCM and matching accepted
StemSet handles at the same window revision. It consumes no cold-worker slot and
does not reload, align or hash stem files. It still queues the native transaction
and waits for its actual callback ACK before guarded launch. Equal window
revisions are admitted only with identical full-mix/component PCM ownership;
source, request, authority, accepted timing and latest intent guards remain in
force. A different required range or DSP context uses bounded preparation before
adoption, and an old source's ticket cannot authorize a replacement source.

For finite current-source NormalLoop KEYLOCK, scalar stem mode/mask changes use
the already ACKed same resident committed set. Inactive verified pair publication
keeps FullMix selected; subsequent playback and native scalar commands provide the
real Some/Some route. Admission and feed include target and active outgoing
selection, all four real component ranges, accepted timing, source/window revisions
and canonical tap/rate trajectory. Empty masks still require the actual admitted
set. No hidden complete backing or default target selection replaces a missing
outgoing input. The existing 128-source-frame ramp preserves cursor/loop/timing,
wet native/FIFOs and chronological filter while new preparation is deferred; its
later settled-selection prepare/adoption remains independently guarded.
`SetStemPairFullMix` retires the resident components after their final ramp use,
with history/jobs retaining their own readers until existing off-thread retirement.
Ordinary controller FullMix/AllStems warm return and active initial None->Some
are covered separately by the [typed P5a transaction](#active-same-set-residency-p5a-2026-10-11);
permanent retained scalar fixtures cannot close that workflow.

Normal finite loop edits use the exact proved physical tap interval. ALL admits
complete playback. Nonresident seeks admit complete context so physical intro
and tail continue to the existing loop/full-source boundary. Source-end clamping
retains the exclusive full frame endpoint; the next read wraps. A paused seek
stays paused and a stopped seek does not start or request complete PCM. A voice
pinned to a prior source retains its own source extent, timing and frozen
stem selection/transition. A preallocated one-slot handoff captures that actual
voice before the worker acquires its older sealed source lease. A nonresident
seek prepares that source and the identical old complete StemSet, then rechecks
voice generation, window and timing at ACK. It replaces only that voice's reader;
the new bank's window, stem set, selection and timing remain intact. Key Lock uses
the labelled admitted full-track exception while finite continuation is unproved.
Same-source storage relocation retains actual native/FIFO/filter/rate history;
a real seek or discontinuous loop clamp preserves the existing invalidation rules.

A seek whose actual pinned reader already covers the proved target and future
loop read set is acknowledged directly by the bounded capture command; it keeps
the same PCM Arc and performs no complete-source read or copy. A covered seek
still resets the existing discontinuity history and obeys cancellation/retirement
capacity before adoption.

The Python `ResidencyController` is the shared UI/MIDI/control readiness path.
Durable project fields record requested intent; `effective_region` retains the
previous acknowledged region while preparing. Error or cancellation restores
only that still-current intent, never a newer assignment or timing authority.
Per-pad latest tickets have eight admission/launch attempts and a 30-second
deadline; a claimed predecessor is observed before a newer attempt. A claimed
native tail cannot be rolled back. A deadline exposes unconfirmed ownership and
revokes its waiting launch until actual completion is observed. Freshness requires actual source generation,
authority, unique latest intent and adopted window, rather than ACK alone.

Repeated starts for the same pending source owner and requested loop/Key Lock
context retain that preparation and replace only its waiting launch action with
the latest gesture's original input timestamp. They do not cancel/restart the
work, supersede its ticket or reset its retry count/deadline. Changed owner or
intent uses the existing replacement path; STOP still revokes the waiting launch.
Admission of a replacement complete StemSet retires older resident start intents
under the same native source/producer/ownership fence. A subsequent click waits
through pending stem publication and obtains a fresh matching transaction; an
older ACK cannot authorize it or restore the replaced component owner. Mode/mask
edits do not replace the complete set or retire this readiness.

Prepared UI starts retain their original input timestamp. MIDI fallback uses
the same preparation and then refreshes its native current-source guard before
launch. `play_resident_control` carries the acknowledged ticket's opaque source,
window and timing binding through the existing queued/scheduled trigger. Execution
rechecks that binding and latest intent before an exclusive start can stop other
pads. A MIDI runtime refresh alone cannot invalidate a prepared UI launch.
GLOBAL START retains one complete source-bound batch until pending
regions settle and retries batch queue pressure within eight attempts/30 seconds.
A newer global gesture replaces the earlier timestamp. STOP cancels pending
launches even before a voice becomes active. A bounded per-pad stop revision
fences queued and scheduled UI/MIDI triggers and global START entries, separately
from complete source/window authority. Native STOP admission advances it only
after queue capacity is reserved; later starts capture the fresh revision.
The controller retains one admitted launch ticket per pad for exact cancellation;
its launch flag cannot revoke a newer ticket or undo an acknowledged window.
GLOBAL STOP includes pending START targets whose claimed callback tail may not
have reached UI telemetry yet, then queues STOP behind that tail. Fixed native
admitted-start records also supply direct MIDI targets before input or active
feedback is observed. STOP captures these targets and revokes their launch
revisions under the same producer admission mutex used by UI/MIDI/global
starts, so an earlier independent scan cannot miss a claimed start.
Cancellation alone retains targets until ordered STOP admission. STOP metadata
may cover all 216 pad assignments while START retains the 32-voice limit and
existing retirement reservations. Shutdown disables mapped MIDI before launch
revocation and Python/native worker joins. A previously captured mapping rechecks
that enabled state under producer admission; the saved input preference remains
unchanged. This mutex is used only in the control path. Native shutdown closes
cold-lane admission and drains queued ownership before cancellation releases
active workers, so they cannot start a queued job on their way to the join.
GLOBAL STOP preserves its existing current-source binding guards: an old pinned
voice whose source differs from the current bank still rejects the whole batch.
The existing single-pad STOP remains available for that pinned voice.
Unload/shutdown cancels continuations. Failed post-ACK launch admission reports
an error while keeping the acknowledged loop/source; transient queue pressure
has bounded retries. New complete stem generation/adoption remains inactive-only.

After direct bank replacement, a loop preparation with unchanged Key Lock may
acknowledge the new bank while a prior-source voice keeps its frozen loop and
stem view. The guarded retrigger switches to the new bank and retires the old
voice off-thread. A changed Key Lock request with that unmatched finite voice
reports unavailable rather than changing its unprepared processing context.

Hardware-free proofs exercise actual worker admission, native command drain,
ACK, complete/window/source identity, independent full-buffer PCM and finite
output, lifecycle and off-thread retirement. Accepted musical P and integer H
remain separate, with the existing <=1-loaded-frame 75/1000-cycle gate. Resource
limits and logical resident bytes remain distinct from the
[C3 process-RAM and startup measurements](pcm-cache-measurements.md).
Actual human/device acceptance remains separate and open.

## Historical preliminary C1b integrity measurements

The isolated 600-second, 48-kHz PCM24 mono source contains 86,400,690 bytes
(SHA256 `96ffe98cf44215719b0b57d605d6dc586c9c4e763ad3d47d512d0ba787d204ef`).
After an actual cold import/ACK and engine shutdown, a fresh engine restores the
saved relative original through the productive warm lane and actual native-bank
ACK. An independent streaming integer PCM24 oracle checks every complete
115,200,000-byte decoder and 230,400,000-byte stereo playback digest against
both cache files and actual adopted native PCM. This is headless source restore;
it does not establish app Automatic timing restore or device acceptance.

Warm capture reads and writes 86,400,690 bytes into its exclusive snapshot and
rereads those bytes for snapshot verification. Cache validation reads the full
115,200,000 decoder and 230,400,000 playback bytes, then materializes playback
with another 230,400,000-byte read. Restoring copies zero additional durable
original bytes. Manifest bytes are recorded separately. These are counted logical
payload extents, excluding container/header selection and filesystem metadata;
they are not physical disk traffic or peak RAM. The OS file cache is warm after
cold import and was not flushed. No disk-cold label or 200-pad benefit is implied.

| Native profile | Warm validation wall / process CPU (s) | Capture through actual ACK wall / process CPU (s) |
| --- | --- | --- |
| Debug | 16.600 / 16.594 | 19.207 / 19.844 |
| Release | 0.437 / 0.422 | 0.596 / 0.547 |

Save/export uses the existing real accepted-QM verifier and actual Python atomic
`ProjectPersistence.flush`, with a 256,003-frame/8-kHz source (32.000375 seconds,
512,050 bytes; SHA256
`7a961e08a6d7735e7c0993f77af6596b7d5a790b44333d3d9970eee611fda21b`).
Each successful verification performs two complete source hash reads, totaling
1,024,100 bytes, plus full loaded-mono conversion, 44.1-kHz resampling and PCM/
backend digest CPU work. It performs no PCM disk read. Three exports and three
saves per profile produced the medians below; JSON/config extents remain in local
probe evidence. Same-size source corruption rejects native export and leaves the
previous config byte-exact and the project dirty after a failed save.

| Native profile | Export median wall / process CPU (s) | Atomic save median wall / process CPU (s) |
| --- | --- | --- |
| Debug | 0.713 / 0.719 | 0.724 / 0.719 |
| Release | 0.013 / 0.016 | 0.017 / 0.016 |

These historical preliminary integrity costs used one long warm run per profile
and three small save/export repetitions. They preserve full integrity and existing
timing authority. Current 200-pad cold/warm, resource, lifecycle and save results
are recorded separately in [C3 measurements](pcm-cache-measurements.md).
Human hearing/device gates remain open.

## Audited C0 baseline (historical)

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
(`key_lock_preparation.rs:471-503`). Current setup/RAM measurements are recorded
in [C3 measurements](pcm-cache-measurements.md); historical 64-handle observations
retain their original scope.

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
2. Obtain a source read protocol that excludes mutation through lookup and copy
   (on Windows, a retained handle with tested write/delete-sharing exclusion).
   A mutable source that cannot be captured stably fails safely. Before/after
   stat or path hashes do not prove stability. Hash the bytes successfully
   read completely; handle short reads/writes, errors and cancellation. A hidden
   full-SHA claim serializes canonical selection before the derivative gate.
3. Retain the sealed incoming handle as the preparation snapshot. A compatible
   hit independently verifies and retains the sealed canonical original and
   creates no `snapshot.original` or additional original leaf. A genuine miss
   copies the exact original once, flushes/reopens/verifies it and publishes its
   binding after durability. Restore creates neither a snapshot copy nor another
   durable original. A read-only filename flag does not enforce this custody.
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
full validation decide reuse. Different names with identical bytes share the
first canonical original/PCM while retaining independent pad assignments.
Conflicting candidate content,
partial entries, old versions, format/device differences and corrupt manifests
regenerate off-thread; they never overwrite an entry held by a reader.

## Integrity policy and bounded workers

Every fresh source/cache lease hashes the complete project original and complete
required PCM files, checks manifest identity, dimensions, checked expected byte
length and exact file EOF. The lease protects exactly the verified immutable
objects from writes/replacement. Existing verified in-process leases can be
shared across consumers; reopening a mutable path requires verification again.
This deliberately incurs O(full source + required PCM bytes) warm I/O. An index
or size/mtime shortcut cannot weaken it.
[C3 measurements](pcm-cache-measurements.md) report that verification cost.

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
| Seek before/after resident loop | Non-KEY or complete-backed Seek retains the admitted complete exception. A held finite KEYLOCK source prepares prefix plus loop for Intro or loop plus actual source end for Tail through effective-voice capture and its own ACK. Preserve the old effective playhead until ACK; no target-only halo or continuous callback streaming. |
| ALL | Requested manual [0,full duration), auto disabled; prepare full-track exception unless already resident. Failure retains prior effective audio. |
| Return to finite loop | Explicit finite replacement prepared against actual current cursor/history and both selection sides; continuous next taps preserve adopted Native/FIFO/filter history, while clamps or changed interpolation seams reserve clean Native state before cutting it. Release full exception after final voice/job/history reader retires. Never evict an active trajectory's required samples. |
| Stems/masks | Full mix plus the four component windows use matching source/window revisions, range coverage and current accepted projection. Current-source NormalLoop scalar commands admit already ACKed same-set transitions only when both actual active sides are covered, including the outgoing stem side and empty masks. A finite window relocation may adopt while active only for the identical complete source and already accepted complete StemSet, with proved history and ACK; it cannot introduce generated audio or new timing evidence. Preserve shared cursor/taps/ramp/filter/native-FIFO history. Initial active None->Some and ordinary controller warm return require separate [typed P5a residency/effective receipts](#active-same-set-residency-p5a-2026-10-11). Instrumental is not a fifth live component. |

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
holding a newer canonical set. Any container removal requires all generations,
readers and unknown content to be gone under the same cleanup/admission gate;
C1b only attempts nonrecursive final empty-container removal under those checks.
Crash recovery removes recognized unreferenced staging safely; it cannot treat
unknown files, project config, originals or private audio as garbage.

## Bounded implementation and evidence plan

- **C1a, delivered:** bounded cold-load job admission, immutable copy-first
  input, full decoder/playback artifact writer and manifest with complete actual
  digests, exclusive atomic cold commit, request-guarded all-or-none publication
  and snapshot/staging cancellation/queue-failure rollback. Retain current
  full-buffer playback. Resolve/prove decoder boundary
  policy and reuse the stronger resampler dimension/tail rules. Integrate cold
  path with existing guards; do not implement resident windows.
- **C1b, delivered:** complete validated warm reuse extending guarded cold publication, shared
  digest/assignment leases, containment, cancellation/unload/shutdown and eventual
  last-owner cleanup. Measure integrity bytes/CPU preliminarily, without startup
  improvement claims. Preserve current accepted save/export behavior.
- **C2a:** complete-source authority versus resident descriptor/address translation,
  proved context and saved-loop startup including matched stems. Rebind CURRENT/
  MIDI/history consumers; eliminate hidden full playback pins for normal loops.
- **C2b:** finite readiness across controls, full editor/analysis access and explicit
  nonresident seek/ALL/full-DSP exceptions, real source/output/history parity and
  <=1-loaded-frame loop gates. No streaming framework.
- **C3, measured:** actual cold/warm readiness, lifecycle and resource results on
  200 occupied pads, with paired finite/full outcomes, regressions and measurement
  limits in [C3 measurements](pcm-cache-measurements.md).

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

[C3 measurements](pcm-cache-measurements.md) bind identical original hashes,
durations, saved loops, processing versions and pad assignments for paired
finite/current-full comparisons. The 200 occupied pads cover shared, duplicate and
unique long sources plus explicit full-track/Key Lock/editor/analysis/seek/ALL
exceptions. Occupied pads remain distinct from simultaneous voices; active-pad
renders and bounded stress retain the voice/native-handle limits.

Artifact-cold means no compatible committed PCM; fresh-process warm means validated
complete caches. OS page cache was uncontrolled. Retained PCM, declared per-operation
checkpoints, process lifetime peaks and sampled observations have separate scopes;
exact simultaneous aggregate transient PCM remains unmeasured. Readiness, resource,
verification/save and cancellation/cleanup outcomes include signed regressions.
Numerical/native parity does not prove human hearing or real devices; those
gates remain open for final pre-port acceptance. No part of C0-C3 starts planning
or implementing the full application Rust port.

## Active same-set residency (P5a, 2026-10-11)

The typed [committed-set APIs](prepared-stem-publication.md#native-api-and-ownership)
separate verified selected disk eligibility, current four-component residency and
effective audio mode. Registration alone cannot activate components. A fresh
callback capture binds each request to actual source/window/timing/STOP/voice/history
and rate target; bounded workers read the held immutable pair off realtime.
Callback adoption rechecks those owners and retirement capacity before effects.
The current production-k=0 NormalLoop voice, cursor, FullMix backing, Native/FIFOs
and EQ remain continuous through first None->Some and the real 128-source-frame ramp.

Own residency ACK does not grant effective-mode ACK: KEY LOCK requires actual
output from its source-specific Native owner. Pause freezes progress until resume.
Known Native preparation failure rejects only the unproved effective receipt while
accepted components and existing audio owners remain intact. Selection cancellation
revokes future ALL without releasing actual readers; source-bound FULL MIX still
withdraws outgoing components. New descriptor registration has its own revocation
owner even for equal verified logical content.

Stopped residency remains waiting for effective audio. A later ordinary ready
Play or other superseding context can make an accepted receipt historical: the
helper withdraws it to unconfirmed while retaining registration and saved intent.
It clears the withdrawn queued intent and never obtains a new live ACK by polling;
an explicit mode gesture captures fresh authority. Known actual Native preparation
failure remains rejected with its reason even after later context changes. Pending
and rejected transactions retain strict errors and all adoption guards. A stopped
FULL MIX callback release clears actual component control owners separately from
continuing-output proof, and ready Play keeps its existing admission path.

The genuine Python StemController test executes ALL->FULL MIX->ALL after actual
outgoing/history reader retirement, plus rejected-helper recovery through ordered
legacy release and a second disk-only preparation. Both warm returns need fresh
residency/effective receipts; helper-removal recovery additionally needs a fresh
callback registration receipt. Saved mode colors remain intent;
the sidebar reports pending/rejected/effective audio separately. Queue failure
preserves explicit retry, including repeated FULL MIX release admission failure.

Before the stopped-status correction, 19 focused Debug cases pass in 24.95s;
the final affected Release run includes all 21 P5a cases. Independent
complete-PCM and algebraic raw-Native/EQ comparisons retain exact output, cursor,
FIFOs, bounded taps and nonzero signal checks under irregular partitions, actual
rate smoothing, masks including zero, interruption and pause/resume. Negative
cases cover fresh generation/old voices, stale source/window/history/voice,
cancellation, queues, retirement backpressure, positioned-read failure and a
genuinely full existing reader registry. The per-reader failure seam and lower
per-pad PCM cap are test-only: the latter exercises the same held+new+scratch
calculation and owner-preserving error path, not real 1-GiB allocation, RSS or P6/V0.

Initial Native failures are retained in the local test log. The real aligner exposed
an unrelated-original/component-sum fixture; the corrected PCM16 original is the
actual four-component sum, with verified zero alignment and nonzero aligned loop
PCM. Signal thresholds, exact references and the bounded continuation are retained.
The retrigger race uses a legitimately pre-admitted launch; empty masks still
observe addressed physical taps while their selected feed is zero.

Broader Release integration reports 1083 passes, 7 failures and 19 ignored cases
in 912.21s, with one actual-device test filtered. Six failures are old scalar
message-count expectations in public worker/retry/publication bridges; the seventh
incorrectly requires unused FullMix Native history to pin four component crops.
Their actual typed receipts and downstream ownership/audio proofs are corrected,
with no capability hiding or guard waiver. The affected Release pair run reports
93 passes and one later source-retirement wait failure in 90.46s. That fixture now
waits independently for the actual old source owner and components; its Release
rerun passes in 0.68s. Three publication cases also pass in Release in 8.23s.
These 97 affected cases close the original seven failure maps; unchanged positive
results from the broader run are reused without relabelling that historical FAIL.
Helper phase and stopped-test cleanup failures are retained in the local test log.

The final Release wheel build passes in 20.77s; fresh scratch Native import, both
typed APIs/ticket classes/failure reason and release start-check pass without
installation or App start. All 688 impacted Python controller/UI cases pass in
59.05s against this corrected Native runtime and current Python source. The
installed Human Release extension remains unchanged. Cargo workspace check,
Rust/Python formatting, Ruff/Mypy, official affected strict, relative links and
whitespace checks pass. The one independent source/evidence review found and
verified the stopped-status correction and actual final-reader wait.
Full P4b/P5b, new-generation LIVE-STEMS, nonzero KEY/B5,
aggregate resources, musical/device/listening/Human and whole-program gates remain
open, including the actual H16384 negative of 1598 frames / 33.292ms.

## R0 shared material and content boundary

The shared-material boundary reuses cold_store/project_assets/asset_lifecycle/stem_cache;
canonical `samples/<Originalfilename>/` originals, `.pcm-cache` and `stems` replace
physical per-pad duplicates. No second cache/hardlink/reparse/phantom origin.
P1a supplies canonical typed material roots and durable per-content IDs. P1b extends
the existing central ownership registry: each normal cold/warm admission creates its
own CacheAssignment ID while sharing verified immutable backing; cloning a lease
continues to represent the same assignment. All-bank saved stem assignments release
one owner individually, and physical retirement starts only after the last saved user.

One registered StemGenerationJob can serve compatible same-source/processing
interests in different slots. Each interest captures its own content instance and
native PreparedSourceTicket and reserves its own cleanup capacity before admission.
Removing the origin detaches only that interest; the backend source/private-generation
leases survive until actual read completion, and cancellation occurs after the final
interest. One ready generation is promoted before individual native publications.
Source/content/timing/queue rejection affects that subscriber alone.

Current MATERIAL compatible completed-set lookup additionally verifies the
supported actual producer: Demucs37 or BS33 complete files within Native's bounded
64-file guard, comparison of loaded host code/bindings with held source, exact
captured child package roots and a fresh empty bytecode namespace. Actual tools
retain their image/FileID and exclusive child-returned birth/body witness requirements.
Unknown historical or unsupported execution stays UNKNOWN. The dependency-version
and narrow OS runtime policy does not certify neural execution, a hostile/global
runtime or resource/music acceptance; the MATERIAL report owns the scoped proof.

ProjectAssets retains weak references to the five aligned PCM allocations and sealed
artifact handles under its existing bounded reader gate. A compatible subscriber
reuses actual retained PCM, then binds its own current source/window, publication
permit and accepted timing projection. It receives its own native ACK. Equal hashes
and paths supply no publication authority. Different ranges/processing/version do
not reuse incompatible PCM, and the registry never keeps complete PCM alive itself.
Sealed artifact handles are released off-thread after the last actual component
reader. A pending regeneration holds a reserved old owner; rejection reclaims that
exact existing stem lease under the same gate and captured directory identity,
without another capacity-dependent owner allocation. Missing, replaced or reparse
directories fail closed. No callback file handles or I/O are introduced.

The earlier R0 storage/control foundation does not itself prove CopySnapshot/GUI
operations, mass migration, persistent aligned stem-f32 caches or later resource/preload gates;
current MATERIAL/storage evidence is tracked separately at its actual bounded scope.
Copy adds equal immutable refs and fresh stopped content/native ACK, not analysis/
decode/separation/files. New versions keep old users; current/old voices, jobs,
subscribers, actions/holds/history/native unload ACK participate in true last use.
Move/Swap carries exact DSP/history/cohort; all36 operations preflight full capacity,
acquire refs first and never free unconfirmed native claims. Origin bank deletion
and new-process restore require actual evidence; full finite216/P6/V0 proofs remain.


## Public GLOBAL with independently audible voices — 2026-10-13

This bounded software block verifies the actual Python Transport GLOBAL/Pad
routes through the current PyO3 engine, WindowWork and scheduled callback for
several simultaneous finite NormalLoop voices. Existing stopped all-bank,
override/failure and single-voice public GLOBAL tests remain their own genuine
Native evidence. The 716 Python controller tests remain separate test-double
evidence; they do not establish this concurrent audio path.

The new child `public_global_voice_tests.rs` reuses the existing real Native-load
and Project-fixture Harness. Device startup alone is replaced by its existing
test control/parameter producer seams; ordinary Python Loader birth is not part
of this fixture. Pads 0/36 inherit, 72 is explicit ON and 215 explicit OFF. Each
starts through public Transport at a staggered time, at volume1 and rate.73,
with its own phase in physical32..224 at48kHz. Nonneutral EQ setup enters the real
typed parameter queue; this is not a new public-EQ gesture proof.

Each pad's reference owns separately allocated original PCM, an algebraic consumed-source
clock, raw Native adapter and EQ state. Every actual one-frame scheduled render
checks each running voice's chosen pre-EQ dry/warming/wet buffers, its post-EQ
pad peak and the audible filtered sum. Independent first-block dry guarding and
4096-frame candidate feeding determine each own Native chronology, FIFO,
productive history, physical phase and current-source readiness. Preparation is
awaited after its first actual feed, without advancing callback time. Own source
and window/request receipts, launch generation and geometry remain distinct from
wet success; numeric request IDs need not be globally unique.

The successful history starts inherited OFF, enables GLOBAL ON while pad36 is
paused, resumes it publicly, proves all own wet outputs/adoptions, then applies
GLOBAL OFF across running0/paused36 and resumes dry36. Paused ON is truthfully
armed/effective/ready before its first live wet output. Source and EQ stay fixed
through its own mode ACK; Native/FIFO remain fixed within the paused phases,
while own OFF legitimately clears mode state. Explicit72 retains wet processing
and explicit215 retains dry processing throughout. No START/STOP/retrigger is
introduced by GLOBAL, and all running contributions remain audible.

The partial-failure history exhausts only pad36's actual warmed reserve after
the real pre-ACK capacity gate. Its original generation/source/window and own
failed request remain authoritative; dry source/EQ audio continues with requested
inherited ON and truthful error. Pad0's queued global request is superseded by
public local OFF then clear-override; the stale ticket loses authority and its
fresh inherited request independently reaches own wet output. Explicit peers'
separate lanes continue successfully. This is target-local reserve admission
exhaustion. The initially proposed worker crash was rejected before execution:
its worker_failed Arc is shared by every mixer lane, so it could not certify an
isolated target failure. Production failure scope and lane wiring are unchanged.

Outgoing inherited ON ResidentSourceView Weak owners reach zero after actual OFF,
receipt-alias release, production retirement FIFO completion and real relocation
reconciliation while callbacks and preparation workers remain alive. Each current
immutable PCM allocation remains directly shared by its own bank and publicly started
voice. All six measured cross-pad PCM pointer pairs were FALSE in both histories.
The conditional cross-pad shared-owner branch was therefore not exercised;
shared backing used by another pad remains a separate open coverage case. No
shared cross-pad allocation, disk deletion or every-owner proof is inferred.

Validation:

| Check | Result and profile |
|---|---|
| Focused public GLOBAL histories | 2 pass, 1.69s, after Release TEST compile1m14; headless actual PyO3/controller/callback kernels |
| Public controller parent | 13 pass, 6.19s, includes the same two new cases |
| Native mode chronology | 9 pass, 18.56s, existing independent physical/raw-Native/FIFO/EQ workers |
| Production buffer retirement | 2 pass, 0.00s, this module's focused cases |
| Final Clippy workspace/all-targets | exit0, 7.56s; unchanged149 diagnostic warnings/77 message kinds, lib-test62 including26 duplicates; warnings remain |

The 24 integrated cases include the new two rather than adding another two to
the count. The first compile failed only on an invalid test import; no audio
ran in that attempt. The oracle now normalizes its bounded fixture EQ independently.
Initial Clippy passed with one additional tuple-complexity warning. A descriptive
`PreviousOnOwners` alias removes it without suppression or runtime changes;
final all-targets Clippy compiles that source. The successful audio histories are
reused across this type-annotation-only change, not relabelled as a fresh Release
run. Logs are in workspace/scratch/goal-public-global-20261013.

Only the new cfg(test,windows)-reachable child and its parent registration alter
Rust inputs; production Rust/Python/build inputs remain unchanged. The earlier
7b6c7fd private production Release23.07s/API preflight and Native SHA
`a9a9329dd36bf9c3e173a1cf0a3d8ef5476c6e9d43574d348e8c2671bc817bab`, plus
716 Python controller-double checks with two device cases excluded, are explicitly
reused. The Human extension is not installed or started by this block.

Official affected strict validation, whole Rust formatting, document/link/JSON
consistency and actual final diff checks pass. The one independent final
source/evidence review passes with no open findings in this bounded result.
These one-frame finite k=0 representatives do not establish fresh irregular
callback, complete-source/ALL GLOBAL, cross-pad shared backing, accepted-musical,
aggregate216-resource, device/listening/Human or whole E11/P4b/P5a/P5b/Goal gates.


## Public complete restore sharing and final PCM readers — 2026-10-13

One actual public saved-project Restore history supplies the complete physical
0..N sharing case left unexercised by the preceding finite GLOBAL fixture. The
canonical original is restored into fresh pads0 inherited,36 explicit ON and215
explicit OFF. The preparation PCM Weak is zero before the sole public
`Loader.restore_samples_from_project_state` call. All three actual bank and
publicly started voice PCM pointers are unconditionally equal; no fixture owner
or conditional branch supplies this result.

Existing digest/device preparation serializes the same original. `ColdLoad::prepare`
holds its preparation gate through `transaction.bind_pcm`; complete PCM Weak and
sealed cache ownership register before the gate releases, and the returned sample/
lease remain strong while that producer awaits its own Native ACK. Distinct fresh
pad histories permit followers' complete warm loads to upgrade that same Arc.
Finite warm selections intentionally read their own ranges. A second whole
Restore would deliberately copy a pinned same-pad reassignment, so that proposed
setup was rejected before execution. No private restore subset, dummy PCM seed,
new API or public Copy birth is used.

The stopped setup observes each real LoadColdSample generation/adoption0→2 by
relaying its unchanged actual head message through a one-item ring into the
unmodified production control drain, at the same frame with the same scheduler,
transport, mixer, feedback and retirement owners. Original-ring retirement
capacity0 first proves the own source remains unclaimed. FIFO order remains;
this observational partition does not claim normal production callback batches
adopt exactly one source. Original normal drains handle all later playback,
GLOBAL and unload. Only the initially empty control/parameter test queues grow
to carry restored scalar defaults. The existing real feedback ring/Rx and real
Loader success/Transport handlers remain authoritative. Stream-bound gain/EQ/
loop/timing scalar kernels use the established saved-restore bridge to actual
queues, including current source-generation guards.

Each voice starts publicly at a staggered phase, volume1 and rate.73 over4096
physical frames at48kHz. Its separately allocated reference owns complete original
PCM, an algebraic consumed-source clock, raw Native/FIFO and EQ state. Every actual
one-frame scheduled callback checks each pre-EQ dry/warming/wet feed, post-EQ
pad peak and audible sum; own independently fed4096-frame candidates, FIFO,
productive/filter history, source trajectory, Native address and own source/
window/request receipts remain separate from shared immutable samples.

Saved OFF0..N hints initially retain the existing FiniteLoop classifier even
though their actual extent and shared allocation are complete. After coupled
ordinary public START acknowledgements, bank0/215 are observed FullTrack and36
KeyLockFullTrack. GLOBAL ON then OFF affects only inherited0 through its own
pending/ACK and actual Native/dry output; its post-OFF context is observed
FullTrack. Explicit36 continues its own wet processing and215 its own dry output,
without retrigger, phase/FIFO/filter resets or origin changes.

Old0 ON mode-view Weak reaches zero through actual production retirement FIFO,
relocation reconciliation and Native worker retirement while36/215 audibly retain
the same PCM. Public Loader unload0 retains both peers; unload36 retains dry215;
unload215 obtains its own ordered STOP/unload ACK and actual final shared PCM Weak
zero. Preparation, receipt and independent reference aliases have ended at their
declared checkpoints. Engine, cold, Native preparation and retirement workers
remain alive at final Weak-zero. Disk deletion is not asserted.

Validation:

| Check | Result and profile |
|---|---|
| Focused shared restore history | 1 pass, 1.09s, after Release TEST compile1m22; actual public controller/PyO3/callback kernels |
| Public key-lock parent | 14 pass, 6.96s, includes this case and both preceding finite GLOBAL histories |
| Existing public saved ON restore/Play | 1 wrapper pass, 3.86s, preserves its existing own Native chronology scope |
| Full-content warm sharing at device rates | 1 pass, 1.61s, retained complete integrity/Arc/lease checks |
| Productive warm subscriber/cancel/final readers | 1 pass, 0.32s, actual Native adoption and cleanup |
| Clippy workspace/all-targets | exit0, 9.36s; unchanged149 diagnostics/77 message texts/120 message-file pairs, lib-test62 including26 duplicates; warnings remain |

The 17 integrated Rust cases include the new one. No compile or runtime failure
occurred in this block. Pre-build source corrections preserve the rejected second
Restore proposal and the actual classifier/ACK-partition anchors. The only
existing helper changes parameterize its source range and split complete versus
finite prepared-tap observations; the prior finite32..224 bounds, real taps and
strict assertions remain unchanged and pass again. Complete prepared workers
report no finite tap observation; independent full PCM/raw-Native output remains
required. Logs are in workspace/scratch/goal-public-global-20261013, with
public-shared-restore-first.log and shared-restore-* integration/Clippy files.

Production Rust/Python/build inputs remain unchanged under the existing
cfg(all(test,windows)) parent. Earlier private production Release/API Native SHA
`a9a9329dd36bf9c3e173a1cf0a3d8ef5476c6e9d43574d348e8c2671bc817bab` and716
Python controller-double cases/two device exclusions are explicitly reused.
Affected official strict, whole Rust formatting and final document/link/JSON/diff
checks pass. The one independent final source/evidence review passes with no
open findings in this bounded disposition. Public Copy/ReArrange R1/R2, selected-ALL GLOBAL, finite
cross-pad allocation reuse, accepted-musical/irregular-callback/resource216,
disk-delete/device/listening/Human and whole E11/P4b/P5a/P5b/Goal gates remain open.

## Scoped runtime admission and lazy preload policy — 2026-10-13

The P5b runtime uses one engine/project resident account inside the existing
asset registry. Actual live resident-view witnesses determine which unique PCM
backings belong to that account. A second engine adopting shared PCM pays its
first scoped cost; ending the first engine's real view witnesses ends its charge
even while the second engine still retains the same physical backing. Duplicate
views in one account charge the backing once. Pending reservations include
prospective allocation and ownership; publication transfers them to real view
witnesses under the asset gate. These values measure PCM, not process RSS.

The persisted settings are strict `preload_stem_loops_on_startup=false` and
`resident_pcm_budget_mib=512`, with an accepted integer range of128..16384MiB.
Native receives the budget before the engine starts and before restore. Lowering
the budget preserves live pins and reports above-budget occupancy; future
admission waits or reports a typed budget error. Ordinary timing/analysis
preparation remains separately bounded at512MiB, and paired preparation retains
its existing separate1GiB bound. Existing2-worker/32-job lanes are reused.

Incoming unknown geometry and callback-captured window/seek/stem demand start as
bounded proposals. Exact byte/owner/reader capacity is reserved and fences are
revalidated before live request/source/window/intent authority is armed. The
callback captures and adopts bounded prepared payloads; it does not access the
accounting mutex, files, Python or new job scheduling. Admission failure leaves
the previous playable source and intent intact; no epoch rollback is used.

With preload OFF, saved ALL remains durable desire while restore admits only
required FullMix PCM and verifies available committed pairs without components.
An explicit ALL request still needs its own residency and effective-mode receipt.
Preload ON adds prepare/retain-only policy interest to eligible pads without
changing their audio mode. Disabling policy removes that interest while explicit
interests and actual readers retain their authority. Preferred-ALL generation
uses its ordinary fenced live transaction; FullMix with preload OFF remains
disk-only. A single bounded216-entry demand owner feeds eight initial/deferred
items, alternates direct/policy work and rotates recoverable blocked entries.
UI counts current own ACKs and displays exact PCM counters separately from a
conservative demand estimate, unknown geometry and process overhead.

Scoped validation:

| Check | Result and scope |
|---|---|
| Native runtime regressions | 80 distinct passing cases across the recorded implementation/repair profiles: accounting5, proposals3, ColdJobs6, source-error/analysis2, new productive P5b5 and59 affected cases; overlapping reruns are counted once |
| Actual216 stopped pads | 32.46s; supported128MiB budget, actual source/selection/window/policy receipts, independently deduplicated PCM costs, typed pressure and actual retirement FIFO/last-reader/account zero |
| Seek, registry and public Generate repairs | Existing covered/current/previous finite-source checks, reader-pressure old-audio assertions and public generation with actual own Register command pass; all original admission/adoption guards remain |
| Private production Release | Fresh optimized build35.15s, wheel extracted only into workspace scratch; Release/API/Portable/controller/UI import preflight passes without Engine/App/device construction |
| Impacted Python controllers/UI/Portable | 492 distinct cases pass,2 device cases skipped; initial12-file run484 pass/8 fail/2 skip112.82s, then both repaired fixture files66 pass3.17s; unchanged passes are reused, not a fresh whole-pytest result |
| Clippy workspace/all-targets | exit0,18.05s;146 diagnostics/77 message texts/117 message-file pairs, no new pairs against149/77/120 baseline; warnings remain |
| Static checks | whole Rust formatting, Ruff281 files, impacted Mypy9 source files, official strict validation and diff whitespace pass |

The two renewal rounds reuse each pad's actual compatible selection identity,
but obtain fresh window/policy receipts. The fixture initially registers216
sets and does not forge repeated registration acknowledgements. The PCM oracle
counts actual independently observed backings; it does not subtract the initial
56-byte owner retained by the collecting retirement fixture. That owner now
ends through the real retirement FIFO and worker before its charge is expected
to end. The216 case uses stopped pads and does not claim216 audible voices,
full process-memory peaks, cold/warm process speed or musical acceptance.

All negative compile/runtime results remain in
`workspace/scratch/goal-p5b-runtime-20261013`. Repairs include the post-arm source
epoch fence, independent preload authority and exact queue/busy retry codecs;
fixture repairs retain the actual current preparation lease, idempotent native
selection semantics and scoped transfer of test observers into existing cold
workers. No guard, tolerance, deadline or production worker count was weakened.
Final cfg-test/argument-bundle/lexical-scope cleanup is behavior preserving;
the80 successful Native runtime cases are explicitly reused after that cleanup,
with fresh all-target compilation, private Release and Python integration.
Private Native SHA256 is
`805909f54148e2ec90f1794095626ea62da2dc71691fb9df640f39f54c7d2516`.
The Human's installed Native and running App/settings remain unchanged.

The Python negatives exposed historical Loader fixtures that still targeted
the prior ordinary admission route. Their full timing, cancellation and scalar
publication assertions now exercise the actual maintenance/restore route with
its exact Native arguments asserted. Ordinary failures use the actual incoming
API; three previously ineffective failure cases now assert actual admission
and the reported error. Existing incoming-default/receipt/old-feedback tests
remain intact. The metadata queue fixture now forwards its declared components
flag into preparation, retaining the complete all-False retry oracle.

The one independent final review passes with no open findings for this scoped
source and receipt profile. Four bounded implementation tasks close.
The full P5b/P6 resource, musical and Human parents remain open;
control-scheduler216 fixtures alone do not establish Native residency,216
voices or a process-memory gate.
