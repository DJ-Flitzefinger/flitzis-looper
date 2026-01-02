## Context
Sample loading runs on a Rust background thread and reports status to Python via `LoaderEvent` and `AudioEngine.poll_loader_events()`. The UI already tracks best-effort `sample_load_progress` but does not display it on pads, and the Rust side currently emits only a single hard-coded progress update.

## Goals / Non-Goals
- Goals:
  - Provide regular progress updates during async sample loading.
  - Report a single *total* progress value across all sub-tasks in `percent` (0.0..=1.0).
  - Expose a human-readable `stage` string to the UI using a main task plus optional sub-task (e.g. `Loading (decoding)`).
  - Add a pad-local progress bar visualization and text showing `stage + percent`.
  - Show the same `stage + percent` in the selected-pad sidebar.
- Non-Goals:
  - Perfectly accurate ETA or time-based progress.
  - Changing the threading model or moving work to the audio callback.
  - Implementing future tasks (e.g. BPM detection); only ensuring the progress/event shape supports it.

## Decisions
- Progress payload shape:
  - Extend `LoaderEvent::Progress` to include both:
    - `percent: f32` (0.0..=1.0, consistent with existing code + SessionState docs)
    - `stage: String` (human-readable)
  - `stage` uses the format `"<Task>"` or `"<Task> (<Subtask>)"`.
    - Examples for current implementation: `Loading (decoding)`, `Loading (resampling)`.
    - This keeps room for additional tasks/sub-tasks later (e.g. `BPM detection`).
  - `poll_loader_events()` returns `{type: "progress", id, percent, stage}` for progress events.

- Total progress computation:
  - Use a small helper (conceptually a `ProgressReporter`) that maps sub-task-local progress into a global 0..=1.0 value using fixed weights.
  - Initial weight proposal (tunable):
    - decoding/loading: 0.45
    - resampling: 0.45
    - channel mapping + publish to ring buffer: 0.10
  - Emit progress at:
    - stage start (e.g. 0% of that stage)
    - stage end (100% of that stage)
    - periodic updates within long-running stages (where feasible), with throttling (e.g. no more than ~30–60 events/s per load).

- UI rendering approach (ImGui):
  - For pads in the loading state, render a filled rectangle behind the pad’s button area with width = `progress * pad_width`.
  - The progress bar color is derived from the pad background color but darkened slightly (e.g. multiply RGB by ~0.85–0.9).
  - The button label text for loading pads includes both stage and percentage (e.g. `"Loading (resampling) 33 %"`), alongside the filename when available.
  - The selected-pad sidebar shows the same `stage + percent` text when the selected pad is loading.

## Alternatives Considered
- Keep `Progress` as `percent` only and infer stage from percent thresholds.
  - Rejected: does not accurately communicate which task/sub-task is active and is brittle when new tasks are added.
- Report percent as 0..=100 instead of 0..=1.
  - Rejected: `percent` remains 0.0..=1.0; the UI converts to `NN %`.

## Risks / Trade-offs
- Granularity vs complexity:
  - Fine-grained progress requires chunked decoding/resampling. The proposal targets “regular” updates while keeping implementation straightforward; if existing libraries only support all-at-once operations, progress may be stepwise within some stages.
- Event volume:
  - Without throttling, emitting per-chunk progress could spam the Python UI thread. Throttling mitigates this.

## Migration Plan
- Add `stage` to `LoaderEvent::Progress` and to the Python event dict.
- Store latest `stage` in session state for pads currently loading.
- Update the pad grid UI to render progress bar and `stage + percent` text.
- Update the selected-pad sidebar UI to render `stage + percent` text.
- Ensure existing code paths that ignore extra keys continue to work.