# Source-bound prepared stem publication

G3b1 binds the productive Demucs/restored-stem path to the actual immutable loaded
source. It does not adopt the G3a accepted constant timing record.

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

Offline cancellation keeps its existing no-return teardown API: it marks the job
cancelled even if counter exhaustion rejects the shared identity advance. Such a
rejected advance preserves both counters; cancelled analysis still cannot publish
a result. Private checked transitions report the exhaustion error. A cancelled job
whose request was already superseded does not advance the newer owner's counters.

`publish_prepared_stems(id, source_version, cache_dir, source_ticket)` requires the
unchanged admission ticket. It checks ownership before preparing, decodes/aligns
outside the GIL, then rechecks under the request mutex through enqueue. A ticket
enqueues once; a full queue leaves it unconsumed. Old or foreign owners fail.

The prepared set retains the actual alignment-reference Arc and an atomic permit.
Mixer adoption checks both; rejected sets and source pins use existing off-callback
retirement. Rendering also checks source pointer: shape, rate and equal values
are insufficient. Later edits retain accepted same-source stems and their shared
SourcePlayback trajectory/Key Lock source-frame read.

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

G3b2 must feed TimingAdoptionGuard actual source/request/intent and use one complete
accepted binary64 period/origin/revision in editor, snap, auto-loop, SourceGrid,
transport/master and BPMLOCK. Accepted revisions must bind MIDI and prepared Key
Lock/stem state; this generic epoch cannot replace them. Source-verified accepted
timing persistence and manual/TAP/legacy policy remain unimplemented. G3c separately
proves musical versus physical loops over 75/1000 cycles, fractional periods/rates,
callback partitions and rendered/device gates.
