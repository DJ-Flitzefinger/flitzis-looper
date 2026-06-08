# Change: Harden Gen3 Runtime Control Paths

## Why

The completed performance audit identified several correctness and performance risks around async
pad work, direct MIDI dispatch, queue publication failures, and startup state projection. These
paths are visible to performers because stale completions can restore old pad state, Learn can
accidentally trigger an already mapped MIDI action, silent queue failures can leave Rust live state
different from Python intent, and startup can spend work publishing all-pad state that only matters
for loaded pads.

This change defines the behavior contracts needed before implementing approved performance-tuning
proposals 1 through 10 on the `gen3-performance-tuning` branch.

## What Changes

- Add current-request identity checks for per-pad load and analysis completions so stale results
  cannot overwrite newer unload or replacement intent.
- Define Learn precedence over direct MIDI dispatch and define how failed direct dispatch is
  surfaced without partial audio commands.
- Require must-apply Rust command and parameter publications to report enqueue failures instead of
  silently succeeding.
- Make startup project-state publication loaded-pad-aware while preserving explicit unload and
  reset neutralization.
- Keep internal-only callback, telemetry, waveform-cache, input-runtime, meter-decay, and duplicate
  DSP-prepare optimizations in implementation tasks without adding new performer-facing behavior
  requirements unless implementation discovers a contract change.

## Non-Goals

- This change does not add optional audit proposals 11 through 15.
- This change does not introduce plugin hosting, external plugin scanning, or new DSP modules.
- This change does not add disk I/O, JSON access, Python/GIL access, UI calls, logging, blocking
  waits, neural inference, unbounded loops, or heavy allocation to the CPAL audio callback.
- This change does not replace the existing best-effort telemetry model with a reliable audio-state
  acknowledgement stream.
- This change does not redesign the UI.
