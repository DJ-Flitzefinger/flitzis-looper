## Coherent implementation and verification

### Stage A: actual waveform holds and reusable source projection

- [x] Freeze the source-grounded epoch/cohort design and affected ownership.
- [x] Implement exact bounded dry mapped continuation through the actual own-ACK hold path.
- [x] Verify real PCM/control/hold behavior, idempotency, partitions, isolation and refusal cases.
- [x] Run affected regressions, normal Release/import/preflight, strict validation and one independent final-diff review.

### Stage B: ordinary content transport and truthful feedback

- [x] Freeze lossless content/revision-correct ordinary pause/resume feedback, including Pause/Seek ordering.
- [x] Reuse Stage A continuation through actual public content Pause/Resume.
- [x] Confirm ordinary effective state from Native, preserving fast gestures and separate Hold ownership.
- [x] Verify feedback pressure, newer revisions, Move/Swap, retirement and UI/control regressions; perform the coherent delivery checks.

After each coherent stage, commit and normally push it to existing main without
individual approval; verify clean HEAD/tracking/fresh actual remote equality
before the next independent stage or change, and clean proven own disposable output only
after jobs are terminal and evidence retained. Preserve the unchanged FULL goal,
original/extensions/Human/final gates; STOP before full Rust-port/Slice8.
