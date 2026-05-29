## 1. OpenSpec Planning

- [x] 1.1 Create focused OpenSpec deltas for behavior-visible items 1, 2, 3, and 8.
- [x] 1.2 Validate this change with `openspec validate harden-gen3-runtime-control-paths --strict`.

## 2. Correctness Slice A: Stale Background Completion Identity

- [x] 2.1 Add current-request identity to Rust load and task events.
- [x] 2.2 Reject stale Rust load publication after unload or replacement.
- [x] 2.3 Ignore stale Python loader, progress, error, and analysis events after unload or replacement.
- [x] 2.4 Add focused Rust and Python regression tests.
- [x] 2.5 Perform documentation catch-up for request-id event/API semantics and update maintained
      repo docs if needed before continuing later implementation slices.

## 3. Correctness Slice B: Direct MIDI Learn And Failed Dispatch

- [x] 3.1 Publish Learn/capture runtime state from Python to Rust input runtime.
- [x] 3.2 Suppress direct dispatch while Learn is waiting for MIDI input.
- [x] 3.3 Fallback or surface direct-dispatch failures without partial audio commands.
- [x] 3.4 Add focused Rust and Python input-mapping tests.
- [x] 3.5 Check and update affected maintained docs for input-runtime, Learn, and direct-dispatch
      architecture/API changes.

## 4. Correctness Slice C: Must-Apply Setters And Startup Projection

- [x] 4.1 Make must-apply command and parameter producer failures caller-visible.
- [x] 4.2 Update Python callers/tests to handle explicit failures where appropriate.
- [x] 4.3 Make startup per-pad publication loaded-pad-aware.
- [x] 4.4 Preserve explicit unload and reset neutralization.
- [x] 4.5 Add focused Rust and Python regression tests.
- [x] 4.6 Check and update affected maintained docs for setter acceptance semantics and startup
      restore/projection flow.

## 5. Realtime Optimization Slice

- [ ] 5.1 Remove no-op all-pad parameter work from the callback.
- [ ] 5.2 Reduce callback telemetry all-slot scans without changing meter semantics.
- [ ] 5.3 Remove duplicate Rust DSP prepare work at startup.
- [ ] 5.4 Add focused Rust callback/mixer tests.
- [ ] 5.5 Check and update affected maintained docs for callback work bounds, telemetry semantics,
      and DSP preparation if changed.

## 6. UI And Control-Thread Optimization Slice

- [ ] 6.1 Publish input runtime state only when dirty.
- [ ] 6.2 Fix waveform render-data cache source identity.
- [ ] 6.3 Track active Python meter peaks instead of decaying all pads every frame.
- [ ] 6.4 Add focused Python tests.
- [ ] 6.5 Check and update affected maintained docs for input-runtime publishing, waveform cache
      identity, or metering projection if changed.

## 7. Validation

- [ ] 7.1 Run focused validation after each implementation slice.
- [x] 7.2 Run strict OpenSpec validation for this change after spec edits.
- [ ] 7.3 Run full uv-managed Rust/Python validation before completing the branch handoff.
- [ ] 7.4 Confirm required documentation updates are complete, or explicitly record why no
      maintained docs changed for each slice.
