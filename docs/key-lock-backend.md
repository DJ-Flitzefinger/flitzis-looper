# Key Lock Backend

This document records the current Rubber Band based Key Lock implementation.

Key Lock is one bounded part of the Rust audio/DSP foundation. It does not imply
plugin hosting, a separate FX graph, or realtime stem generation.

## Runtime Modules

The active backend is implemented behind:

```text
rust/crates/looper/src/audio_engine/stretch_processor.rs
rust/crates/looper/src/audio_engine/rubberband_backend.rs
rust/crates/looper/src/audio_engine/key_lock_preparation.rs
rust/crates/looper/src/audio_engine/source_playback.rs
rust/crates/looper/src/audio_engine/source_reader.rs
```

`RtMixer` owns tempo-ratio target selection, per-pad Key Lock state and per-voice
source/processor calls. `source_reader.rs` centralizes
effective loop regions, explicit seek progression, full-mix/stem compatibility,
integer source addressing and source-selection crossfades. It borrows accepted
buffers and can be reused by background preparation without importing the mixer
or native processor. `source_playback.rs` owns scalar fractional source epochs
and active-output-frame ratio smoothing. The mixer uses that cursor with the
reader's linear interpolation taps and advances stem transition ramps by
fractional source distance.
`VoiceSlot` owns the per-voice source playback state, `StretchProcessor` and
explicit seek mode. The mixer retains one preparation worker for its 32 voice lanes.

## Playback Semantics

- Per-pad Key Lock off: playback is varispeed, so tempo and pitch move
  together.
- Per-pad Key Lock on: source-frame tempo progression remains active, and the
  varispeed block is processed through a per-voice Rubber Band LiveShifter with
  pitch scale derived from `1.0 / tempo_ratio`.
- The global Key Lock control overwrites currently loaded pads' per-pad Key
  Lock values. A later per-pad toggle changes only that loaded pad, and unloaded
  pads remain disabled.
- BPM Lock off: the active tempo ratio is the global speed multiplier.
- BPM Lock on with valid master and pad BPM metadata: the active tempo ratio is
  `master_bpm / pad_bpm`.
- Pads without valid BPM metadata use the global speed multiplier.
- Full-mix and prepared-stem playback share the same source addressing and Key
  Lock path.

## Source Timing And Resampling

Scheduled render segments carry absolute output-frame positions from
`audio_stream.rs` into `RtMixer::render_rt_at_output_frame(...)`.

Every playback mode uses `SourcePlayback` to derive fractional source progress
from a scalar source epoch plus active output-frame count times the actual native
`f32` tempo ratio promoted to `f64`. Reads linearly interpolate two integer
neighbors through the shared half-open loop/seek policy. The lookahead tap wraps
at loop end; explicit intro seeks play into the loop, and tail seeks play to
track end before wrapping into the loop. Both channels and prepared-stem
selections use the same addresses and fractional source transition progress.

Rate changes rebase from the current fractional position instead of independently
rounding source consumption per segment. Pause freezes source and smoothing
progress; resume continues them. In-range loop edits retain fractional carry,
while out-of-range edits retain the existing loop-start clamp. Explicit seeks
and retriggers start a new source epoch. Integer telemetry floors the next
source cursor. Ordinary wrapping does not redefine the Rust master output
timeline, Rubber Band state, editor source grid or prepared-stem alignment.

The same source feed supplies dry varispeed output and the Rubber Band adapter.
`StretchProcessor` no longer interpolates render-segment endpoints. This makes
source/native input independent of fixed, irregular and one-frame partitions
when source state and accepted control events are equivalent.

The separate `source_grid.rs` foundation derives source beat/bar and loop-cycle
phase from the editor's signed origin. Transport retains complete beat position
across master-BPM changes and can bootstrap once from the selected BPMLOCK
reference. These operations preserve voice read positions and Rubber Band
ownership. The master-to-source mapping helper is internal and tested; normal
starts still read the effective loop beginning. Neither that helper nor the
one-time bootstrap compensates audible Rubber Band/device delay.
Bootstrap queries a copy of the canonical source cursor configured against the
current effective loop. It maps the integer source frame plus its fractional
remainder to beats, matching the position rendering will read after loop/seek
normalization without advancing the live cursor. A changed loop cannot anchor
the master to a source position that will be clamped before the next read.
These changes do not reconstruct Rubber Band state.

## Rubber Band Processing

### Preparation Ownership

Stream setup constructs 64 unique native handles: one current handle and one
reserve for each of 32 voices. Every handle is warmed with neutral silent
blocks before callback rendering. Fixed block buffers, channel pointer arrays,
and bounded FIFOs are also allocated before rendering. Preparation failures,
including failure to start the worker, reject stream setup.

The two-handle pool has a measurable startup/memory cost. Cold standalone
Windows measurements for 32 stereo voices observed:

| Output rate | Pool setup | Working-set increase | Private-committed increase |
| --- | --- | --- | --- |
| 48 kHz | 151.347 ms | 137.477 MiB | 146.668 MiB |
| 96 kHz | 270.090 ms | 205.965 MiB | 230.719 MiB |

These are single-run process deltas on the measured system, not portable memory
limits or live callback costs. The fixed reserve preserves quality/options and bounds
ownership exchange; its resource cost remains part of later profiling.

The pinned Rubber Band 4.0.0 source audit found that native `reset()` and a
pitch change before the first `shift()` call `measureResamplerDelay()`, which
creates two temporary `std::vector<float>` buffers. The probe's calling-thread
Rust allocator counter does not observe these C++ allocations. Construction,
native reset, cold pitch setup, and silent warming therefore run only during
setup or on the preparation worker. The callback applies pitch changes to an
already warmed uniquely owned
handle. See the [pinned native implementation](https://github.com/breakfastquay/rubberband/blob/v4.0.0/src/finer/R3LiveShifter.cpp).

Start/retrigger, stop, seek, and leaving wet processing clear only the adapter's
own bounded storage and mark used native state dirty. On the next wet render,
the voice exchanges that state for its warmed reserve through two bounded SPSC
queues. The worker resets and warms the returned handle. The exchange reserves
return capacity first and never destroys native state, waits, or prepares DSP
inside rendering. Teardown releases the worker after voice rendering stops.

When no reserve is ready or the recycle lane is full, wet rendering returns
silence for that segment and retries later; the source timeline continues.
Dry varispeed and the approximately neutral ratio remain immediate. Pause/resume
retain the current native state. Stem mode/mask transitions retain native
history and use the existing source crossfade instead of resetting Rubber Band.

### Adapter And Measured Delay

The tested SHORT + CHANNELS_TOGETHER backend uses 512-frame blocks. Wet
activation seeds the output FIFO with `block_size - 1` silent frames, so the
adapter adds a fixed 511-frame lead (10.646 ms at 48 kHz), independent of callback
partitioning. This removes the previous growing offset from underflow silence;
the lead is currently uncompensated. Missing shifted output still uses bounded
silence, with no refill spin.

Native nominal delay, measured transient onset/peak, adapter lead, and device
buffering are separate quantities. The initial optimized offline baseline used
cold ratio-specific native state and isolated impulses at output-domain frame
8192:

| Output rate | Tempo ratio | API delay frames | Impulse peak delay frames |
| --- | --- | --- | --- |
| 48 kHz | 0.5 | 2909 | 2909 |
| 48 kHz | 2.0 | 3678 | 3260 |
| 96 kHz | 2.0 | 7774 | 6844 |

These baseline values are not correction constants for music. In the final
warmed-pool probe, the 48 kHz ratio-2 adapter impulse peak was 3771 frames after
its reference: a 3260-frame native peak plus the fixed 511-frame adapter lead.
Fixed 64/128/256/512-frame callbacks and the irregular
`[64,96,257,512,31,1]` pattern produced the same peak for this fixture. The
nominal native API delay was still 3678 frames. The independent fixed-lead FIFO
model reported no underflow across 2048 calls per tested pattern.

Final optimized 48 kHz ratio-2 measurements, using 24 repetitions, observed:

| Operation | Median us | p95 us | Maximum us |
| --- | --- | --- | --- |
| Native preparation for reuse | 1250.25 | 1578.50 | 1742.60 |
| Adapter first activation and processing | 215.90 | 531.30 | 534.50 |
| Adapter Rust-only reset | 0.90 | 1.30 | 2.60 |

The baseline adapter reset median was 105.35 us and first activation median was
800.40 us. Moving preparation changes where work happens; it does not remove
the native preparation cost. Calling-thread Rust allocation counts were zero
for measured first activation, reset, reactivation, and warm processing; worker
and C/C++ allocations are outside that counter.

A handle warmed at neutral pitch retains different startup history when its
ratio changes. The API delay getter remains nominal; it does not prove the
output transient's location for that history or a changing ratio. Startup and
settled responses need separate measurement. The local baseline and final
prepared results live in workspace `scratch/`, including
`slice3-key-lock-latency-findings.md` and
`slice3-key-lock-latency-{baseline,prepared}.csv`; generated CSVs and logs are
not repository artifacts. Reproduce the probe using
[the development guide](development.md#offline-key-lock-measurement).

The adapter partition test supplies identical already-varispeed samples. The
fractional source foundation also compares immutable nonconstant sources,
loop/intro/tail boundaries and prepared-stem sums through the common source
path at 44.1/48/96 kHz. Rate changes and pause/resume use active-frame progress,
so their source feed does not change with render partition sizes. This proves
the source-to-adapter prerequisite independently of the adapter FIFO property;
it does not prove source priming, transient compensation or audible hardware
alignment. Exact-source preparation can reuse this source path, but its native
pre-roll and prepared-state activation are still pending.

This preparation and adapter safety stage (slice 3a) does not perform track
pre-roll, delay discarding, or a separate DSP feed-ahead cursor. Source playheads,
persisted markers, the shared clock, and launch scheduling keep their existing
meaning. Source-aware prepared handover, audible phase compensation, and short
wet/bypass transitions, including ratio 1.0 and global/per-pad toggles, remain
slice 3b work. Current mode changes can still switch between delayed wet output
and immediate dry output without that transition compensation.

Output-clock snapshots estimate device buffering from CPAL callback timestamps.
They do not include native/adapter signal delay or unknown latency after the
device buffer. Offline impulse measurements and Rust allocation telemetry do
not establish hardware onset precision or live callback deadlines. Captured-input
nearest-grid diagnostics remain separate from current launch execution.

## Settings Contract

Project persistence stores the global `key_lock` boolean as global-control
intent and `pad_key_lock` as bounded loaded-pad intent. Unloaded pads are saved
and restored with disabled per-pad Key Lock values. It does not store Rubber
Band handles, DLL/shared-library paths, runtime buffers, measured latency,
algorithmic delay, or callback-internal backend state.

The performer Settings UI exposes no Rubber Band backend tuning surface. Rust
uses a maximum tempo-ratio step of `0.05` every `512` active output frames in
dry and Key Lock modes. A newly accepted target initiates its first step
immediately. Rendering splits at later step boundaries so source ratios remain
independent of callback partitions; paused output does not consume an interval.
Native pitch-update order also matches with equivalent initialized native/preparation
state and reserve availability. Missing reserves retain bounded silence without
changing the canonical source feed. These values are not persisted or user-tunable.

## Realtime Constraints

The audio callback must not:

- allocate or resize DSP buffers,
- read files or decode audio,
- load plugins or models,
- log,
- block on locks or waits,
- acquire the Python GIL,
- run neural inference or stem separation,
- spin while waiting for Rubber Band output.

The callback updates scalar mode/ratio state, reads bounded per-pad Key Lock
state, reads prepared source buffers, uses fixed Rubber Band staging storage,
consumes or produces bounded FIFO data, and mixes the resulting output through
Gain/Trim, DSP, metering, and master volume.

## Native Dependency

The backend requires a Rubber Band C API that exports `rubberband_live_*`
symbols. The Windows vcpkg Rubber Band 4.0.0 package satisfies that requirement.
Ubuntu 24.04 `librubberband-dev` 3.3.0 does not provide the required LiveShifter
C API.

Build discovery uses documented platform mechanisms and explicit environment
overrides:

- Linux: `pkg-config` for a Rubber Band package with LiveShifter C API support,
  or explicit `RUBBERBAND_LIB_DIR` and `RUBBERBAND_INCLUDE_DIR` overrides.
- Windows: `RUBBERBAND_LIB_DIR`, `VCPKG_ROOT`, or the documented
  `%LOCALAPPDATA%\vcpkg` development location.
- Runtime DLL/shared-library availability is established before the audio engine
  enters realtime callback rendering.

The Windows source-run helper registers Rubber Band DLL directories before
loading the native extension. Standalone Rust test binaries do not import that
Python wrapper, so Windows Rust validation should either run with the Rubber
Band runtime directory already on `PATH` or use `scripts/run-rust-tests.ps1`,
which discovers uv's selected Python runtime plus the same documented Rubber
Band runtime locations and prepends them to `PATH` before invoking Cargo.
Packaging should provide the required runtime libraries with the application
artifact and account for Rubber Band licensing before binary distribution.

Reference URLs:

- https://breakfastquay.com/rubberband/integration.html
