# Source-bound prepared stem publication

G3b1 binds the productive Demucs/restored-stem path to the actual immutable loaded
source. G3b2e also binds preparation and adoption to its current native timing
authority and complete acknowledged accepted projection.

## Native API and ownership

`capture_prepared_source(id, source_version)` returns an opaque
`PreparedSourceTicket`. The version contains normalized original path and
`|sha256-v1:<digest>`. Its digest must match the original-file content retained
by the native loader; same-size/mtime replacement before admission is rejected.
The loader hashes before decoding and checks the byte-exact project copy against
that digest. This observes persistent changes, not an immutable decode snapshot:
the complete copy-first/ABA provenance proof remains the separate PCM-cache work.

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
This shared feed reaches Key Lock without giving its productive DSP history a
source/accepted revision binding.

`ticket.publication_status()` reports `captured`, `pending`, `accepted` or
`rejected` through bounded atomics. Python leaves controls unavailable while queued
and enables them only after actual mixer acceptance. Late rejection reports an
error and preserves prior mixer state (full mix when no stems were previously
accepted). Polling observes adoption without driving
audio timing. Mode/mask commands check native loaded identity without rehashing
long original files on each performance click.

## Artifacts and restoration

Each separator writes `samples/stems/#<pad>/.generation-<uuid>/`. Events retain the
original ticket identity, so an old completion cannot consume a new job even after
reloading identical bytes. Obsolete workers discard only their checked private
directory and cannot overwrite current canonical artifacts.

Current completion validates a complete private set, removes canonical
`.complete.json` before replacing any WAV, and atomically publishes the marker
last. The `stem-set-sha256-v1` marker binds source version and all five WAV digests.
Missing, partial, mixed or tampered sets remain unavailable. This bounded stem
protocol is separate from the future full PCM disk cache.

Restore verifies source/marker, waits for matching full-mix load and captures a
fresh ticket. Stat-only legacy stems invalidate and need regeneration. Manual,
TAP and saved legacy timing numbers keep existing behavior and are not promoted
to accepted records. The native placeholder separator event path cannot publish
without admission; the exported legacy deterministic generator fails closed so
it cannot overwrite canonical files during preparation. Its artifact writer is
retained only in Rust test fixtures. Content integrity and declared association do not
certify musical correctness of arbitrary separator outputs. No audible SYNC or
device acceptance is claimed.

## Remaining G3 boundaries

G3b2a/b/c/d connects [explicit acceptance](native-constant-timing.md) to current
native/Python grid, loop, transport/master/rate and MIDI pad consumers. G3b2e
adds productive prepared-source/stem revision binding with a shared source
trajectory; the generic preparation epoch remains a separate freshness check.
Productive StretchProcessor/voice/DSP-history binding remains pending: the warmed
Key Lock pool is source-neutral and key_lock_source_preparation is test-only.
Accepted source-verified SampleAnalysis/ProjectState persistence and loader schema
with fresh runtime adoption, source/accepted-bound controller global START/STOP
batch launch including MIDI, and explicit acceptance/derived loop/master refresh
orchestration remain later gates. The original hash association does not complete
C1 immutable copy-first/ABA proof. G3c separately proves musical versus physical
loops over 75/1000 cycles, fractional periods/rates, callback partitions/wrap and
rendered/onset/device/listening gates. Numerical ownership tests supply no human
listening, label or device acceptance.
