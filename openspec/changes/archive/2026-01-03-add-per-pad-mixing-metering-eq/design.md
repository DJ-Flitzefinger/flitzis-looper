## Context
We add per-pad gain, per-pad EQ, and per-pad metering.

- UI is Python + ImGui via `imgui_bundle` (immediate-mode, per-frame render).
- Real-time audio runs in a CPAL callback in Rust. It must not allocate, block, or interact with Python/GIL.
- Control updates already flow Python → Rust via `ControlMessage` over an `rtrb` ring buffer.
- Non-real-time progress/analysis currently flows Rust → Python via `poll_loader_events()` (std mpsc + dicts).

This change introduces a *new* Rust → Python telemetry stream for per-pad peak levels and adds new per-pad control parameters (gain + EQ) that the audio mixer applies during mixing.

## Goals / Non-Goals

- Goals
  - Per-pad gain and per-pad 3-band EQ are adjustable for the *selected pad* in the left sidebar.
  - Per-pad level meter is visible inside each pad and updates smoothly enough for performance use.
  - Metering is performance-friendly: bounded update rate (~10 Hz max) and real-time safe.
  - No UI freezes; no added allocations in the CPAL callback.

- Non-Goals
  - Detailed metering (RMS, LUFS) or peak-hold with long decay.
  - Per-stem metering or EQ.
  - Persistence of gain/EQ settings (tracked elsewhere in legacy parity list; can be added later).

## Decisions

### Decision: Per-pad telemetry via audio→control ring buffer
Use the existing audio→control `rtrb` ring buffer (polled from Python each frame) for metering updates.

- Why: fixed-size, allocation-free on audio thread; aligns with existing real-time safety constraints.
- Shape: introduce a new `AudioMessage` variant carrying pad id and peak values.
- Rate limiting: audio thread emits at most one peak update per pad per interval, with a global cap of ~10 updates/sec per pad.

Alternative considered: reuse `poll_loader_events()` channel for metering.
- Rejected: current loader channel uses `std::sync::mpsc` and stringy dict payloads; not suited for high-rate/continuous telemetry.

### Decision: Apply gain and EQ in `RtMixer::render`
- Per-pad gain: multiply each voice’s contribution by `pad_gain[sample_id]` (in addition to `voice.volume` and global `volume`).
- Per-pad EQ: apply a 3-band EQ filter per active voice (or per pad) before mixing into output.

Key trade-off: filter state location.
- Per-voice filter state preserves correct behavior when multiple voices of the same pad overlap (if that can happen) and avoids cross-voice state interference.
- Per-pad filter state is cheaper but risks artifacts if multiple voices overlap. Given `MAX_VOICES` is small and real-time constraints matter, prefer per-voice unless we can guarantee one voice per pad.

### Decision: EQ implementation strategy
Evaluate `surgefx-eq3band` (surge-rs) for quality and performance.

- Option A: depend on the crate directly.
  - Pros: proven DSP, less maintenance.
  - Cons: dependency weight, potential compile features, API mismatch.
- Option B: copy minimal implementation into this repo.
  - Pros: full control, minimal deps.
  - Cons: we own correctness and future fixes.

Proposal stance: start with a small, real-time-safe EQ implementation in-tree unless `surgefx-eq3band` integrates cleanly (license/dep weight). During implementation, choose the best option based on build impact and API fit.

## Data Model

### Per-pad parameters (Python)
Add persistent per-pad arrays to `ProjectState`:
- `pad_gain: list[float]` (length `NUM_SAMPLES`, default `1.0`)
- `pad_eq_low_db: list[float]` (default `0.0`)
- `pad_eq_mid_db: list[float]` (default `0.0`)
- `pad_eq_high_db: list[float]` (default `0.0`)

Add runtime per-pad arrays to `SessionState` for metering:
- `pad_peak: list[float]` (mono, `0.0..=1.0`) updated from audio thread
- Timestamp/age to allow UI smoothing/decay

### Control messages (Rust)
Extend `ControlMessage` with:
- `SetPadGain { id, gain }`
- `SetPadEq { id, low_db, mid_db, high_db }`

### Audio messages (Rust)
Extend `AudioMessage` with:
- `PadPeak { id, peak }` (mono peak, post gain/EQ)

## UI Rendering

### Left sidebar: new “Mix / EQ” section
In `sidebar_left.py`, when a pad is selected and loaded:
- Show a Gain control.
- Show three knobs (low/mid/high) using `imgui_bundle`’s `imgui_knobs`.

### Pad grid: per-pad meter overlay
In `performance_view.py::_pad_button`, draw a small VU/peak meter overlay (e.g., a vertical bar at the right edge) using ImGui draw-list primitives.

Performance: use cached per-pad peak values from state; do not format strings per frame beyond what exists today.

## Risks / Trade-offs
- Audio-thread telemetry volume: even with rate limits, 216 pads could be noisy if all emit; ensure emission is only for active pads (or only for pads currently playing) to bound traffic.
- UI smoothing/decay: a simple exponential decay in Python state avoids needing high update rates.
- EQ CPU cost: per-voice filtering increases CPU load. Keep filter structure simple (biquads) and avoid per-sample allocations.

## Parameter Conventions

- Per-pad gain uses an idiomatic linear volume scalar in the range `0.0..=1.0` (unity at `1.0`).
- Metering uses a mono peak in the range `0.0..=1.0`.
- The meter indicates clipping by rendering the top segment in red when the reported peak reaches `1.0`.

## Remaining Open Questions
- Knob ranges: do we want +/- 12 dB, +/- 18 dB, or something else?
