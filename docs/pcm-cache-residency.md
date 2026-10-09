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

## Delivered cold path (C1a)

The pending [pad-owned PCM program](pad-owned-pcm-program.md) extends this delivered
foundation. It replaces obsolete legacy physical containers with canonical immutable material
versions and equal all-bank content users, with #1..#216 only stable membership,
adds persistent aligned stem PCM/direct range reads, and requires actual216-slot
residency plus finite DSP proof. Existing measured200-pad evidence and labelled
full-track KEYLOCK fallback do not complete those new requirements. SourceVersion
path migration needs verified lineage and fresh native source/timing ACK; existing
cache/manifests/readers are not edited in place. This document's delivered path
descriptions remain current until those serial slices implement the new targets.

`cold_jobs.rs` admits at most two active workers and 32 queued/reserved jobs.
The queue bound includes reservations made before request mutation. Each active
job admits at most 1 GiB of transient PCM, including decoder packet/workspace,
Vec growth overlap, retained decoder PCM, FFT conversion/channel mapping, and
Vec-to-Arc overlap. Decoder workspace admits conservative supported-codec packet
maxima; ALAC cookie block length/rate/channels are checked before constructor
allocation, with actual codec configuration SHA256 recorded. Automatic analyzer inputs have a separate conservative
admission calculation within that same job budget after decoder PCM is released.
Thus the cold lane's admitted transient PCM is at most 2 GiB, excluding previous
resident pad/voice PCM and old-source request checkpoints (including pins retained
after unload), existing native preparation pools and non-PCM analyzer
model/DSP storage. These are limits, not measured process peak RAM. Default
analyzer behavior is retained; opaque native analysis kernels finish their bounded
input before a subsequent cancellation check. Other stages check cancellation
between packets/chunks and before commit/publication.

`cold_store.rs` opens actual files with Windows FILE_SHARE_READ exclusion of
writes/deletion, verifies opened leaf/ancestor reparse attributes, copies/hashes
64-KiB chunks into an exclusive snapshot, verifies exact digest/EOF, and retains
the same sealed reader throughout `sample_loader/cold.rs` decode. External
replacement or A-to-B-to-A after capture cannot change decoder input. Inputs with
an existing incompatible writer fail safely. This tested stable-capture protocol
is Windows-only; other platforms fail Unsupported until an equivalent protocol
is proved. Imports retain a byte-exact collision-safe original; restoration
captures a contained existing original without another durable original.

Each cold attempt creates complete `decoder.f32le`, `playback.f32le` and
`manifest.json` under `samples/.pcm-cache/v1/.staging-<pid>-<generation>/`.
Canonical versioned descriptors bind actual original digest/bytes, decoder
container/codec/library/options/packet error policy, full decoder interleaved and
arithmetic-channel-mean-f64-to-f32 digests, actual rate/channels/full frames/zero,
and the full playback parent identity, own digests/dimensions and executed
resampler/channel transform. No window or historical accepted flag supplies
these identities. Original decoder rate/layout is retained independently of the
selected playback/device rate, with no forced 48-kHz conversion.

Files are flushed, the exclusive complete directory is renamed on the same
filesystem to `<full-identity>-<pid>-<generation>`, and every committed file is
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
fixed history. C2a labels unproved finite native continuation as
`full-track-key-lock-continuation-unproved-v1`; saved Key Lock admits complete
PCM instead of guessing source margins from the 4096-output-frame worker horizon.
This fallback is a finite allocation, not a streaming pipeline.

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

When a start requests the same immutable source, exact resident range and DSP
context, the transaction reuses the existing full-mix PCM and matching accepted
StemSet handles at the same window revision. It consumes no cold-worker slot and
does not reload, align or hash stem files. It still queues the native transaction
and waits for its actual callback ACK before guarded launch. Equal window
revisions are admitted only with identical full-mix/component PCM ownership;
source, request, authority, accepted timing and latest intent guards remain in
force. A different required range or DSP context uses bounded preparation before
adoption, and an old source's ticket cannot authorize a replacement source.

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

## R0 shared material and content boundary

The pending target reuses cold_store/project_assets/asset_lifecycle/stem_cache;
canonical `samples/materials/M<id>/original`, `.pcm-cache` and `stems` replace
physical per-pad duplicates. No second cache/hardlink/reparse/phantom origin.
Current last-assignment acquire-before-release and native reader registry are
foundations; current pad-bound stem capture/restore/cleanup still need P1a/b changes.
Copy adds equal immutable refs and fresh stopped content/native ACK, not analysis/
decode/separation/files. New versions keep old users; current/old voices, jobs,
subscribers, actions/holds/history/native unload ACK participate in true last use.
Move/Swap carries exact DSP/history/cohort; all36 operations preflight full capacity,
acquire refs first and never free unconfirmed native claims. Origin bank deletion
and new-process restore require actual evidence; full finite216/P6/V0 proofs remain.
