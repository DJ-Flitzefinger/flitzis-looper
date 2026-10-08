## C0: audit and contract (complete)

- [x] Audit actual editor/seek/ALL/stem/Key Lock/load/analysis/timing/cleanup contracts.
- [x] Record current gaps, selected identity/timebase/residency/readiness/lifecycle
  decisions and bounded C1-C3 proof plan in maintained docs.
- [x] Pass official strict validation and independent final staged semantic/hash review.

## C1a: immutable input and complete cold artifacts

- [x] Bound source-job admission/worker concurrency and transient PCM bytes; retain
  request cancellation before changing the productive cold load pipeline.
- [x] Copy/hash stable bytes first and decode that immutable snapshot; prove
  replacement/ABA, byte-exact originals and snapshot reader lifetime.
- [x] Produce complete versioned decoder PCM and playback derivative/manifest
  from actual digests and recorded transforms with full-buffer playback retained.
- [x] Commit cold artifacts atomically from exclusive staging; implement snapshot/
  staging cancellation and queue-failure rollback before productive publication.
- [x] Prove decoder delay/padding and playback resampler phase/delay/tail/ceiling
  behavior against independent 44.1/48/96-kHz fixtures; preserve source zero.
- [x] Integrate request-guarded all-or-none cold source/metadata publication and
  current load/timing/stem guards without promoting historical
  acceptance; document, validate and independently review the exact final tree.

## C1b: reuse and last-owner lifecycle

- [x] Validate full cache/source content on fresh immutable leases, reuse complete
  compatible entries and regenerate partial/corrupt/version-incompatible entries.
- [x] Extend atomic guarded cold publication to validated warm reuse, deduplicated
  digest ownership and shared-subscriber cancellation/shutdown.
- [x] Replace eager project-asset deletion with safe last-reader/job/pad/voice
  cleanup; prove containment, retry, shared ownership and external-original safety.
- [x] Capture warm-integrity byte/CPU accounting; preserve save/export integrity
  and rerun meaningful productive checks with independent final-tree review.

## C2a: complete-source authority and finite resident windows

- [x] Separate complete identity/metadata/evidence from resident handles/counts;
  update loaded shape, CURRENT/MIDI/source/history bindings coherently.
- [x] Derive and independently prove exact reader/loop/rate/DSP context; use
  admitted full-track fallback for unsupported context, never guessed margins.
- [x] Restore saved short loops as finite resident windows with absolute offsets,
  matching source/window revisions and full-mix/component-stem transactions.
- [x] Permit only same-source/same-complete-StemSet window relocation during
  active playback; preserve inactive-only generation/new complete-set adoption.

## C2b: readiness and all existing controls

- [x] Preserve full-source waveform/navigation/analysis with bounded complete-source
  leases; expose pending/error without moving source zero or timing/labels.
- [x] Prepare finite window/ALL/outside-loop-seek transitions with old audio valid
  until guarded adoption ACK; preserve intro/tail-wrap and paused/stopped seek.
- [x] Route UI/MIDI/control intents through one transaction behavior, including
  stale intent/unload/error/queue pressure and off-thread retirement.
- [x] Prove full-buffer parity, accepted P versus physical H, <=1-loaded-frame
  long-cycle bounds, fractional rate/Key Lock/stems and actual lifecycle; full checks.

## C3: measured acceptance

Results and measurement limits: [C3 measurement report](../../../docs/pcm-cache-measurements.md).

- [x] Freeze real cold/warm protocol, independent oracles and source/cache identities.
- [x] Measure 200 occupied pads, short loops in long sources, duplicate/shared
  sources and ALL/editor/analysis/outside-loop-seek/full-context exceptions.
- [x] Measure readiness, worker/handle counts, retained/transient PCM and process
  peak RAM, disk/validation/save I/O, CPU and cancellations/cleanup in Debug/Release.
- [x] Report current 96-native-handle setup/reserve costs and <=1-frame/parity/
  realtime results; compare identical sources/conditions and report regressions.
- [x] Update docs with measured results; leave actual human/device acceptance open
  for the final authorized acceptance stage. Stop before Rust-port planning.
