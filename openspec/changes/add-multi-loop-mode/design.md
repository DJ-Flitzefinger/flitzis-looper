## Context
- The Rust audio engine already supports fixed-capacity voice mixing (`MAX_VOICES`) and looping playback inside the CPAL callback.
- The current Dear PyGui performance UI triggers pads via a standard button callback (typically firing on mouse release).
- There is no explicit legacy-style **MultiLoop** control, and pads do not currently indicate "active/playing" state.

## Goals / Non-Goals
- Goals:
  - Add a legacy-style **MultiLoop** toggle to switch between monophonic (“one-at-a-time”) and polyphonic looping.
  - Ensure loop onset happens on **mouse down** for maximum performance control.
  - Provide clear visual indication of which pads are currently active.
  - Keep the audio callback real-time safe (no allocations, no blocking) and avoid adding sync/quantization.
- Non-Goals:
  - Tempo sync / quantized start / BPM lock.
  - Persistence of the MultiLoop setting across restarts.
  - Anti-clipping/limiting or per-pad gain (handled by future mixing work).

## Decisions
- MultiLoop mode is a **global app state flag** (owned by the Python core/app layer) that influences how pad-trigger actions are translated into audio-engine commands.
- MultiLoop mode defaults to **disabled** on startup (legacy parity).
- "One-at-a-time" mode uses a single efficient engine call (`AudioEngine.stop_all()`) to stop other pads before triggering the new pad.
- The MultiLoop toggle control is positioned below the bank selector row (legacy placement).
- The UI’s "active pad" indicators are driven from **core state** (updated on trigger/stop/unload). This keeps UI logic thin and makes behavior testable without needing audio-thread → Python events.
- Pad onset and right-click stop are bound to **mouse-down** gestures rather than default button-click behavior. Implementation SHOULD prefer a Dear PyGui handler that fires on press; if per-item press handlers are insufficient, use a global mouse-down handler that checks which pad item is hovered and dispatches the appropriate action for that pad.

## Risks / Trade-offs
- Dear PyGui event semantics can vary by widget/handler; there is a risk of double-triggering (press + release). Mitigation: remove the default button callback for left-click triggering and use exactly one mouse-down pathway.
- The mixer has `MAX_VOICES = 32`, which is less than the pad count (36). If a performer tries to run >32 simultaneous loops, additional triggers may be dropped deterministically by the engine. This proposal keeps the existing bound and treats it as acceptable for now.
- Summing multiple loops can exceed `[-1.0, 1.0]` and clip. This change keeps the current straightforward summing behavior; limiting/normalization is deferred to future mixing controls.

## Migration Plan
- No data migrations. This feature is runtime-only.

## Open Questions
- None.
