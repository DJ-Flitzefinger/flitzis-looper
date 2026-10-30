## Implementation
- [x] Reconcile the observed ordinary-save exception with the common Config writer.
- [x] Retain optional-save errors and current dirty intent with bounded automatic retry.
- [x] Observe deferred saves through the existing control-thread console sink.
- [x] Add real atomic-replace fault injection and latest-intent recovery coverage.
- [x] Run focused persistence, timing, migration-fence and console regressions.
- [x] Pass focused lint, format, types, strict OpenSpec validation and final review.
- [x] Recheck the actual App save path without claiming a Windows failure cause.

Root evidence: 8 new cases PASS and 123 existing neighbors PASS; focused Ruff,
format, mypy, strict validation and independent final review CLEAR. The actual
Release App remained usable during a real Windows Config replacement denial
from an owned six-second read handle, kept the prior bytes and newer dirty
revision, then saved automatically after at least ten seconds. This deliberate
failure proves retry behavior without identifying the first observed failure's
cause. See workspace codex-meta/test-log.md and owned live-app-20261020 logs.
