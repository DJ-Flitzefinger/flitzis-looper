# Audio Engine

Flitzis Looper uses a Rust-based audio engine to meet real-time constraints while keeping the Python side focused on orchestration.
Python interacts with a single `AudioEngine` object; the CPAL audio callback renders audio without acquiring the Python GIL.

```text
+---------------------+
|   Flitzis Looper    |
|      (Python)       |
+----------+----------+
           |
           v
+---------------------+
|    AudioEngine      |
| (Rust, cpal, PyO3)  |
+----------+----------+
           |
           v
+---------------------+
|  System Audio API   |
| (ALSA/PulseAudio)   |
+---------------------+
```

## High-level flow

1. Python creates an `AudioEngine` instance and calls `run()`.
2. `run()` initializes a CPAL output stream and two ring buffers (control→audio, audio→control).
3. Python schedules audio loading into sample slots using `load_sample_async(id, path)` and polls `poll_loader_events()`.
4. Python triggers playback using `play_sample(id, velocity)`.
5. Optional MIDI input mapping is captured by a Rust input layer outside the audio callback,
   timestamped immediately, normalized, resolved from an in-memory mapping snapshot, and then
   bridged to the existing bounded control-command path where possible.
6. The CPAL callback drains pending control messages, updates bounded per-pad timing metadata, routes play/stop commands through the Rust scheduler, mixes active voices into the output buffer, and advances the Rust transport timeline by rendered output frames.
7. Optional: Python can poll `receive_msg()` for messages emitted by the audio thread (e.g., `Pong`).

## Module Structure

The audio engine is organized into modular components following the single responsibility principle:

```
audio_engine/
├── mod.rs              # Main orchestration, re-exports, public API
├── constants.rs        # Configuration constants (NUM_BANKS, GRID_SIZE, etc.)
├── errors.rs           # Error types (SampleLoadError)
├── voice.rs            # Voice struct and lifecycle management
├── mixer.rs            # RtMixer implementation with real-time rendering
├── sample_loader.rs    # Audio file decoding and channel mapping
└── audio_stream.rs     # CPAL stream management and callback setup
```

Each module is `pub(crate)` with only `mod.rs` exposing the public API, ensuring clear encapsulation and reducing coupling between components.

The Gen3 transport helper lives in `rust/src/audio_engine/transport.rs`; the fixed-capacity
absolute-frame scheduler helper lives in `rust/src/audio_engine/scheduler.rs` and is owned by the
CPAL callback.

The low-jitter input mapping helper lives in `rust/src/audio_engine/input_mapping.rs`. It owns
MIDI capture, timestamping, filtering, in-memory mapping lookup, and dispatch bridging outside the
CPAL callback.

## Main components

- Python (control layer)
  - Owns the `AudioEngine` instance and calls its methods.
  - Schedules potentially blocking work (disk I/O, decoding) via the Rust engine.

- Rust (real-time audio layer)
  - `rust/src/audio_engine/mod.rs`: Main orchestration and Python-facing API.
  - `rust/src/audio_engine/constants.rs`: Configuration constants and limits.
  - `rust/src/audio_engine/errors.rs`: Audio-specific error types.
  - `rust/src/audio_engine/voice.rs`: Voice management and lifecycle.
  - `rust/src/audio_engine/mixer.rs`: Real-time mixer implementation.
  - `rust/src/audio_engine/input_mapping.rs`: MIDI capture, timestamping, filtering, in-memory
    mapping lookup, and dispatch bridging outside the audio callback.
  - `rust/src/audio_engine/stretch_processor.rs`: Per-voice bounded speed/key-lock DSP wrapper.
  - `rust/src/audio_engine/transport.rs`: Audio-thread-owned output-frame clock and musical phase helpers.
  - `rust/src/audio_engine/scheduler.rs`: Fixed-capacity absolute output-frame scheduler for playback events.
  - `rust/src/audio_engine/sample_loader.rs`: Audio file decoding and channel mapping.
  - `rust/src/audio_engine/audio_stream.rs`: CPAL stream management and callback.
  - `rust/src/messages.rs`: Fixed-size message types shared between threads.
  - Dependencies:
    - `cpal` for the audio callback/stream.
    - `midir` for MIDI input callbacks outside the audio callback.
    - `rtrb` for SPSC ring buffers.
    - `symphonia` for decoding audio files.

## Speed, BPM Lock, and Key Lock DSP

Global Pitch/Speed and BPM Lock both resolve to one per-voice tempo ratio in the Rust mixer:

- With BPM Lock disabled, the ratio is the global speed multiplier.
- With BPM Lock enabled and valid master/pad BPM metadata, the ratio is `master_bpm / pad_bpm`.
- Pads without valid BPM metadata fall back to the global speed multiplier.

Key Lock selects how that tempo ratio is rendered:

- Key Lock disabled uses varispeed semantics. The source playhead advances by the tempo ratio, and
  perceived pitch follows the speed change.
- Key Lock enabled uses the master-tempo path. The source playhead still advances by the tempo
  ratio, but a bounded per-voice pitch-compensation stage reduces the pitch movement caused by
  varispeed playback.

Key Lock DSP parameters are persisted as one bounded global parameter set. New projects default to
the former `High` baseline:

- delay minimum: `64` samples,
- delay range: `1536` samples,
- delay heads: `2`,
- interpolation: `cubic`,
- window: `hann`,
- smoothing step: `0.05`,
- output gain: `1.0`.

The supported manual Settings ranges are delay minimum `16..512` samples, delay range
`256..1984` samples, combined delay minimum plus range at most `2032` samples, head count `2..4`,
interpolation `linear` or `cubic`, window `triangle` or `hann`, smoothing step `0.01..0.10`, and
output gain `0.25..2.0`. Legacy Key Lock quality values remain compatibility aliases that map to
concrete parameter sets.

The current implementation is intentionally held inside `stretch_processor.rs` behind a narrow
wrapper so a future pro-grade backend can replace the internal algorithm. The wrapper owns fixed
per-channel input, intermediate/output, and delay-line buffers that are constructed with the voice
slots before the CPAL callback runs. Rendering reuses that memory; it does not resize buffers,
read files, load plugins, call Python, block, log, or allocate audio payloads in the callback.
Changing Key Lock parameters sends only bounded scalar state to Rust and does not retrigger active
voices.

Prepared stems and full-mix playback share the same source-frame reader before Key Lock
processing. Stem mask/mix changes therefore preserve the same loop playhead, BPM-lock ratio,
Key-Lock mode, gain/EQ, metering, and playhead reporting path as full-mix playback.

## Threading and real-time constraints

The project deliberately separates non-real-time work from the real-time audio callback:

- The audio callback MUST avoid blocking, disk I/O, heap allocations, logging, and Python/GIL interaction.
- Inter-thread communication uses fixed-capacity ring buffers (1024 messages).
- MIDI input ports, keyboard input, mapping JSON files, and Learn UI state are outside the audio
  callback. Rust input/control modules may own MIDI callbacks and bounded queues, but they may
  only affect playback by sending bounded control messages through the established control path.
- Sample playback is designed for predictable performance:
  - `MAX_SAMPLE_SLOTS` fixed sample slots addressed by `id` (`0..n`).
  - `MAX_VOICES` fixed polyphony; additional triggers are dropped deterministically.
- Sample decoding happens outside the callback. Decoded sample data is published to the audio thread via a shared handle (no full-buffer copies just for cross-thread transfer).

## Gen3 transport timeline plan

The first Gen3 behavior change is specified in
`openspec/changes/add-rust-transport-timeline/`. The initial transport timeline module now
exists and is owned by the audio callback. The fixed-capacity scheduler module now exists
with deterministic unit tests, and the audio callback owns a scheduler for current-frame
playback integration.

Implemented first slice:

- `TransportTimeline` stores the absolute output-frame clock, output sample rate, default
  transport master BPM, and downbeat anchor.
- The CPAL callback advances the transport by the number of rendered output frames.
- Rust validates transport master BPM and derives beat/bar phase for 4/4 timing.
- Deterministic Rust unit tests cover frame advancement, BPM conversion, phase, grid
  boundary calculations, and invalid BPM fallback.
- `FixedCapacityScheduler` stores accepted events in bounded array storage, orders by
  absolute output frame, preserves same-frame insertion order, drains late events at the
  current callback start frame, and rejects new events when full without eviction.
- Scheduler tests cover named capacity, target-frame ordering, same-frame stable ordering,
  late events, in-buffer target execution frames, and full-capacity rejection.
- The CPAL callback owns `TransportScheduler` and routes `PlaySample`, `StopSample`, and
  `StopAll` through current-frame scheduled commands while preserving the public immediate
  `play_sample` behavior.
- Scheduled events inside an output buffer split rendering at the target frame so starts and
  stops occur at the intended sample-frame offset.
- `AudioEngine.set_trigger_quantization(mode)` publishes a fixed-size trigger-quantization
  update to the audio thread. The controller uses `"immediate"` while trigger quantization is
  disabled and grid-step strings `"1_64"`, `"1_32"`, or `"1_16"` while it is enabled. The
  persisted Settings default is `"1_64"`, but new projects keep the bottom-bar `Q` toggle
  disabled so playback remains immediate by default. Legacy `"next_beat"` and `"next_bar"`
  inputs remain accepted as compatibility aliases for `"1_16"`.
- `AudioEngine.set_pad_timing_metadata(id, phase_anchor_s)` publishes bounded per-pad
  beatgrid/downbeat timing metadata prepared outside the audio callback. The Python controller
  derives `phase_anchor_s` from the same `grid_anchor_sec` that the waveform editor draws and
  snaps against: first finite non-negative downbeat, then first finite non-negative beat, then
  `0.0`, plus the per-pad grid offset in samples. Unloaded pads do not publish stale grid anchors
  to Rust, and published anchors are bounded to non-negative seconds before crossing the native API.
- When trigger quantization is enabled, Rust computes the current or next future selected-grid
  target frame from the permanent `TransportTimeline`, the downbeat anchor, and the selected grid
  step, then schedules `PlaySample` at that absolute output frame. Quantization only changes when
  the pad becomes audible in output time; every newly triggered pad starts from its effective Loop
  Editor loop start, or sample start when no loop region exists. Rust does not compensate late
  clicks by starting inside the loop and does not establish the masterclock from active pads.
- Scheduler-full quantized play requests are rejected without evicting existing scheduled events
  or changing currently playing pads.
- `AudioEngine.play_sample_exclusive(id, velocity)` publishes one fixed-size command for
  one-at-a-time playback. With quantization enabled, Rust schedules the stop-all operation and
  requested pad start as one atomic `StopAllThenPlaySample` event at the same absolute output
  frame; scheduler-full rejection leaves current playback unchanged.
- The corrected `add-phase-aware-playback-sync` slice keeps `TransportTimeline` target-frame phase
  helpers and `RtMixer` phase helper coverage available for explicit sync behavior, but normal
  quantized `PlaySample` and `PlaySampleExclusive` events no longer carry a source-frame phase
  descriptor and no longer seek into the source loop.
- Immediate playback commands carry no phase descriptor, so `play_sample` and
  `play_sample_exclusive` keep the existing prompt loop-start behavior when trigger quantization
  is disabled.
- `AudioEngine.anchor_transport_phase_from_pad(id)` publishes a fixed-size explicit phase-anchor
  request. When the selected pad is active and has valid BPM/timing metadata, the audio thread
  derives the pad's current bar phase from mixer state and moves the Rust transport downbeat anchor
  to the matching phase while setting the transport BPM from the active pad output tempo when
  needed. If the pad is inactive, paused, or missing BPM/timing metadata, the transport downbeat is
  left unchanged and existing BPM-ratio tempo matching continues.

The planned direction is:

- Rust owns the global transport timeline and advances it by rendered output sample frames.
- The global transport timeline runs independently of active pad count and trigger quantization
  state.
- Scheduled playback events target absolute output-frame positions.
- Rust stores transport master BPM and derives beat/bar phase from the audio-thread sample-frame
  clock.
- Quantized pad triggers use a fixed-capacity scheduler owned by the audio thread.
- Existing immediate trigger behavior remains the default when trigger quantization is disabled.
- Quantized pad triggers preserve source-side loop starts and manual musical offsets between pads.
- Beatgrid and downbeat metadata is prepared outside the audio callback, then published as
  bounded per-pad timing metadata for Rust playback timing.

The audio callback must remain real-time safe for this work. It must not perform disk I/O,
Python/GIL access, blocking waits or locks, logging, heavy allocations, neural network
inference, real-time stem separation, or long-running work.

Later Gen3 stem work must be offline/cache-based. Stem generation is only allowed for pads
that are not currently playing; the audio callback may only mix already prepared audio data.

The active Gen3 stem planning slice is `openspec/changes/add-offline-stem-cache/`. It defines
offline/background stem generation, project-local stem cache identity, inactive-pad gating,
fixed-size publication of prepared immutable stem buffers, and future real-time-safe stem mixing
using the same voice playhead and loop timing as full-mix playback. It does not implement neural
inference, stem UI, or mixer stem toggles.

The performer-facing stem control planning slice is
`openspec/changes/add-stem-performance-controls/`. It defines stem availability indicators,
selected-pad generation entry points, explicit full-mix versus all-stems mode selection, future
per-stem mute/solo/toggle controls, persistence expectations, and fixed-size audio-thread stem mix
control messages. This planning slice does not implement UI controls, production source
separation, neural model integration, or new mixer control behavior.

The current stem implementation defines Python-side project metadata for a pad-scoped
project-local `samples/stems/#<pad-number>/` cache layout, using a source-version token derived
from the cached source path plus file metadata. The controller rejects stem generation requests
for pads that are playing, loading, analyzing, already generating stems, missing a loaded source,
or missing the cached source file. The selected-pad sidebar now renders controller/session stem
status, routes Generate Stems and Delete Stems through controller actions, and exposes the durable
full-mix/all-stems mode control without inspecting cache directories in the render loop. Unload
Audio deletes the tracked pad stem cache outside the audio callback.

Production stem generation now runs through a replaceable Python-side backend boundary. The first
backend adapter invokes Demucs from a background worker, with Torch/Demucs work kept out of app
startup and out of Rust. The backend request carries the source path, pad id, source version,
project-local cache directory, loaded sample shape, model cache directory, and device policy. The
Demucs adapter maps `other.wav` to project `melody.wav`, derives `instrumental.wav` by summing the
final aligned drums, bass, and melody artifacts, and postprocesses final cache WAV files so they
match the loaded full-mix sample rate, channel count, and frame count before Rust publication.
User-facing setup commands for FFmpeg and the Demucs model are documented in
`docs/stem-generation-setup.md`.
Demucs is declared as a runtime dependency of the application so a freshly synced environment can
start the backend. Model download is intentionally not started from the Looper UI: the performer
must install the Demucs model ahead of time, and the backend reports the short error
`no Model installed` when the expected local checkpoint is missing. If the active Python
environment is not synced or is otherwise missing Demucs/Torch, generation reports a normal
background-task error and full-mix playback remains available.
Demucs also requires working `ffprobe` and `ffmpeg` executables for audio probing/decoding. The
backend checks those tools before invoking Demucs and reports `FFmpeg/ffprobe unavailable` when
Windows cannot run them. Tool lookup uses the process `PATH`, an explicit `FLITZIS_FFMPEG_DIR`
directory, or a local WinGet `Gyan.FFmpeg*` package install, and prepends the resolved directory
to the Demucs subprocess environment. Because the installed Torchaudio save path uses TorchCodec,
the backend also checks TorchCodec before invoking Demucs and reports `TorchCodec unavailable` if
its native libraries cannot load.
The first production backend uses Demucs defaults of `--shifts 4` and
`--overlap 0.5`. Those values are bounded request parameters (`shifts` 1 through 20 and
`overlap` 0.25 through 0.95). The Settings page persists validated replacements in project state,
and `StemController.generate_stems_async(...)` copies them into the file/artifact backend request.
`AudioEngine.publish_prepared_stems(id, source_version, cache_dir)` validates those cached WAV
artifacts against the currently loaded full-mix buffer outside the audio callback, then publishes
shared immutable prepared-stem handles to Rust through a fixed-size control message.

Demucs model files are kept in the standard Torch Hub checkpoint cache, outside the repository and
outside project samples. On Windows this is
`C:\Users\<YOUR_NAME>\.cache\torch\hub\checkpoints`; the default `htdemucs` checkpoint expected by
the Looper is `955717e8-8726e21a.th`. GPU acceleration is optional. CPU separation is supported
but slower. Windows GPU acceleration requires an NVIDIA GPU, a compatible/current NVIDIA driver,
and a CUDA-enabled PyTorch/Torchaudio build; the separate CUDA Toolkit is not expected for normal
packaged PyTorch use. If the CUDA path fails, the background worker retries on CPU when CPU
processing can proceed.

The low-level Rust API still exposes a deterministic `generate_stems_async(id, source_version,
cache_dir)` cache writer for engine-level validation. It is not the production separation path.
The audio callback accepts prepared handles only for loaded inactive pads and stores them in
bounded per-pad/per-stem state. The first performer-control implementation slice adds a durable
per-pad `full_mix`/`all_stems` preference with `full_mix` as the default for new and older
projects. Rust stores that preference as bounded audio-thread state updated by fixed-size control
messages, and prepared stems are used only when `all_stems` is selected and the accepted prepared
set matches the requested source-version hash. Missing, stale, incomplete, rejected, or disabled
stems fall back to the loaded full-mix buffer. The next performer-control slice added a selected-pad
bottom-bar `V`/`D`/`M`/`B`/`I`/`A` mask control: `V`, `D`, `M`, and `B` toggle component stems,
`I` selects Drums + Melody + Bass, and `A` selects Vocals + Drums + Melody + Bass. Component clicks
from `I` or `A` enter custom mode with only the clicked component active, while custom masks that
match a preset remain custom until the performer explicitly clicks that preset. `I` and `A` share
one exclusive preset group: entering a preset remembers the last `V`/`D`/`M`/`B` component mask,
switching between presets preserves that remembered mask, and clicking the active preset again
returns to it. The cached `instrumental.wav` artifact is not used as the `I` preset or as an extra
layer in `A`. The pad grid now renders compact stem status badges for available, generating,
blocked, and error states from controller/session snapshots only. Production source separation is
now provided by the background Demucs backend. A bottom-right Settings overlay exposes manual
Key Lock DSP parameters plus Demucs shifts and overlap controls, stores them as project settings,
and leaves stem
generation, model lookup, and cache work on the existing background path. Right-clicking `V`, `D`, `M`, or `B`
sets a non-momentary custom solo state for that component without adding a separate mute feature.

The active Gen3 phase-aware sync slice is `openspec/changes/add-phase-aware-playback-sync/`. It now
keeps normal quantized starts loop-start based while preserving bounded transport/pad phase helpers
for explicit sync behavior. BPM-lock tempo matching remains separate from the permanent transport
masterclock unless an explicit `anchor_transport_phase_from_pad` request is sent.

## Gen3 low-jitter input mapping

The active low-jitter input mapping slice is
`openspec/changes/add-low-jitter-input-mapping/`. It keeps Python in charge of Settings, Learn
state, keyboard focus rules, and mapping-file edits, while moving the MIDI hot path into Rust
outside the CPAL callback.

Implemented first slice:

- The `L` Learn workflow is preserved: `L -> input -> learnable action` saves a mapping, and
  `L -> input -> L` deletes that input's existing mapping.
- Learnable control coverage includes Tap BPM, the bottom-bar selected-pad `V`/`D`/`M`/`B`/`I`/`A`
  stem mask buttons, per-pad Gain, per-pad EQ bands, Master Volume, and the global Speed/Pitch
  control. Keyboard and MIDI Note mappings to continuous controls save bounded set-value actions
  from the selected UI value. MIDI CC and NRPN increment/decrement mappings to those controls save
  relative-step actions: the first value establishes a baseline, then later encoder movement
  applies one controller-owned increment or decrement outside the audio callback. The relative path
  handles endless-controller 0..127 wraparound, repeated relative encoder values such as `1`/`127`
  or `65`/`63`, and NRPN Data Increment/Data Decrement messages normalized to stable
  `midi:nrpn:<channel>:<parameter>` bindings. The controller-side setters clamp at the existing
  target limits. Global Speed/Pitch relative steps use the displayed-BPM reference when available,
  so each relative movement targets a 0.1 BPM change and is converted back to the bounded speed
  multiplier before reaching Rust; without a BPM reference, the existing bounded multiplier step
  remains the fallback. Hardware endless knobs should be configured for relative/inc-dec output;
  absolute CC mode still reports a finite device-side 0..127 position. MIDI values still do not
  become audio-callback parameter streams.
- Keyboard mappings retain key-plus-modifier bindings, do not execute while text input is
  focused, and remain the responsiveness reference for mapped dispatch.
- MIDI Note On velocity greater than zero, Control Change, and NRPN increment/decrement are
  normalized to stable binding keys such as `midi:note:1:60`, `midi:cc:1:7`, and
  `midi:nrpn:1:0`.
- Note On velocity zero, Active Sensing, MIDI Clock, SysEx, Program Change, Pitch Bend,
  Aftertouch, and MPE-style messages are ignored for version 1.
- Incoming MIDI events are stamped with a monotonic timestamp immediately in the Rust MIDI
  callback and placed onto a bounded channel without Python, JSON, UI updates, or logging.
- Normal mapped MIDI playback resolves against an in-memory snapshot published by Python after
  Learn/save/delete/clear-all changes. The hot path does not read or write JSON.
- Direct audio-safe actions such as pad trigger, pad stop, and stop-all are bridged to the
  existing bounded control-command path. Other mapped actions are reported to Python for
  controller-owned execution outside the hot path.
- The Settings page exposes Input Mapping ON/OFF plus Delete all Keyboard Mappings and Delete
  all MIDI Mappings.

The Rust MIDI input layer is not part of the audio callback. The audio callback must never handle
MIDI ports, keyboard polling, Learn state, mapping JSON, Python/GIL access, blocking locks,
logging, neural inference, or unbounded work.

## Current Python API surface

The Rust engine is exposed to Python as `AudioEngine` with:

- Lifecycle
  - `AudioEngine()`
  - `run()`
  - `shut_down()`

- Sample workflow
  - `load_sample_async(id, path)` schedules loading on a Rust background thread.
  - `poll_loader_events()` polls for background loader events (e.g. started/success/error).
  - `play_sample(id, velocity)` triggers playback (`velocity` in `0.0..=1.0`).
  - `play_sample_exclusive(id, velocity)` stops all active voices and starts the requested loaded
    sample as one audio-thread command. The controller uses this for MultiLoop-disabled playback.
  - `set_trigger_quantization(mode)` sets low-level Rust trigger quantization mode. Supported
    modes are `"immediate"` plus grid steps `"1_16"`, `"1_32"`, and `"1_64"`. Legacy
    `"next_beat"` and `"next_bar"` aliases remain accepted as `"1_16"`.
    The Python controller persists `trigger_quantization_enabled` separately from
    `trigger_quantization_step`; the bottom-bar `Q` toggle enables/disables quantization and the
    Settings page selects the grid step.
  - `set_pad_timing_metadata(id, phase_anchor_s)` publishes a finite non-negative per-pad phase
    anchor derived from analysis metadata. It is stored in Rust state for pad timing metadata and
    explicit sync behavior; full beat-grid vectors are not sent to the callback.
  - `anchor_transport_phase_from_pad(id)` explicitly requests transport downbeat anchoring from a
    selected playing pad using only audio-thread-owned transport and mixer state.
  - `set_input_mapping_enabled(enabled)` turns the Rust input layer's mapping resolution on or off.
  - `set_input_mapping_snapshot(mappings)` publishes `(binding_key, action_key)` MIDI mappings to
    Rust for in-memory lookup.
  - `set_input_runtime_state(multi_loop, loaded, loop_starts, loop_ends)` publishes the bounded
    pad runtime state needed for direct Rust dispatch.
  - `start_midi_input()` opens available MIDI input ports through the Rust input layer.
  - `stop_midi_input()` closes those MIDI input connections.
  - `poll_input_events()` polls normalized MIDI input events, including MIDI value, for Learn UI
    and diagnostics.
  - `inject_midi_input_for_test(message)` injects a MIDI message into the same normalization path
    for hardware-free bridge tests.

- Messaging utilities
  - `ping()` sends a ping to the audio thread.
  - `receive_msg()` polls for an `AudioMessage` from the audio thread and returns `None` when no message is available.

## Not implemented (yet)

- Audio device selection/configuration (the engine currently uses the default output device/config).
- Broader channel-layout support; currently decoding only supports mono↔stereo mapping.
- Real-time stem separation is intentionally out of scope.
- Offline stem cache identity, request gating, deterministic cache artifact writing, prepared
  stem-buffer validation/publication, prepared-stem rendering fallback infrastructure, durable
  full-mix/all-stems mode plumbing, selected-pad stem status, Generate Stems button wiring,
  selected-pad full-mix/all-stems controls, bottom-bar selected-pad per-stem mask controls, and
  pad-grid stem indicators are implemented. Production Demucs source separation is implemented
  behind a replaceable backend boundary, and the Settings overlay now exposes the bounded Demucs
  quality parameters. The selected-pad Delete Stems action, automatic stem deletion on Unload
  Audio, and component right-click solo setter are implemented; no separate stem mute feature is
  planned for the current Gen3 direction.

## Related specs

- `openspec/specs/minimal-audio-engine/spec.md`
- `openspec/specs/ring-buffer-messaging/spec.md`
- `openspec/specs/load-audio-files/spec.md`
- `openspec/specs/play-samples/spec.md`
- `openspec/changes/add-rust-transport-timeline/`
- `openspec/changes/add-phase-aware-playback-sync/`
- `openspec/changes/add-offline-stem-cache/`
- `openspec/changes/add-low-jitter-input-mapping/`
