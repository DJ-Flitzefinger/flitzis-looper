## Why

Adjust Loop already stores signed sample offsets, but Python clamps negative grid origins before
publication and Rust stores a non-negative anchor. The editor and audio engine therefore disagree
about source phase. The next synchronized-launch stage also needs one shared source/master mapping,
an explicit session bootstrap and a defined policy for accepted starts during metadata/BPM updates.

## What Changes

- Publish the same finite signed source-grid origin used by Adjust Loop, preserving its precision
  through fixed-size metadata and Rust source-frame conversion.
- Preserve editor loop-frame markers through `f64` loop seconds on ordinary/direct-MIDI paths,
  correcting native narrowing under the existing sample-accurate loop-marker contract.
- Introduce focused bounded Rust source beat/bar/loop phase mapping and musical-loop compatibility
  checks for prepared constant-tempo material, without activating phase-based playback starts.
- Preserve complete transport musical position through master-BPM changes, rather than retaining
  only the current bar phase.
- Bootstrap the master phase once per stream from the existing selected-pad/BPMLOCK reference.
  A dedicated request selects the reference; unavailable metadata may defer completion.
- Freeze accepted scheduler output targets, ordering and captured timestamps across subsequent
  BPM/metadata changes. Current execution still reads the effective loop start.
- Document the foundation/activation boundary and extend deterministic regression coverage.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `transport-timeline`: signed metadata, canonical source/master phase mapping, one-time reference
  bootstrap, musical-position continuity and stable pending starts.
- `waveform-editor`: preserve the editor's signed source-grid origin through native publication.

## Impact

Rust source-grid/transport/mixer/stream commands and APIs; Python loop/BPMLOCK reference
publication and restore; native stub; focused tests and maintained architecture/native/UI docs.
Existing signed grid-offset persistence and loop-marker formats remain compatible. No dependency
or performer control is added.

## Non-goals

This slice does not activate nearest-boundary launch, source catch-up, phase-based start reads,
DSP pre-roll or audible latency compensation. It does not change Rubber Band, stem generation,
persisted loop markers, variable-tempo warping, plugin hosting or add a SYNC/MASTER control.
Compatibility checks describe prepared constant-tempo loops that evenly tile one 4/4 bar or span
whole bars; they do not claim that arbitrary full tracks or clipped tempo ratios stay synchronized.

## Realtime constraints

All analysis/editor preparation stays outside the callback. Rust operates on bounded scalar
metadata, preallocated scheduler state and existing source handles. The callback performs no
allocation, locking, GIL work, file access, logging, analysis, inference or DSP construction.
Source/clock tests establish mapping behavior, not measured audible hardware synchronization.
