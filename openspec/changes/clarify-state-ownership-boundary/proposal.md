# Change: Clarify state ownership boundary

## Why

The architecture audit found that durable Python project state, transient Python session state,
and live Rust audio state duplicate several performance concepts. Before clock/scheduler and DSP
foundation work, the project needs a documented ownership model for state restore, audio
telemetry, and future parameter authority.

This change records that boundary and moves audio telemetry dispatch out of the UI context into
controller-owned runtime event polling.

## What Changes

- Define `ProjectState` as durable performer intent and `SessionState` as a transient projection.
- Define Rust audio-thread state as the live authority for active voices, playheads, scheduler,
  transport, loaded buffers, prepared stems, and future smoothed DSP parameters.
- Document project restore ordering and acknowledgement/reconciliation expectations.
- Route audio-to-control telemetry through `AppController.poll_runtime_events()` before it mutates
  Python session projections.
- Keep the UI layer as the caller that requests polling during render, not the owner of telemetry
  message dispatch.

## Impact

- Affected specs: `project-persistence`, `ring-buffer-messaging`.
- Affected docs: `docs/audio-state-ownership.md`,
  `docs/audio-performance-architecture-audit.md`, `docs/audio-engine.md`,
  `docs/message-passing.md`.
- Affected code: Python controller/UI context event routing and focused controller/UI tests.

## Non-Goals

- No new EQ implementation.
- No new DSP/FX implementation.
- No VST, LV2, CLAP, AU, or other plugin-hosting infrastructure.
- No MIDI latency or jitter rework.
- No transport BPM unification in this change; that remains the next clock/scheduler stage.
- No Python DSP, GIL access, disk I/O, logging, blocking waits, neural inference, or heavy
  allocation in the audio callback.
