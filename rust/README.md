# Rust Audio Engine Module

This crate builds the native `flitzis_looper_audio` Python extension. Prepared
stem tickets and callback feedback are documented in
[prepared publication](../docs/prepared-stem-publication.md). It owns
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
    |           |-- productive_source_history.rs
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
            |-- bpm_pipeline.rs    # shared QM analysis, lossless capture and legacy projection
            |-- tempo_evidence/    # complete PCM content binding and lossless backend adapters
            |-- tempo_acceptance/  # immutable accepted timing and control-only adoption guard
            |-- tempo_refinement/  # isolated repeated PCM features and explicit count correspondence
            |-- tempo_summary/     # offline count hypotheses and robust period diagnostics
            |-- detection_function.rs
            |-- tempotrack.rs
            |-- phase_vocoder.rs
            |-- downbeat.rs
            |-- math_utils.rs
            `-- window.rs
```

Most modules are `pub(crate)`. `lib.rs`, `audio_engine/mod.rs`, and
`src/flitzis_looper_audio/__init__.pyi` define the Python-facing boundary.

The analysis crate's `tempo_acceptance` API retains a binary64 fitted period,
complete evidence and a canonical accepted revision behind explicit acceptance
and origin provenance. G3b2a's `audio_engine/constant_timing.rs` connects its
request/intent guard to actual native loaded-source/QM capture and explicit
publication, bounded callback acknowledgement and exact live SourceGrid metadata.
Remaining shared-period consumers and G3c physical/musical loop proof stay
separate; see [native adoption](../docs/native-constant-timing.md) and
[accepted timing](../docs/accepted-constant-timing.md).

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

The analysis crate's `tempo_evidence`, `tempo_refinement` and `tempo_summary`
APIs assess complete source-bound evidence offline. They have no automatic
load, manual Analyze, callback or runtime-publication caller. Their narrow
repeated-attack policy and independent-count assertion boundaries are described
in [constant-tempo candidates](../docs/constant-tempo-summary.md).
`analysis_pcm/tempo_gate.rs` is a test-only, explicitly ignored private-reference
gate. It uses the existing native complete-input resampler and independently
verified retained native PCM/Beat This evidence, with input/output paths supplied
through `G2B2_GATE_INPUT` and `G2B2_GATE_OUTPUT`. It starts no device or inference
and runs only when explicitly selected with `--ignored`.

Run these from the repository root:

```powershell
uv run maturin develop
uv run cargo check --manifest-path rust/Cargo.toml --workspace
.\scripts\run-rust-tests.ps1
uv run cargo fmt --manifest-path rust/Cargo.toml --all --check
```

Use `uv run cargo ...` so PyO3 and maturin use the project Python environment.
The helper adds uv's selected Python runtime and the documented Rubber Band
runtime directories to `PATH`, and its `purelib`/`platlib` directories to
`PYTHONPATH` for embedded PyO3/NumPy tests. It restores both variables after Cargo
returns. Direct `uv run cargo test --manifest-path rust/Cargo.toml --workspace`
also works when both native runtime libraries and the selected environment's
Python package directories are already available to standalone test executables.

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
- Key Lock setup constructs 96 unique native handles for 32 voices: the existing
  64 effective/neutral-reserve handles are warmed at setup, and 32 source reserves
  receive exact pitch/reset and actual source feed on the worker. Historical
  64-handle startup/memory measurements do not measure the extended pool.
  The third owner adds unmeasured resource cost. A shared
  `key_lock_preparation.rs` worker resets and warms recycled handles; bounded
  per-voice SPSC lanes exchange ownership without callback waits or native
  destruction. Native reset/cold pitch setup allocate in the pinned backend.
  Wet rendering uses silence if a reserve is unavailable; dry playback remains
  reactive. Pause/resume and stem source crossfades retain native history.
- G3b2f1 productive StretchProcessor fills canonical fractional feed from the actual
  borrowed source. Actual native and pending FIFO history bind source/shape/rate
  and the complete effective accepted projection with bit-exact period/origin.
  Expected next fractional position/seek mode detects discontinuities before
  consumption; bounded adapter invalidation uses existing worker recycling for
  native reset/warming. Continuous same-source timing/rate refresh retains history;
  pending/rejected timing cannot relabel it. Active voice source/timing stays pinned
  independently of bank replacement; retrigger adopts current bank PCM through
  off-realtime retirement. Warmed reserves remain source-neutral. See
  [productive timing history](../docs/native-constant-timing.md#productive-voice-and-nativefifo-history).
- G3b2f2 extends that worker with bounded productive request/result/recycle lanes.
  `prepared_native_history::NativeAdapterState` owns an actual native handle and
  its fixed FIFOs shared by live rendering and worker preparation. Current/prepared
  adapter boxes and pending-request retention allocate at setup; lanes move those
  boxes and fixed request storage without callback allocation or a large 32-voice
  inline stack aggregate. Worker catch-up owns temporary feed/output scratch. Requests pin
  actual PCM/stems, copy canonical playback/read plan and use `NativeHistoryPermit`
  for current loaded-request generation/source/rate/shared preparation epoch,
  authority/runtime revisions and exact complete accepted projection. The worker
  processes 4096 active frames including smoothing; active source transitions defer
  preparation. Rendering splits at request output frame + 4096 and transactionally
  swaps only after current source/loop/seek/stem/rate/full-trajectory rechecks and
  reserved worker recycling. Pending/failed/stale/late/unready/full-lane work keeps
  old effective audio/native history. Actual ownership/shifted-output/failure tests
  establish this native gate; G3b2g source-verifies complete native QM persistence
  with fresh existing-guard/callback adoption and distinct historical/runtime
  identities. Acoustic B5 remains separate.
  The worker checks local atomic voice cancellation before/after catch-up, and
  adoption checks the exact outstanding request ID. Stop/reset/wet deactivation
  retires source/stem pins through a separate bounded worker lane while retaining
  fenced dirty native/FIFOs. Completed-request atomics settle discarded jobs;
  inactive/paused callback polls retire ready results without source rendering,
  including a result published after cancellation. Teardown tails remain off RT.
- Productive per-pad EQ/isolator history uses that rendered voice's actual
  source/projection/trajectory. Continuous same-source timing/rate refresh retains
  fixed filter state; foreign source or discontinuity clears bounded Rust filter
  storage before replacement output. Its ledger counts actual filtered output,
  including wet fallback silence, without claiming audible source samples.
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
  is not a selected live strategy. Production builds exclude this fixture.
  G3b2f2 productive native ownership/permits/adoption/catch-up is a distinct path;
  this fixture cannot prove it. Transitions and audible compensation remain later B5 work.
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
  within a rate epoch, using the actual native binary64 ratio.
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
  metadata, until source-frame conversion. Native period/rate/BPM parameters
  preserve binary64. Long source markers retain frame
  precision; this does not imply arbitrary decimal BPM exactness.
- Waveform query/X, loaded duration, seek commands and playhead telemetry retain
  `f64` source seconds through the public boundary. Amplitudes remain `f32`.
  Source address helpers recover frame-derived bounds within a tight roundoff
  tolerance; envelope buckets use integer boundaries. Control queries stay outside
  the callback; fixed-size messages preserve existing source seek semantics.
- The pure `ScalarSourceGrid` PyO3 facade reuses `source_grid.rs` with a `f64`
  control period for editor lines, snap and automatic ends. The live constructor
  accepts native `f32` BPM for legacy state, while G3b2a acknowledged explicit
  acceptance supplies the binary64 period/origin directly. Transport/BPMLOCK
  rates are not yet migrated. See
  [scalar source coordinates](../docs/scalar-source-coordinates.md).
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
- Ordinary UI starts retain output target/order/timestamp across BPM and
  metadata updates; current execution uses the latest effective loop start.
  Source-phase mapping remains an internal foundation, with ordinary immediate
  or future-grid loop-start launches still active.
- Per-pad load and analysis work carries request identity across the PyO3
  boundary. Rust rejects stale sample publication after unload or replacement,
  and Python ignores stale progress, error, success, and analysis events.
- Rust MIDI capture runs outside the callback and receives mapping snapshots,
  native source/authority-bound runtime state, and Learn/capture state from Python.
  MIDI loop/launch is one guarded effect, checked again at quantized execution
  before loop mutation or exclusive stop/start. Full accepted revision/period/
  origin and source/authority revisions reject retired snapshots. Failed direct
  pad triggers refresh current runtime and retry through the same guarded native
  API outside the MIDI dispatcher, preserving all-or-nothing admission.
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
