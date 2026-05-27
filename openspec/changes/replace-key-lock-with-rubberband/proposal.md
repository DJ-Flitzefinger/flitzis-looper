# Change: Replace Key Lock backend with Rubber Band

## Why

The current Key Lock path uses a custom bounded delay-line pitch-compensation
stage. It fixed the master-tempo semantics, but it is not the desired
professional backend for Gen3.

The user explicitly wants the active runtime Key Lock backend replaced with
Rubber Band while preserving the realtime audio boundary:

- Key Lock off remains varispeed playback.
- Key Lock on keeps tempo changes audible while preserving perceived pitch as
  well as the Rubber Band backend allows.
- Full-mix and prepared-stem playback continue through one shared voice timing
  path.
- Multi Loop with many active voices remains bounded and callback-safe.

## What Changes

- Add a Rubber Band based Rust backend behind the existing Key Lock processing
  boundary.
- Prefer a narrow manually declared Rust FFI wrapper around Rubber Band's C API.
- Use Rubber Band's realtime-safe pitch-shifting path when it fits the existing
  mixer architecture: source-frame playback already advances by the active
  tempo ratio, so the backend only needs to compensate the resulting pitch
  shift.
- Keep Key Lock off on the cheap varispeed path.
- Remove the custom delay-line pitch-compensation algorithm from the active
  runtime path.
- Replace or remove obsolete manual delay-line tuning settings from the
  performer-facing Settings UI and Python/Rust message surface.
- Document Rubber Band latency, block-size, DLL/runtime, and callback-boundary
  requirements before treating the branch as ready for user testing.

## Non-Goals

- No per-pad Key Lock toggle.
- No per-pad Rubber Band quality controls.
- No plugin hosting, VST/LV2/CLAP/AU scanning, or external plugin lifecycle.
- No live stem separation, neural inference, decoding, or disk I/O in the audio
  callback.
- No broad DSP-chain rewrite, deck/group/master FX chain, or transport rewrite.
- No old-project migration requirement for removed custom delay-line settings;
  the user has no old projects for this branch.

## Realtime Constraints

The CPAL audio callback and realtime hot path MUST NOT allocate, resize buffers,
perform disk I/O, read or write JSON, call Python or acquire the GIL, call UI
code, block on locks or waits, log, scan or load plugins, run neural inference,
or execute unbounded loops.

Heavy setup, dependency probing, Rubber Band handle construction, block-size and
latency queries, staging-buffer allocation, runtime DLL discovery, and error
reporting setup must happen outside the callback.

## Impact

- Affected specs: `time-stretch-pitch-shift`, `play-samples`,
  `ring-buffer-messaging`, `performance-ui`, `project-persistence`.
- Affected docs: `docs/architecture.md`, `docs/key-lock-backend.md`.
- Affected code: Rust audio-engine Key Lock wrapper, voice lifecycle/mixer
  integration, Rust build/link setup, Python Settings/project-state plumbing,
  Python type stubs, and focused Rust/Python tests.
