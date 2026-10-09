## Why

The selected variable-beatmap/SYNC direction must leave room for future per-pad KEY transposition
without making pitch another timing authority. The current LiveShifter adapter derives pitch
only from inverse tempo, clamps that value and bypasses near unity. Those assumptions do not
establish independent transposition or aligned transitions across variable rates.

## What Changes

- Define an internal pitch/time contract with separate semitone intent, canonical source
  trajectory and common output-frame coordinates.
- Prepare test-only nonzero transposition diagnostics for retained Rubber Band render paths,
  combined pitch/rate bounds, native state identity, readiness and pitch-change latency.
- Expand B5 diagnostics to all37 extra steps/base -5..+6/total -23..+24 and actual
  r(n), preserving original acoustic/RT/latency gates and individually admitted events.
- Keep current production transposition at zero; the pending
  `add-key-transposition-performance` change owns later guarded performer activation.

Status: planning draft, with uncompleted implementation tasks. This is internal contract and
diagnostic preparation, not a KEY control or live SYNC activation. It uses accepted map semantics
from `prepare-versioned-source-beatmaps` or exact synthetic maps, and complements
`evaluate-variable-tempo-rendering`. This is the B5 core proof in
`../../../docs/beatmap-sync-design.md`; K1 is the separate future performer feature.

## Non-goals

No user-facing KEY UI/API, new mapping action, project-field rollout, model installation, live
rendering adoption, global speed reinterpretation, manual-key mutation, automatic key matching,
stem regeneration, plugin host, changed acoustic thresholds or full Rust application port.

## Realtime Constraints

All experimental construction, reset, analysis, rendering and exports run outside the callback.
This change does not introduce a callback allocation, lock, wait, log, GIL access or destructor.
Future live adoption requires separately validated native preparation and bounded ownership;
offline measurements do not prove callback safety or hardware timing.

## Impact

One focused internal contract and test/example diagnostics. No production input path exposes
nonzero KEY in this slice. Existing source grids, original audio/stems, manual metadata,
transport, active scheduler behavior and persisted settings remain unchanged.
