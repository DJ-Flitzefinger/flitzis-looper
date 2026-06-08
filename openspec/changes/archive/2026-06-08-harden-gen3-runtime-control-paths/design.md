# Design: Runtime Control Path Hardening

## Overview

The implementation should keep the existing Rust/Python ownership split. Rust remains responsible
for live audio truth and direct audio-safe MIDI dispatch. Python remains responsible for project
intent, UI state, persistence, Learn UX, and controller-owned fallback actions. The audio callback
continues to mix already available audio state only.

## Per-Pad Request Identity

Load, restore, and manual analysis work should capture a pad request identity when the request is
accepted. That identity can be a monotonic per-pad generation, source-version token, or equivalent
bounded value that is carried by Rust loader/task events and mirrored in Python pending state.

Completion handling should compare the event identity with the current pad identity before applying
the result. A stale success, error, progress, analysis, or publication result should be ignored for
state mutation. For Rust load publication, stale workers should be prevented from replacing the
sample cache or sending `ControlMessage::LoadSample` after a newer load or unload invalidates the
identity.

This work stays outside the audio callback. Request identities are generated and checked on control
or background-worker paths.

## MIDI Learn And Direct Dispatch

Direct Rust MIDI dispatch is useful only for normal mapped playback. Learn mode has higher
priority: when Learn is waiting for an input, the first accepted normalized MIDI event should be
reported to Python as capture input and must not enqueue a direct trigger, stop, or stop-all command.

Because the Rust input dispatcher currently cannot infer Python Learn state from mapping snapshots,
Python should publish a small input-runtime Learn/capture flag to Rust when Learn toggles. The
dispatcher can then bypass action dispatch and emit an input event for capture. This keeps MIDI
callback work bounded and avoids UI-frame mouse simulation.

For normal playback, direct dispatch remains all-or-nothing. If a direct command sequence cannot be
fully enqueued because the command ring is full or locked, the dispatcher should emit an event with
the action key and a not-dispatched status. Python can then execute controller-owned fallback
semantics outside the MIDI callback. Partial direct loop-and-play transactions remain forbidden.

## Must-Apply Publication Failures

Rust-facing setters should make their acceptance semantics explicit. Setters used for startup
restore, unload/reset neutralization, and one-shot ordered state changes should return an error when
the target command or parameter ring cannot accept the message. High-rate controls may still choose
best-effort behavior only when the caller and tests deliberately classify that write as best-effort
and surface diagnostics appropriately.

Implementation should centralize producer push helpers so command and parameter enqueue failures do
not get dropped by `_ = producer.push(...)`.

## Loaded-Pad-Aware Startup Projection

Startup restore should not publish per-pad live-audio settings for empty pads. Restored per-pad gain,
EQ, BPM, loop, timing metadata, Key Lock, stem mode, and stem mask state should be sent only for pads
with valid restored sample assignments, and only when the restored value differs from default or is
needed by the loaded pad's current behavior.

This does not remove explicit neutralization. When a pad is unloaded, cleared because its restored
file is missing, or force-reset by controller logic, the controller still sends the bounded
neutralizing commands needed to clear Rust live state for that pad.

## Internal Optimization Slices

The remaining approved proposals are internal implementation changes unless code review discovers a
visible contract change:

- remove no-op all-pad parameter work from the callback,
- publish input runtime state only when dirty,
- fix waveform render-data cache source identity,
- reduce callback telemetry all-pad scans without changing meter semantics,
- track active Python meter peaks instead of decaying all pads every frame,
- remove duplicate Rust DSP prepare work at startup.

Each implementation slice should preserve the callback boundary and add focused regression tests for
the affected module.
