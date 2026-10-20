## Cause

`AppController._restore_initial_runtime_state` starts loader restores before
`ApplyProjectState` runs. `on_pad_bpm_changed` sends legacy BPM, origin and optional
manual/tap intent. Native `commit_timing_intent_under_request_lock` advances the
prepared-source epoch and timing authority even for an empty slot. An unarmed
ColdLoad must reject that changed capture, independently of any transient Pending
placement marker. Its final source arming also requires the original epoch. The
existing late empty-slot test edits only after the Cold source has been enqueued;
that does not exercise preparation before arming.

## Ordering

First reuse the loader's original path-validation and missing-assignment cleanup
policy without admitting workers. Retain recovery-owned missing references and
their deferred ownership. Run the complete saved transport/control projection
only for the classified valid source IDs, excluding missing recovery references
from per-PAD controls and master-anchor selection without mutating their intent.
Then admit the classified original paths, still subject to the existing Native
path/source and request checks. The classification is no new source authority.
Standalone loader restoration reuses that same validation helper.

Capture the options for each asynchronous restore only after that
projection has finished, including legitimate loop/grid normalization. Keep
existing arrangement recovery fences, stem validation, migration scheduling and
Native selection initialization. Loading still initializes source-bound pitch and
Key Lock atomically. Automatic timing remains unresolved until the original Cold
acknowledgement and subsequent fresh saved-evidence verification.

Setter admission order alone does not prove callback adoption. Gain/EQ and legacy
timing messages captured against the empty slot may be delayed until after Cold
publication advances its lifetime. Their existing incarnation guards must remain.
Restored gain/EQ travel as validated fixed scalars in the original Cold payload and
apply inside its existing exact source claim before acknowledgement. The existing
post-ACK analysis refresh repairs stored legacy timing. An old empty-slot message
never gains permission to modify arbitrary replacement content.

The two optional `load_sample_async` keywords `initial_gain_db` and `initial_eq_db`
require explicit restore intent and a complete gain/three-band tuple together.
Reject malformed, nonnumeric, nonfinite, out-of-range, partial or import-only
options before reserving work or changing authority. Absent values retain existing
fresh/import and migration behavior. Ordinary restore always supplies its saved
tuple, including neutral values, and the existing portable API preflight requires
these keywords before activation.

The existing transport readiness boundary also rejects current-source gestures
while `SessionState.loading_sample_ids` still owns an unprojected restore. This
prevents accepted-looking gain/EQ edits against the empty slot from being lost or
overwritten by the frozen original restore tuple. The same guard already owns
ordinary incoming-source refusal and PAD Key Lock, loop, timing and audible pitch
gestures. Startup projection runs before that loading row exists; the loader
settles the row before its own post-ACK refresh. No new pending-gesture queue is
introduced, and GUI/mapped-input refusal preserves durable state until a fresh
gesture follows the source receipt.

## Verification

Controller regressions exercise the actual startup method with generated local
references and a mocked audio boundary, covering legacy, manual, tap, Automatic,
finite loops, inherited Key Lock, pitch, mixing and missing-source cleanup. They
cannot prove actual Native cancellation or callback acceptance. A separate
device-free Native probe must hold the original Cold worker before preparation or
publication, exercise real timing setters and compare initialization before versus
after admission, with genuine callback acknowledgement in the successful control.
Real replacement, source epoch/proposal/request invalidation and shutdown retain
their rejection tests. No device/listening acceptance is inferred.

A real delayed-parameter regression must publish the source before consuming the
earlier empty-slot gain/EQ messages, receive a genuine callback ACK, then observe
the exact saved mixer values. Neutral restoration and rejected/superseded original
payloads are controls; no stale payload may change replacement mixing.
