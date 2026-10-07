# C0 decisions

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
