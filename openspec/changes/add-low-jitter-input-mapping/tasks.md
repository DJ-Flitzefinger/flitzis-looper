# Tasks

## OpenSpec
- [x] Define low-jitter input mapping behavior and real-time boundaries.
- [x] Validate with `openspec validate add-low-jitter-input-mapping --strict`.

## Implementation
- [x] Add Python mapping models, storage, Learn controller, and Settings actions.
- [x] Add Rust MIDI input runtime with timestamping, filtering, bounded queues, and in-memory lookup.
- [x] Bridge MIDI mapping snapshots and runtime state from Python to Rust.
- [x] Route keyboard and MIDI mappings through shared LooperAction semantics without simulated clicks.
- [x] Update official docs.
- [x] Update local Codex state notes.

## Validation
- [x] Add focused Python tests for storage, Learn save/delete, keyboard focus suppression, and Settings clear-all.
- [x] Add Rust unit tests for MIDI normalization/filtering and dispatch behavior.
- [x] Add hardware-free Python bridge test for injected MIDI input.
- [x] Run the full uv-managed validation sequence.
