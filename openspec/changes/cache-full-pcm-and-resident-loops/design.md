# C0 decisions and C1a/C1b integration

The maintained design and audited code/test/spec map live in
[PCM cache and residency](../../../docs/pcm-cache-residency.md).
That document distinguishes current implementation from the selected C1-C3
contracts and supplies slice boundaries, failure behavior and proof requirements.

## Selected boundaries

- Copy and hash actual stable source bytes into an immutable project-owned
  snapshot before decode; keep byte-exact originals independently of regenerable PCM.
- Store full decoder-domain PCM and versioned full playback derivatives. A
  canonical identity includes actual original/PCM digests, versions, dimensions,
  complete extent, zero and transform; neither paths nor stat tuples prove content.
- Keep complete-source authority distinct from resident ranges/window revisions;
  use full-source non-realtime leases for existing editor/analysis consumers.
- Admit short loop residency only with demonstrated interpolation, wrap, rate,
  Key Lock/history and stem context. Use an explicit complete-track resident
  fallback when bounded context is not proved or outside-loop seek requires it.
- Keep the old effective audio until a matching finite transaction is accepted.
  Pending intent is separate from CURRENT/effective state; ACK establishes readiness.
- Reuse existing request guards, native permits, command/feedback admission and
  off-thread retirement. Cleanup waits for pads, jobs, queued handles and voices.

## Tradeoffs and unresolved measurements

Complete source and PCM verification on a fresh warm cache lease entails full
integrity I/O. Leases may share that verified immutable object within a process;
mtime/size alone cannot authorize reopening. Whole editor/analysis/seek/ALL leases
can temporarily retain complete PCM, and proved DSP context may be large. Peak RAM,
disk bytes, CPU, readiness and integrity cost must be measured rather than hidden.
Decoder delay/padding and the current playback resampler's short/tail behavior
need explicit versioned proof before an artifact claims time-origin parity.

## C1a implementation boundary

The productive existing load API uses two fixed workers, 32 queued/reserved
requests and 1 GiB transient PCM per worker (2 GiB aggregate, not process RAM).
Windows write/delete-excluded copy-first leases, canonical complete PCM
manifests, exclusive same-filesystem atomic directory commit and post-commit
full sealed verification precede bounded guarded native source adoption ACK.
Only matching ACK permits source/metadata Success; cancellation and queue/guard
failure roll back exclusively owned cold creations and their still-current
proposed control source. Newer source or Manual/Tap intent is never overwritten.
A stalled backend after the bounded irreversible callback tail begins has a
finite unconfirmed-adoption error: complete files remain durable and no Success
or unsafe rollback is invented. C1b retains actual native readers off-thread and
reconciles explicit retirement, stale metadata delivery and shutdown.
Power-loss directory durability and portable stable capture remain unproved;
non-Windows capture fails safely. Resident windows and measured startup/RAM
acceptance remain C2/C3. See maintained docs
for exact policies, independent decoder/FFT evidence and the realtime boundary.

## C1b implementation boundary

Warm admission verifies the complete actual source, selected decoder configuration,
canonical manifest, every decoder/playback PCM byte and recomputed mono digest on
fresh immutable readers. A bounded candidate scan rejects incompatible/corrupt
generations without overwriting them. Shared preparation is gated by digest/device;
native publication remains request/generation/intent guarded and ACK based.
Cross-pad PCM can share, while live former same-pad readers require an admitted
complete copy to preserve existing address-based source/history authority.

Non-realtime assignment/job tokens and weak queued/bank/voice/history PCM readers
protect exact originals, cache assignments and immutable stem generations. Pending
delivered metadata has bounded retained ownership until claim, stale retirement
or engine shutdown. Persisted assignments survive ordinary process shutdown.
Cleanup checks resolved containment, reparse attributes and captured file identity,
uses exact Windows handles and bounded sharing retries, and preserves unknown or
newer generation content. Recognized dead-process staging is recovered only under
ownership exclusion. Final empty stem containers can be removed nonrecursively
under the same gate; otherwise they remain. Warm integrity and real export/save probes record costs without
weakening verification or claiming C3 performance. Full-buffer playback remains.
