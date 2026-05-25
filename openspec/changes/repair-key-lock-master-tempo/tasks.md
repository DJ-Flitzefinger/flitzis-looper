## 1. Specification And Research

- [x] 1.1 Capture the Key Lock master-tempo repair as an OpenSpec behavior change.
- [x] 1.2 Record the library/backend decision and real-time audio constraints.

## 2. Rust DSP Repair

- [x] 2.1 Make Key Lock off use varispeed/repitch semantics.
- [x] 2.2 Make Key Lock on use bounded master-tempo pitch compensation.
- [x] 2.3 Preallocate all per-voice DSP buffers and delay lines outside the audio callback.
- [x] 2.4 Preserve BPM Lock, prepared stems, Multi Loop, trigger scheduling, loop-region, gain/EQ,
  metering, and playhead integration.
- [x] 2.5 Add bounded manual Key Lock DSP parameters with former High values as the default.
- [x] 2.6 Persist the Key Lock parameter settings and publish changes through fixed-size Rust
  control messages.

## 3. Tests And Documentation

- [x] 3.1 Add deterministic Rust tests for Key Lock pitch behavior.
- [x] 3.2 Add deterministic Rust tests for neutral-speed transparency and preallocated buffers.
- [x] 3.3 Add regression coverage for bounded pitch-compensation delay-line wraparound.
- [x] 3.4 Add regression coverage for bounded Key Lock parameters and their project/UI plumbing.
- [x] 3.5 Update repository audio/message-passing/time-stretch documentation.

## 4. Validation

- [x] 4.1 Run `openspec validate repair-key-lock-master-tempo --strict`.
- [x] 4.2 Run the full uv-managed Rust/Python validation sequence.
