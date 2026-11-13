# Architecture

Flitzis Looper is a desktop sample looper with six banks of 36 PADs. It plays
one-shot and looping samples, layers independently controlled PADs, schedules
quantized launches, and supports BPM Lock, Piano/transposition, Key Lock,
prepared stems and mix recording. Python owns application coordination and
saved performer intent. Rust owns current sources, output-frame time and audio.

| Reference | Read for |
| --- | --- |
| [Audio runtime and transport](architecture/runtime-and-transport.md) | Output backend, callback, scheduler, play/stop/pause/hold, render chain and feedback. |
| [Grid and source timing](architecture/grid-and-source-timing.md) | Source coordinates, precision, timing authority, markers, fractional loops and BPM Lock. |
| [Media and PCM](architecture/media-and-pcm.md) | Import, identity, disk PCM, finite residency, sharing and last readers. |
| [State, input and UI](architecture/state-input-and-ui.md) | GUI/MIDI intent, project and session state, arrangements, restore and shutdown. |
| [BPM analysis](architecture/bpm-analysis.md) | Beat This, full-source admission, scalar selection, uncertainty and manual/Tap correction. |
| [Key analysis](architecture/key-analysis.md) | Complete-file CQT/KeyNet, independent outcomes and source/correction epochs. |
| [Pitch and Key Lock](architecture/pitch-and-key-lock.md) | Piano/SET/Retrigger, nullable inheritance, Rubber Band and source history. |
| [Stems](architecture/stems.md) | Model jobs, immutable artifacts, component residency and live adoption. |

The [documentation map](README.md) links setup and usage. The
[OpenSpec contracts](../openspec/specs/) specify supported behavior.

## Runtime owners

| Owner | Entry points |
| --- | --- |
| Application and UI (Dear ImGui Bundle) | [`__main__.py`](../src/flitzis_looper/__main__.py) starts [`ui/run.py`](../src/flitzis_looper/ui/run.py). [`AppController`](../src/flitzis_looper/controller/app.py) coordinates input, loading, persistence, observations and close. |
| Native boundary (PyO3) | [`flitzis_looper_audio`](../src/flitzis_looper_audio/) imports the extension exported by [`lib.rs`](../rust/crates/looper/src/lib.rs). The [type stub](../src/flitzis_looper_audio/__init__.pyi) documents the public Python interface. |
| Native control | [`audio_engine/mod.rs`](../rust/crates/looper/src/audio_engine/mod.rs) coordinates loading, preparation, command publication and retirement outside realtime. |
| Realtime | [`audio_stream.rs`](../rust/crates/looper/src/audio_engine/audio_stream.rs), [`transport.rs`](../rust/crates/looper/src/audio_engine/transport.rs), [`scheduler.rs`](../rust/crates/looper/src/audio_engine/scheduler.rs) and [`mixer.rs`](../rust/crates/looper/src/audio_engine/mixer.rs) own the callback, timeline, scheduled effects and audio voices. |
| Offline processing | The [analysis crate](../rust/crates/analysis/), Native application analysis, [Python analysis](../src/flitzis_looper/analysis/) and [stem controller](../src/flitzis_looper/controller/stems.py) own full-source analysis and separation off the callback. |

## Control and signal paths

```text
GUI / captured input / restored performer intent
  -> controller or guarded Native input
  -> validate source, content, and requested revision
  -> prepare immutable media or history off the audio thread as needed
  -> bounded publication -> due-frame ownership check and mixer effect
  -> matching acknowledgement and observations -> controller projection

sealed original -> complete disk PCM -> admitted resident or streaming reader
  -> voice source trajectory -> full mix or prepared stems -> rate/pitch processing
  -> PAD gain, isolator and trigger velocity -> PAD meters and sum
  -> master level/mute -> master meters and mix capture -> selected stereo output
```

A PAD is a location. Content lifetime, source generation, placement, and
material identity are separate. Sharing immutable PCM leaves each voice's
progression and DSP state independent. Full-source analysis holds its own
reader and publishes source-qualified metadata.

Requested, pending, and effective state can differ. Input admission reserves an
operation. Matching Native outcomes establish whether it applied to the intended
source. Telemetry can be lossy. Replacement, cancellation, and pressure preserve
ownership until the callback, queues, workers, and last relevant readers settle.

Saved project state stores performer intent and material references. Restore
reopens sources and obtains fresh ownership and acknowledgements. Output Apply
also restores musical intent against the newly installed stream and starts in
the stopped state.

The audio callback runs bounded render, control and telemetry work. File and
JSON I/O, Python/GIL access, UI, logging, inference, plugin loading, blocking
locks, large allocations and resource destruction belong outside that path.

Performance tuning targets 2/4/6 simultaneous PADs. The 216 assignment slots
and internal 32-voice capacity serve different purposes. Synthetic performance
stress is capped at ten PADs. The [user guide](user-guide.md#memory-and-performance)
describes expected headroom and resident PCM controls.
