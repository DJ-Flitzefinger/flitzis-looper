# Rust Audio Engine Module

This crate builds the native `flitzis_looper_audio` Python extension. It owns
the realtime audio path and exposes the PyO3 `AudioEngine` class used by the
Python application package.

The Python package wrapper lives in:

```text
src/flitzis_looper_audio/
```

`maturin develop` builds the platform extension into that wrapper package.

## Module Structure

```text
rust/
|-- Cargo.toml                     # virtual workspace
`-- crates/
    |-- looper/                    # flitzis-looper (PyO3 + audio engine)
    |   |-- Cargo.toml
    |   |-- build.rs
    |   `-- src/
    |       |-- lib.rs             # PyO3 module export
    |       |-- messages.rs        # fixed-size command/parameter/telemetry types
    |       `-- audio_engine/
    |           |-- mod.rs         # AudioEngine API and background orchestration
    |           |-- analysis_jobs.rs # optional diagnostic request/retirement ownership
    |           |-- analysis_pcm.rs  # immutable source, shared mono, export and key input
    |           |-- analysis_predictions.rs # bounded lossless diagnostic envelope validation
    |           |-- audio_stream.rs
    |           |-- buffer_retirement.rs
    |           |-- constants.rs
    |           |-- dsp.rs
    |           |-- input_mapping.rs
    |           |-- key_lock_preparation.rs
    |           |-- mixer.rs
    |           |-- scheduler.rs
    |           |-- source_grid.rs
    |           |-- source_playback.rs
    |           |-- source_reader.rs
    |           |-- timing.rs
    |           |-- transport.rs
    |           |-- voice_slot.rs
    |           |-- stretch_processor.rs
    |           |-- rubberband_backend.rs
    |           |-- sample_loader.rs
    |           |-- stem_cache.rs
    |           |-- progress.rs
    |           |-- channels.rs
    |           `-- errors.rs
    `-- analysis/                  # flitzis-looper-analysis (BPM pipeline)
        |-- Cargo.toml
        `-- src/
            |-- lib.rs
            |-- detection_function.rs
            |-- tempotrack.rs
            |-- phase_vocoder.rs
            |-- downbeat.rs
            |-- math_utils.rs
            `-- window.rs
```

Most modules are `pub(crate)`. `lib.rs`, `audio_engine/mod.rs`, and
`src/flitzis_looper_audio/__init__.pyi` define the Python-facing boundary.

## Runtime Path

```text
Python controllers
-> AudioEngine PyO3 methods
-> bounded command ring + bounded parameter ring
-> CPAL callback
-> TransportTimeline + TransportScheduler
-> RtMixer
-> canonical fractional source progression
-> source selection, loop wrap, playback-rate / Key Lock
-> smoothed per-pad Gain/Trim
-> per-pad DSP chain
-> trigger velocity / master volume / metering
-> output buffer
```

The callback must not perform disk I/O, JSON access, Python/GIL work, UI work,
logging, plugin loading, neural inference, blocking waits, unbounded loops, or
heavy allocation.

## Development Commands

The optional `AudioEngine.begin_offline_analysis` boundary pins loaded PCM for
the Python diagnostic supervisor. It retains one native reservation per engine
until preparation/key work and resource retirement finish. All export,
resampling, key inference and JSON event publication run outside the callback;
the existing automatic/manual analyzer remains selected. See
[Offline analysis boundary](../docs/offline-analysis.md) for limits, terminal
component semantics and remaining Beat This setup/inference work.

Run these from the repository root:

```powershell
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml --workspace
.\scripts\run-rust-tests.ps1
uv run cargo fmt --manifest-path rust/Cargo.toml --all --check
```

Use `uv run cargo ...` so PyO3 and maturin use the project Python environment.
The Windows script adds uv's selected Python runtime and the documented Rubber
Band runtime directories to `PATH` before launching the standalone Rust test
executable.
On non-Windows platforms, or in a Windows shell where the Rubber Band runtime
DLLs are already visible to standalone test executables, the Rust test command
is `uv run cargo test --manifest-path rust/Cargo.toml --workspace`.

## Design Notes

- Rust owns live audio truth: transport, scheduler, mixer, loaded buffers,
  source playheads, prepared-stem selection, realtime parameter application,
  smoothed dB Gain/Trim, metering, and DSP state.
- Python owns UI, durable project intent, persistence, settings, mapping edit
  UX, and offline/background orchestration.
- Ordered commands and high-rate scalar parameters use separate bounded queues.
- PyO3 setters for must-apply command and parameter publications report full
  queues as caller-visible `RuntimeError`s instead of silently accepting the
  write.
- Key Lock setup warms 64 unique native handles for 32 voices. A shared
  `key_lock_preparation.rs` worker resets and warms recycled handles; bounded
  per-voice SPSC lanes exchange ownership without callback waits or native
  destruction. Native reset/cold pitch setup allocate in the pinned backend.
  Wet rendering uses silence if a reserve is unavailable; dry playback remains
  reactive. Pause/resume and stem source crossfades retain native history.
- The wet adapter has a fixed 511-frame lead at the 512-frame block size.
  Algorithmic/adapter delay remains uncompensated and native nominal delay is
  distinct from measured transient peaks. Source pre-roll, DSP feed-ahead, and
  click-safe wet/bypass transitions (including ratio 1.0 and toggles) remain
  pending. Existing clock, markers, source playheads, and launch policy are
  preserved. The release `key_lock_latency_probe` example measures synthetic
  responses and preparation costs without a device; run it using the Windows
  DLL override in [the development guide](../docs/development.md#offline-key-lock-measurement).
- Native tests compile `key_lock_source_preparation.rs` as a non-live exact-source
  proof with separate logical/feed cursors and coherent native/FIFO continuation.
  An independent algebraic source/raw-native reference verifies output at explicit
  discard indices; release CSV metrics expose discarded startup peaks and uncropped
  residuals. Optional explicit forward source history must reach the requested fractional
  logical phase; raw discard and history remain distinct. Isolated impulse/tone/percussion
  sweeps assess stereo energy timing, clipping and cut/join continuity before compensation.
  Longer-history silent/nonzero-content probes measure translation-invariant onset
  deformation and one fixed offline dry-to-wet bridge. Native clipping, nonlinear
  background sensitivity and temporary varispeed pitch remain visible; that bridge
  is not a selected live strategy. Production builds exclude this fixture. Timed live
  adoption, identity, retirement, transitions and audible compensation remain pending.
- `key_lock_source_causal_probe.rs` is a test-only fixed-case proof of strict versus hypothetical
  early emission against identical continuous source/native histories. It checks canonical phase,
  exact retained continuation and disjoint missing-content intervals. Nominal translation remains
  illustrative; omitted mixture context is not isolated attack retention, and the audible-start
  product decision remains pending. Reproduction is in the development guide.
- Parameter messages are coalesced by identity in the callback before applying
  the latest drained value. The callback applies only identities touched by the
  drained batch instead of sweeping every pad slot.
- Pad peak/playhead telemetry is cadence-gated and published only for pads
  touched by rendering in that callback; inactive bank slots are not scanned for
  telemetry.
- Scheduled mixer segments carry absolute output-frame positions. Every active
  voice derives fractional source progress from its active-output-frame count
  within a rate epoch, using the actual native `f32` ratio promoted to `f64`.
  This avoids cumulative segment rounding in all lock modes.
- `source_grid.rs` owns signed source beat/bar, loop-start phase and internal
  master-beat-to-loop mapping. The editor origin is published as `f64` seconds
  and retained as a signed virtual source frame. Compatible tick periods divide
  one bar or span whole bars, within one frame of rounding; musical wrapping
  precedes source rounding to avoid cycle drift. Unsupported physical loops
  retain bounded wrapping without a sustained synchronization claim.
- `source_reader.rs` shares effective loop bounds, explicit seek progression, full-mix/stem
  validation, integer source reads and source-selection crossfades. Voice state and mixer
  rendering use the same policy. `source_playback.rs` adds scalar fractional epochs
  and fixed active-frame ratio smoothing; the reader provides linear two-tap reads.
  Both taps obey the
  loop/intro/tail seek policy; source-selection ramps advance by fractional source
  distance. `StretchProcessor` receives the canonical already-resampled feed directly.
  These modules do not own native DSP state or perform source pre-roll.
  The maximum per-voice ratio step remains `0.05`, now every `512` active output frames
  in dry and Key Lock modes, with the first step at a newly accepted target. Render
  work splits at rate boundaries; pause freezes source and smoothing progress.
- Signed grid and loop-region seconds use `f64`, including MIDI runtime loop
  metadata, until source-frame conversion. Pad/master BPM remain native `f32`
  parameters promoted to `f64` for phase math. Long source markers retain frame
  precision; this does not imply arbitrary decimal BPM exactness.
- Transport stores complete musical position across BPM changes. The dedicated
  `bootstrap_transport_from_pad(id)` request latches the selected BPMLOCK
  reference once per stream, waits for valid active source state and consumes
  the opportunity on success or deliberate explicit sync. Silence cannot rearm
  it. Active source playheads and the monotonic output-frame clock are preserved.
- Rate changes and in-range loop edits preserve fractional source carry.
  Out-of-range edits retain the loop-start clamp. Bootstrap configures a copy of
  the canonical cursor against the current effective loop and maps frame plus
  fractional remainder to beats. It matches rendering after loop/seek normalization
  without advancing the live cursor.
- Already accepted starts retain output target/order/timestamp across BPM and
  metadata updates; current execution uses the latest effective loop start.
  Source-phase mapping remains an internal foundation, with ordinary immediate
  or future-grid loop-start launches still active.
- Per-pad load and analysis work carries request identity across the PyO3
  boundary. Rust rejects stale sample publication after unload or replacement,
  and Python ignores stale progress, error, success, and analysis events.
- Rust MIDI capture runs outside the callback and receives mapping snapshots,
  input-runtime pad state, and Learn/capture state from Python. Direct MIDI
  command dispatch is all-or-nothing; failed direct attempts are reported back
  to Python for controller-owned fallback outside the MIDI dispatcher.
- `timing.rs` owns the shared engine monotonic epoch, coherent bounded clock
  observations and captured-input nearest-grid diagnostic math. MIDI/UI launch
  time survives fixed-size commands and scheduled events. Callback publication
  uses atomics without locks or retry loops; readers attempt one coherent read.
  `AudioEngine.output_clock_snapshot()` reports `valid`/`fresh` and estimated
  frame/time/rate/grid metadata; `input_clock_target_frame(...)` returns no target
  for unavailable mapping. Current launch behavior is unchanged. CPAL's WASAPI
  delay estimate and unmeasured DSP delay do not establish hardware accuracy.
- Sample and prepared-stem handles removed from callback-owned state are retired
  through a bounded non-audio worker to avoid large final drops on the audio
  thread.

See `../docs/architecture.md` for the full architecture reference.
