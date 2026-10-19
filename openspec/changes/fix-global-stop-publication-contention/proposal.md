## Why

Right-clicking global START/STOP while another pad is being loaded or analyzed can
encounter the native content publication marker. Correct whole-set cancellation rejects
that busy claim, but its RuntimeError escapes rendering and terminates the application.

## What Changes

- Retry only brief native Pending publication contention on the control thread, with
  a fixed attempt bound and the original sole-producer exclusion held throughout.
- Preserve atomic whole-set cancellation, guarded STOP publication, captured source
  identity, current timing, launch ordering and truthful failure on persistent contention.
- Display remaining global START/STOP admission errors without terminating the GUI or
  fabricating stopped/restored state or an uncaptured delayed action.

## Non-goals

No callback retry loops, sleeps, additional audio path, raw unguarded STOP fallback,
automatic playback, data reset, cargo-fix sweep or unrelated Key Lock changes.

## Impact

Native off-realtime launch-control preflight/claim, global GUI admission boundary and
existing START/STOP error feedback. No persistence or public native API change.
