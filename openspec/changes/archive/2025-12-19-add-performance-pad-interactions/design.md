## Context
- Legacy interaction model (Tk):
  - Left-click triggers/retriggers
  - Right-click stops
  - Middle-click opens a per-pad context menu
  - Reference: `old-project/flitzis_looper/ui/widgets/loop_grid.py`
- Current audio API supports play by ID and load by ID, but not stop.
- Current sample slot range is `0..32`, which does not match a 6×6 grid (36 pads).

## Goals / Non-Goals
- Goals:
  - Implement left/right/middle click interactions on pads in Dear PyGui.
  - Provide an audio-engine stop operation so retrigger can be deterministic.
  - Align sample slot ID range with the 36-pad UI.
- Non-Goals:
  - Per-bank pad assignment persistence (handled by later parity epics)
  - Looping playback semantics (handled by parity epic 2)

## Decisions
- Decision: Pad-to-sample mapping uses a simple default
  - Default mapping: pad number `1..36` maps to sample slot `id = pad_number - 1`.
  - Rationale: Keeps the system testable with the existing `load_sample(id, path)` API.
- Decision: Retrigger is implemented as stop-then-play
  - On left-click, the UI sends `stop_sample(id)` followed by `play_sample(id, velocity)`.
  - Rationale: Provides predictable restart semantics without tracking voice state in the UI.
- Decision: Add a real-time-safe `StopSample` message
  - The audio callback marks voices matching `sample_id` as ended without allocating or blocking.
- We should keep `MAX_SAMPLE_SLOTS` aligned to 36 (one bank’s worth), and plan for a larger fixed capacity to eventually support multiple banks without reloading.

## Risks / Trade-offs
- Bank switching does not yet imply independent audio content per bank; later changes can introduce a bank-aware mapping and sample reloading strategy.
- DPG mouse button handling differs across platforms; verify right/middle click behavior on the target OS.
