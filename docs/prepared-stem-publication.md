# Source-bound prepared stem publication

G3b1 binds the productive Demucs/BS-RoFormer/restored-stem path to the actual immutable loaded
source. G3b2e also binds preparation and adoption to its current native timing
authority and complete acknowledged accepted projection.

The bounded Settings selection is captured in each separator request. Both
adapters produce the same five aligned artifacts through a streaming writer;
neither controls promotion, retirement or native availability. Selecting another
model does not invalidate a complete current set. Restoration never imports or
requires a separator model, even when the selected model is absent.

## Native API and ownership

The pending [pad-owned PCM program](pad-owned-pcm-program.md) retains these guards
and adds an explicit residency-only first None->Some adoption while FullMix plays.
Current generic publication rejects active pads; current relocation requires an
already accepted identical StemSet. Those APIs do not yet implement lazy first
activation. Future active residency must preserve source/ticket/voice/geometry/
history/DSP/lease guards and effective-mode feedback, without permitting active
generation/different-content replacement or restarting playback. Complete durable
five-artifact integrity remains separate from four live component-window readiness.

`capture_prepared_source(id, source_version)` returns an opaque
`PreparedSourceTicket`. The version contains normalized original path and
`|sha256-v1:<digest>`. Its digest must match the original-file content retained
by the native loader; same-size/mtime replacement before admission is rejected.
The productive loader copies/hashes stable bytes first and decodes that sealed
snapshot, or validates complete compatible warm PCM on retained immutable readers.
Its byte-exact project original remains separate from the shared digest cache.
The [PCM cache](pcm-cache-residency.md) records complete decoder/playback lineage;
the prepared ticket still requires its own current source/timing permit.

Capture serializes with real load/unload publication under the request mutex and
pins the actual sample Arc, request, loaded rate and engine-specific preparation
epoch. New load intent, unload and successful pad BPM/origin edits invalidate
pending work, including equal-valued edits. Offline analysis admission and
cancellation also advance the shared request and preparation epoch together under
that mutex, so already queued stems cannot adopt a retired request. Epoch/source
request exhaustion fails without wrapping. Full queues preserve prior state. The epoch invalidates before
the successfully reserved timing command becomes visible. This is a freshness
counter, not the provenance-rich accepted timing revision.

Capture also resolves actual native source and declared timing authority through
the existing current-pad resolver, which verifies source generation and content
digest. The permit keeps a fixed InputPadBinding with source address, shape,
loaded rate, authority revision and, for Automatic authority, the current
acknowledged complete accepted revision, exact binary64 period and independent
signed origin. Separate generation/digest fields are not stored in that callback
binding: the pinned actual sample, matching source_version digest and checked request/
preparation-epoch/monotonic-authority owners preserve source ownership after
capture. Automatic without a consistent current acknowledgement cannot admit
preparation. Manual, Tap and Legacy retain
their own authority; their numerical values do not become accepted evidence.
Historical constant-timing ticket metadata, equal endpoints, raw revisions,
source hashes or the preparation epoch cannot substitute for current acceptance.

Offline cancellation keeps its existing no-return teardown API: it marks the job
cancelled even if counter exhaustion rejects the shared identity advance. Such a
rejected advance preserves both counters; cancelled analysis still cannot publish
a result. Private checked transitions report the exhaustion error. A cancelled job
whose request was already superseded does not advance the newer owner's counters.

`publish_prepared_stems(id, source_version, cache_dir, source_ticket)` requires the
unchanged admission ticket. It checks source, request and current timing binding
before preparing, decodes/aligns outside the GIL, then rechecks after preparation
and under the request mutex through enqueue. A ticket enqueues once; a full queue
leaves it unconsumed. Old or foreign owners fail. Pending or rejected accepted
replacement does not promote its proposed timing: the previous acknowledged
projection remains current, while existing request/epoch guards still retire
superseded preparation jobs.

The prepared set retains the actual alignment-reference Arc, atomic permit and
fixed timing binding. Mixer adoption rechecks actual source, current authority,
full accepted revision, period and signed origin. A source or timing change after
enqueue rejects the pending set without replacing previously admitted audio.
Rejected sets and source pins use existing off-callback retirement.

Control authority and effective mixer timing can briefly differ during an admitted
Legacy/Manual/Tap edit: control revokes the accepted acknowledgement before the
parameter callback applies its clear. A fresh nonaccepted ticket can therefore
capture valid current control authority yet be rejected by callback adoption while
the mixer still has the old accepted projection. This preserves retained PCM and
audio; a new capture after the mixer clear can succeed. Preparation feedback does
not promote the historical ticket during that transition.

Already admitted stems are immutable source-frame PCM, so successful native
accepted adoption or clearing for that same source refreshes only their fixed
effective timing projection. It does not decode or align again, allocate a second
cursor, reset fractional progression or change physical loop endpoints. Pending,
failed and rejected adoption leave that effective projection unchanged. Rendering
checks the retained actual source and exact effective accepted projection against
the mixer's effective projection, including that brief control/callback transition;
shape, rate and equal numerical values alone are insufficient. Full mix and every
stem read both interpolation taps at the same
SourcePlayback position, rate/ramp, loop/seek policy and source-selection transition.
This shared feed reaches Key Lock with G3b2f1 continuous native/FIFO history bound
to the actual source and complete effective timing. G3b2f2 productive worker
preparation pins the same admitted stems and copies the same canonical trajectory;
its source-specific native/FIFO adoption rechecks their actual buffer identities
and effective timing. Active selection ramps defer preparation until complete.

`ticket.publication_status()` reports `captured`, `pending`, `accepted` or
`rejected` through bounded atomics. Python leaves controls unavailable while queued
and enables them only after actual mixer acceptance. Late rejection reports an
error and preserves prior mixer state (full mix when no stems were previously
accepted). Polling observes adoption without driving
audio timing. Mode/mask commands check native loaded identity without rehashing
long original files on each performance click.

## Artifacts and restoration

C2a distinguishes complete source/set identity from resident PCM. Saved finite
loops retain full duration/rate/evidence and absolute window offsets. A fresh
ticket fences the current resident allocation/revision as well as the complete
source and timing authority. Stem admission temporarily reads a bounded complete
source, aligns/hashes the complete stem set, and then retains only matching finite
fullmix/component views. It does not align complete stems against a cropped
reference or hide complete PCM behind an accepted ticket.

Changed storage uses the same bounded source preparation lane and native ACK.
An unchanged start reuses the exact immutable fullmix/component PCM handles and
window revision, with matching complete-set identity and acknowledged timing
projection. It requires the native transaction's callback ACK and guarded launch,
but neither cold-worker admission nor complete stem reload/alignment/hashing.
Pending stem publication and mismatched source/set/window remain unavailable.
Repeated identical pending starts keep that preparation and retain only the
latest gesture's original timestamp for launch; STOP can revoke that launch.
Replacement complete-set admission atomically registers its pending owner and
retires older resident start intents before enqueue becomes visible. A new click
uses bounded publication retries and fresh ACK, rather than coalescing a retired
ticket. Queue-full rejection preserves the previous owner and start authority.
Active adoption requires the identical complete source and already accepted
complete set token/content, matching fullmix/components and coverage of the actual
voice trajectory. Source fraction, rate/ramp, filter and native/FIFO state stay
coherent; old job/voice allocations remain pinned until their final reader retires.
Introducing a new complete set or source generation remains inactive-only.
C2b source-bound control preparation changes matching fullmix/stem windows and
loop/seek/Key Lock effect under one native ACK. Complete source/set identity,
timing and history still govern adoption; pending/error/cancellation leaves
previous effective audio available. Complete waveform/analysis readers and
nonresident seeks use bounded complete-source exceptions, documented in
[PCM cache and readiness](pcm-cache-residency.md).

Each separator writes `samples/stems/#<pad>/.generation-<uuid>/`. Events retain the
original ticket identity, so an old completion cannot consume a new job even after
reloading identical bytes. Obsolete workers discard only their checked private
directory and cannot overwrite current canonical artifacts.

Current completion validates a complete private set, writes `.complete.json`
last and publishes it as an immutable `.ready-<uuid>` generation inside the pad
container. Metadata points to that exact generation. The `stem-set-sha256-v1`
marker binds source version and all five WAV digests. Missing, partial, mixed or
tampered sets remain unavailable. The marker and current permit remain separate
from the full-mix PCM cache.

Native artifact preparation accepts the exact published
`samples/stems/#<pad>/.ready-<uuid>` path as well as direct legacy cache sets.
Published names use the lifecycle owner's pad/generation checks and a 32-character
lowercase hexadecimal UUID. Private `.generation-<uuid>` directories and arbitrary
nested paths are not publication inputs. The contained asset lease still rejects
traversal, links and Windows reparse points before native preparation reads files.
Source identity comes from the content marker and current native ticket, rather
than from the cache directory name.

Assignment and separator-job tokens protect original and generation paths under
the native admission/cleanup gate. Native PCM readers cover queued, bank, voice
and history owners until off-thread retirement. Unload or Delete Stems revokes
eligibility immediately, then retires exact owned paths after the last reader.
Legacy canonical sets retire only their declared files; unknown content and newer
generations survive. Windows sharing failures are retried off-thread. Shutdown
drains jobs and closes runtime tokens while preserving persisted assignments.

Restore verifies source/marker, waits for matching full-mix load and captures a
fresh ticket. Stat-only legacy stems invalidate and need regeneration. Manual,
TAP and saved legacy timing numbers keep existing behavior and are not promoted
to accepted records. The native placeholder separator event path cannot publish
without admission; the exported legacy deterministic generator fails closed so
it cannot overwrite canonical files during preparation. Its artifact writer is
retained only in Rust test fixtures. Content integrity and declared association do not
certify musical correctness of arbitrary separator outputs. No audible SYNC or
device acceptance is claimed.

The hardware-free regression runs the productive Python stem controller and
generation events, native preparation/enqueue, command drain, adoption feedback
and PCM renderer against a deterministic offline artifact backend:

```powershell
.\scripts\run-rust-tests.ps1 -CargoArgs @('stem_publication_tests')
```

Its injected command producer uses the same native publication core as the
stream-backed API. The mouse regression also drives real ImGui press/hold/release
frames through `UiContext`, input mapping, playback and residency controllers,
then the native transaction ACK, guarded command drain and PCM renderer. It
compares full-mix and accepted-stem starts while the cold lane is deterministically
occupied, preserving the original Rust input timestamp. This is a control-path
regression proof, with no acoustic-delay or general throughput claim.
It opens no CPAL stream and measures no model quality,
inference performance or human/device acceptance.

## Remaining G3 boundaries

G3b2a/b/c/d connects [explicit acceptance](native-constant-timing.md) to current
native/Python grid, loop, transport/master/rate and MIDI pad consumers. G3b2e
adds productive prepared-source/stem revision binding with a shared source
trajectory; the generic preparation epoch remains a separate freshness check.
G3b2f1 binds continuous productive StretchProcessor/voice/filter history and pinned
source timing. G3b2f2 adds actual worker-owned native/FIFO continuation from those
PCM/stem owners, exact current permits and timed transactional adoption; completion
requires productive ownership/output/failure proof. Neutral reserves and the
separate test-only key_lock_source_preparation cannot substitute for that owner.
G3b2g adds supported complete source-verified SampleAnalysis/ProjectState persistence
and fresh acknowledged loader adoption. G3b2h binds productive controller GLOBAL
START/STOP batches including MIDI to current native source/authority and complete
acknowledged accepted timing. G3b2i adds general explicit publication and guarded
derived loop/master completion, shared with fresh saved adoption before restored
stem intent completion in the application.
Immutable loader lineage does not replace the current prepared-source permit
or promote historical timing. G3c separately proves musical versus physical
loops over 75/1000 cycles, fractional periods/rates, callback partitions/wrap and
rendered/onset/device/listening gates. Numerical ownership tests supply no human
listening, label or device acceptance. Later B5 audible crop/delay/transition
compensation stays separate; current 96-native-handle setup/RAM and C3 multi-pad
save costs remain unmeasured. Preliminary complete native export/project-save
integrity accounting is recorded in [PCM cache and residency](pcm-cache-residency.md).

## Pending R0 origin independent versions

R0 revises the physical target to canonical immutable materials with equal content
users. Current cache_dir_matches_sample_id/restore and native project_stem_cache_dir/
project_assets validation are still slot-container-bound; P1a/b must reconcile
writer/capture/reader/restore/retirement consistently. Material/StemSet identity
alone cannot supply a copied current SourceTicket/history permit/ACK. Copy gets
fresh independent content authority, Move/Swap needs a proven bounded native remap
without resetting voices/FIFO/filter/history; raw historical evidence is unchanged.
Jobs keep remaining subscriber interests and retain physical leases until actual
read end. All-bank assignments/old voices/actions/version/native unload ACK determine
cleanup, not origin-to-copy hierarchy. Complete five-WAV/five-f32 joint integrity
and four-live-component semantics are preserved. The current runtime above is
unchanged; these are pending program/R2/R4/P6/V0 and HC-01..HC-26 obligations.
