# G3b1 transactional prepared-stem publication

The native loader hashes original bytes before decoding and checks the cached
project copy has the same digest. Python source tokens use normalized path plus
`|sha256-v1:<digest>`; observable changes during hashing reject the read. This
detects persistent same-size/mtime replacement, including replacement before job
admission. It is not the future immutable copy-first decode proof: concurrent
unobserved ABA source mutation during decoding remains outside this contract.

The engine retains its content digest alongside loaded generation/rate. Under
the request mutex, capture pins the actual sample Arc, current request, loaded
rate, source version and engine-specific publication epoch. The epoch increments
on new load intent, unload and successfully published pad BPM/origin edits,
including equal-valued edits. It is a preparation freshness counter, not the
provenance-rich accepted timing revision. Overflow fails without wrapping.

Preparation uses the retained actual source and rate outside the GIL. The owner
rechecks the ticket under the same mutex through enqueue. A prepared set retains
the source Arc and epoch permit. Mixer adoption checks the permit and source
pointer; later rendering checks only source pointer so later timing edits preserve
accepted same-source buffers and their existing shared SourcePlayback trajectory.
Rejected sets and pins use existing off-callback buffer retirement.

Each Python separator request has its own artifact directory and ticket identity.
Only its current completion can promote validated complete files into the normal
pad cache. Old workers cannot overwrite current artifacts. A complete-set marker
binds source version and all WAV digests, is removed before replacement, and is
atomically published last. Restore rejects incomplete/tampered sets. Full PCM
cache lifecycle remains the separate C0-C3 gate.
Restoration validates content and captures a new ticket after full-mix loading.
Stat-only legacy stems invalidate; saved timing numbers retain existing behavior.
Atomic status reports actual mixer acceptance/rejection. Python holds availability
false until acceptance and reports late rejection. Performance mode/mask commands
use the loaded identity without original rehashing.

G3b2 remains the complete accepted-timing production caller and binary64 consumer
integration. G3c remains the musical/physical loop and rendered/device proof.
