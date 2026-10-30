## 1. Reproduction and implementation

- [x] Reproduce the reported cancellation exception through the actual GUI facade.
- [x] Prove native Pending/no-partial-write behavior and deterministic contention recovery.
- [x] Implement bounded off-realtime retry retaining the original producer exclusion.
- [x] Contain global GUI admission failures with visible feedback and honest state.

## 2. Validation and delivery

- [x] Run impacted Python transport/UI/input regressions and static checks.
- [x] Run native cancellation/STOP/source/queue/claim and callback allocation regressions.
- [x] Validate this change strictly and obtain one independent final review.
- [x] Build/import-check private Release and safely refresh Debug without opening devices.
- [x] Isolate this fix for normal commit/push, preserve unrelated WIP and record owned cleanup.

Python integration: 1476 passed, including ten new GUI regressions. Native final
selection: 28 passed, including all twelve launch-publication tests and two callback
allocation probes; 18 existing failures reproduce identically without this fix.
Fifteen older global START fixtures lack a headless output format; three older
command-count assertions omit the existing captured-target command. They remain
open, with no guard or oracle weakening. A separate broader worker run aborted in
an older seek fixture with stack overflow and is not counted as passing.

One independent final review is CLEAR after the held-right-click regression fix.
Strict validation, static checks, private Release and safely installed Debug import
with nine existing API check families passed. No App/device/listening acceptance
is claimed. Concise positive/negative evidence and cleanup receipt are retained in
the workspace's existing codex-meta/test-log.md.
