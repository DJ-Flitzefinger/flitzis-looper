# Design: DSP/FX foundation preparation

## Foundation Boundary

The first implementation slice should add a narrow internal Rust DSP module without adding a
performer-facing effect. The module should be able to host a neutral no-op or test-only node in a
per-pad chain so the mixer can prove the processing boundary without changing audio output.

The current hardwired EQ remains in place during the foundation slice. The safe initial render
position is after source selection, loop wrapping, playback-rate/Key Lock processing, and before
the existing EQ and pad gain/metering path.

## Node And Chain Shape

The initial node/chain model should be intentionally small:

- explicit prepare/reset paths for sample rate, channel count, and maximum block size,
- fixed-size internal node state and parameter slots,
- no heap allocation, file I/O, plugin handles, Python objects, logging, or blocking waits while
  processing,
- deterministic process calls over already prepared sample buffers,
- neutral behavior when no visible DSP node is enabled.

The exact Rust API can follow the local implementation style, but it should preserve these
semantics:

```text
prepare(sample_rate_hz, max_block_frames, channels)
set_parameter(parameter_id, normalized_target)
process_block(frames, channels, buffers)
reset()
```

## Parameter Identity And Smoothing

Future DSP parameters should use typed fixed-size identities. Identities should be stable across
UI frames and mapping dispatch, and must not contain callback-local pointers, plugin handles,
Python objects, file paths, or unbounded metadata.

Accepted continuous targets should enter through the existing bounded parameter path and then
smooth on the Rust audio side. The first foundation slice may use test-only parameters to validate
smoothing without exposing new UI controls.

## Later Isolator Replacement

The later 3-band DJ isolator should be a separate behavior change after the foundation is present.
That change should replace the current hardwired EQ path with a DSP node rather than patching the
current mixer-specific EQ storage.

The later isolator should use normalized internal controls, with `0.5` neutral, full kill at
minimum, limited smooth boost at maximum, and Rust-side smoothing before sample processing.

## Realtime Constraints

The audio callback must still avoid disk I/O, JSON reads/writes, Python/GIL access, UI calls,
blocking locks or waits, logging, neural inference, plugin loading/scanning, unbounded loops,
heavy allocation, and long-running work.
