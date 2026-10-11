## Context

Per-pad booleans currently mix three concerns: durable choice, inherited global behavior and live
Native acceptance. A global overwrite loses explicit pad intent, and an OFF load default prevents
new sources from following the current performance default.

## Decisions

### One intent resolver

Keep global `key_lock` as a boolean default. `pad_key_lock[id]` is `true` for explicit ON, `false`
for explicit OFF and `null` for inheritance. Resolve a loaded pad to its override when present,
otherwise the current global default. Reuse this resolver across edits, load completion, restore
and UI; keep the callback's resolved live mode bounded and scalar.

Global changes persist only the default and republish changed inheriting loaded pads. Explicit
overrides survive. The adjacent selected-pad GLOBAL action clears that pad's override; the main
global action changes the default. They are separate actions with separate hit targets.

### Versioned migration

Persist strict integer `key_lock_intent_version: 2` and a fixed-length strict `bool | null` list in
the existing `pad_key_lock` field. Legacy loaded boolean entries become explicit overrides, even
when equal to the global default, because their previous origin cannot be reconstructed safely.
Absent legacy values become inheritance. Unloaded entries clear to `null`. Invalid shape/types or
unsupported versions use the existing safe project-load path.

### Track boundaries and failed publication

Successful completion must match the current source request/generation before clearing the old
track's override. Then resolve the new inheriting track against the latest default. Admission,
worker and stale completion failures do not erase the old track's origin or confirmed Native state.
Unloading clears the override and publishes disabled neutral live state outside realtime.

Retain requested origin separately from pending/preparing, error and confirmed Native status.
Queue admission does not prove Native acceptance. Associate confirmation with the matching
pad/source request; stale results cannot confirm newer intent. A failed effective publication
retains the requested origin and previous confirmed state rather than copying an observed boolean
back into persistence. These projections remain transient.

### UI

PAD left-click selects ON/green; right-click selects OFF/red; inherited PAD is neutral. Adjacent
GLOBAL is default green/red only for inheritance and neutral for explicit overrides. Its left-click
restores inheritance. Repeated gestures retain their meaning. Keep controls hidden for empty or
loading selected pads. Show confirmed/unknown Native state, preparation/pending and error separately;
main global color always represents the default.

## Safety and acceptance

No new source-prepared live handover is introduced. Preserve callback ownership, bounded message
publication, off-thread preparation/retirement and all pending musical/device gates. Status must
not imply that an accepted scalar mode establishes source-aligned playback or hearing acceptance.

The existing active `adopt-pad-owned-pcm-residency` delta's E11-01/E11-02 requirements retain their
identifiers and own-window ACK/actual Native wet-output proof. E11-02 now targets inherited pads
for global changes and preserves explicit overrides; its former all-loaded broadcast is superseded
by this intent contract. Both active changes require official strict validation.

## Validation

Use deterministic focused tests for migration, resolution, explicit override preservation, success
and stale/failure load boundaries, publication failure and UI click/color/status semantics. At the
product boundary run the affected integrated suites, required Release/import/preflight gate and one
independent final review. Validate this change with official OpenSpec strict validation.
