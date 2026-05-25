# Tasks

## OpenSpec
- [x] Define low-jitter input mapping behavior and real-time boundaries.
- [x] Validate with `openspec validate add-low-jitter-input-mapping --strict`.

## Implementation
- [x] Add Python mapping models, storage, Learn controller, and Settings actions.
- [x] Add Rust MIDI input runtime with timestamping, filtering, bounded queues, and in-memory lookup.
- [x] Bridge MIDI mapping snapshots and runtime state from Python to Rust.
- [x] Route keyboard and MIDI mappings through shared LooperAction semantics without simulated clicks.
- [x] Repair Learn target coverage for Tap BPM, stem mask buttons, per-pad EQ bands, and Master Volume.
- [x] Add relative MIDI CC steps for Master Volume and per-pad EQ while preserving set-value
      keyboard and MIDI Note mappings.
- [x] Add relative MIDI CC steps for per-pad Gain and global Speed/Pitch while preserving
      set-value keyboard and MIDI Note mappings.
- [x] Support endless-encoder CC wraparound and repeated relative encoder values for relative
  continuous-control mappings.
- [x] Support common MIDI encoder increment/decrement encodings such as `1`/`127` and `65`/`63`.
- [x] Normalize NRPN increment/decrement encoder messages to stable learnable MIDI bindings.
- [x] Update official docs.
- [x] Update local Codex state notes.

## Validation
- [x] Add focused Python tests for storage, Learn save/delete, keyboard focus suppression, and Settings clear-all.
- [x] Add focused Python tests for the repaired learnable control coverage.
- [x] Add focused Python tests for relative MIDI CC continuous-control mappings.
- [x] Add focused Python tests for endless-encoder wraparound and Gain/Speed relative mappings.
- [x] Add Rust unit tests for MIDI normalization/filtering and dispatch behavior.
- [x] Add hardware-free Python bridge test for injected MIDI input.
- [x] Run the full uv-managed validation sequence.
