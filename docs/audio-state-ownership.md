# Audio State Ownership

Date: 2026-05-25

Status: Stage 4 architecture preparation. This document does not implement EQ, DSP effects,
plugin hosting, transport BPM unification, or new user controls.

## Purpose

This document defines the boundary between durable Python intent, transient Python session
projections, and live Rust audio state before clock/scheduler and DSP foundation work continues.

This boundary is not a prohibition on moving focused responsibilities from Python to Rust. If a
Python-side function currently influences live audio timing, transport/BPM authority, scheduling,
loop/source-frame conversion, playback-rate application, or future realtime parameter state, a
small Rust migration is allowed when it reduces duplicated live truth and preserves the protected
callback boundary.

## Ownership Table

| Area | Durable Python `ProjectState` | Transient Python `SessionState` | Live Rust audio state |
| --- | --- | --- | --- |
| Sample identity | Project-local `sample_paths`, durations, analysis metadata | loading ids, pending paths, progress/errors | immutable loaded sample buffers and slot ownership |
| Pad activity | no live playback persistence | best-effort active/paused pad projection | active voices, pause/render state, source playheads |
| Metering and playheads | none | best-effort peaks and playhead positions | rendered levels and source/output positions |
| BPM metadata | manual BPM overrides and analysis BPM | BPM-lock anchor and displayed master BPM projection | per-pad BPM parameter and accepted master BPM shared by BPM-lock tempo matching and transport-grid timing |
| Speed and pitch | global speed multiplier | edit buffers and display projection | current playback ratio and per-voice smoothing state |
| Key Lock | enabled flag and bounded settings | UI/editor state only | Key Lock mode, settings, per-voice processor state |
| Loop points | editable seconds, auto-loop flag, bar count, grid offset samples | waveform editor view state | loop frame region and playhead wrapping after publication |
| Stem cache | cache metadata, availability, durable full-mix/all-stems preference | generation progress/errors and component mask display state | accepted prepared-stem handles, mode, enabled component mask |
| Gain/EQ | per-pad gain and current EQ dB intent | UI gesture state only | current scalar gain/EQ parameters in mixer state |
| Future DSP parameters | normalized persisted intent where applicable | UI edit/learn projections | smoothed parameter targets and DSP node state |
| Transport/quantization | trigger quantization enabled/step | UI display/edit projections | output-frame timeline, downbeat anchor, scheduler |

## Restore Ordering

Startup follows this control-plane order:

1. Load and validate `ProjectState` from project persistence.
2. Create a fresh `SessionState`.
3. Start `AudioEngine`.
4. Construct controllers.
5. Publish restored global audio settings, per-pad gain/EQ, loop regions, pad BPM/timing metadata,
   BPM-lock state, Key Lock state, and trigger quantization through bounded Rust APIs.
6. Validate restored stem cache metadata in Python.
7. Schedule cached sample loads from project-local paths.
8. Publish input-mapping runtime state and in-memory mapping snapshots.

Persistence, sample path validation, stem cache validation, and sample loading remain outside the
audio callback.

## Telemetry And Reconciliation

Rust owns live playback truth. Python `SessionState` stores a recoverable projection for UI and
controller decisions.

Controller-owned runtime polling handles these audio-to-control messages:

- `SampleStarted` updates `SessionState.active_sample_ids`.
- `SampleStopped` clears active and paused projections for that pad.
- `PadPeak` updates best-effort metering.
- `PadPlayhead` updates best-effort playhead display.

The UI may request polling during rendering, but telemetry message dispatch belongs to the
controller. Dropped or delayed audio-to-control messages must not mutate durable project intent.
Current recovery remains action-based: unload clears the affected session projections, and later
audio telemetry can refresh playback indicators. A future follow-up may add an explicit
audio-state snapshot or acknowledgement path if best-effort telemetry becomes insufficient for a
specific workflow.

## Stage 4 Decision

For the next stages, assume these authorities:

- Rust is authoritative for live active voices, transport time, scheduler state, source playheads,
  loop wrapping, loaded buffers, prepared stem handles, and future DSP smoothing.
- Python `ProjectState` is authoritative for persisted performer intent.
- Python `SessionState` is not persisted and must be safe to rebuild.
- Future DSP and clock work should avoid adding new duplicated authorities. New controls should
  cross the Python/Rust boundary either as durable intent that is restored or as transient
  controller/session projection that can be recomputed.
- Future Python-to-Rust migrations should transfer one bounded authority at a time. Behavior
  changes require OpenSpec coverage, focused tests, and realtime-safety review. Python may still
  store and restore durable intent, but Rust should own the live audio interpretation whenever the
  value affects sample-frame timing or callback-side rendering state.
- Stage 5 applies that policy to master BPM: Python keeps durable BPM intent and session display
  projections, while accepted master-BPM updates become the shared Rust live tempo for both
  BPM-lock tempo matching and transport-grid timing.
