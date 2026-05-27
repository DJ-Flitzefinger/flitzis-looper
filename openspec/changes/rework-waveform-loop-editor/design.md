# Design: Waveform loop editor rework

## Loop state model

Loop intent remains durable project state owned by Python controllers. The bar count becomes a
finite numeric value so it can represent `0.5`, integers, and values reached by exact `1.0` bar
steps. Existing projects with integer bar counts load as equivalent numeric values.

Newly loaded tracks initialize their loop intent to:

- `loop_start_sec = 0.0`,
- `auto_loop_enabled = true`,
- `auto_loop_bars = 8.0`.

The analysis downbeat/beat onset remains useful for musical grid anchoring and snapping, but it no
longer replaces the loaded-track loop start default.

## ALL action

`ALL` replaces the old Reset action. Because auto-loop derives loop end from BPM and bar count,
`ALL` stores the full track as an explicit manual region:

- `loop_start_sec = 0.0`,
- `loop_end_sec = sample_duration_s`,
- `auto_loop_enabled = false`.

The stored bar count may remain unchanged because it is ignored while auto-loop is disabled. The
controller applies the effective region to Rust immediately.

## Bar stepping and bounds

The UI exposes two stepping modes on the bar arrow controls:

- left mouse down follows the musical sequence `0.5, 1, 2, 4, 8, 16, 32, ...`;
- right mouse down subtracts or adds exactly `1.0` bar.

The controller owns validation. Accepted values must be finite, at least `0.5`, and no larger than
the maximum loop length that fits from the current loop start to the loaded sample duration at the
effective BPM. If BPM or duration is unavailable, or if the requested value does not fit, the
change is a no-op.

## Waveform gestures

Empty plot left-click remains a fast loop-start shortcut. It sets the selected pad's loop start
using the same controller-owned snapping and sample quantization as marker dragging, then retriggers
only the selected pad from the new effective loop start. It must not stop other active pads and
must not become a generic playback seek shortcut.

Middle mouse down sends one immediate seek request for the selected pad. If the performer keeps
holding and drags with the middle button, the view can continue to pan after the initial seek; the
drag must not send repeated seeks or modify loop markers.

## Audio seek semantics

The seek command is a bounded Rust control message carrying pad id and source position in seconds.
The Python controller validates pad id and finite target seconds before crossing the PyO3 boundary.

Rust applies the seek only to an active or paused voice for that pad. A stopped pad has no voice to
seek and the request is a no-op. Paused voices remain paused after the seek.

The voice needs transient seek-origin behavior so explicit out-of-loop seeks are not immediately
clamped to the loop start by the existing render path:

- seeking before the loop start plays forward until the loop start, then loops normally at loop end;
- seeking inside the loop uses normal loop wrapping;
- seeking after the loop end plays forward until the track end, then jumps to loop start and loops
  normally.

Live loop-region edits keep the existing clamp-to-loop-start behavior for out-of-range playheads.
This change does not add a live loop-edit crossfade.

## Transport controls

Waveform editor transport actions remain selected-pad-only and route through `UiContext` into
controllers. The render code owns only widget layout and input-to-action mapping.

Play left mouse down retriggers the selected pad from loop start without stopping other pads. Play
right mouse down stops only the selected pad. Pause left mouse down toggles pause/resume. Pause
right mouse down records transient hold state, pauses immediately, and resumes on right release
only if that hold caused the pause.

## Window state

Waveform maximize/restore state is transient UI/session state. It should not be saved as durable
project intent unless a later spec explicitly asks for persistence.

Toolbar hit target sizing is a UI presentation concern and must stay out of controller and Rust
audio logic.

## Realtime safety

All heavy work remains outside the audio callback. The callback may read bounded scalar seek state
and update voice playheads, but it must not perform disk I/O, JSON work, Python/GIL access, UI
calls, blocking locks, logging, neural inference, plugin scanning/loading, unbounded loops, heavy
allocation, or long-running work.
