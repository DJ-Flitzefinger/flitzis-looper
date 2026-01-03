## Context
A per-pad 3-band EQ already exists and is controlled from Python UI, with DSP applied in Rust during mixing. The current implementation uses a low shelf, mid peaking, and high shelf biquad chain. This produces musical tone shaping but does not behave like a DJ isolator: minimum settings still leak significant energy from neighboring bands.

The goal is a “Kill EQ” that can effectively remove an entire band (low/mid/high) while keeping the sum transparent when all gains are at unity.

## Goals / Non-Goals

- Goals
  - Provide a 3-band per-pad EQ that behaves like a DJ isolator.
  - Allow each band to reach a true kill (effective gain `0.0`).
  - Keep real-time safety constraints: no allocations or blocking in the audio callback.
  - Preserve good reconstruction: when all band gains are unity, output equals input (subject to floating point error).

- Non-Goals
  - Per-band frequency adjustment UI (crossover frequency knobs).
  - Variable slope selection (e.g., 12 dB/oct vs 24 dB/oct).
  - Adding new external DSP dependencies.

## Proposed DSP Approach

### Decision: Use Linkwitz–Riley 4th-order crossovers (LR4)
Use LR4 crossovers (two cascaded 2nd-order Butterworth stages) at two crossover frequencies.

Rationale:
- Flat summation at crossover points when recombining bands.
- Stable, common “mixer isolator” behavior.

### Decision: Parallel split + sum, with mid as residual
Split the input into low and high using LR4 low-pass and high-pass filters. Compute mid as a residual:
- `low = LP(input)`
- `high = HP(input)`
- `mid = input - low - high`

Rationale:
- Avoids needing a dedicated band-pass with additional phase constraints.
- Guarantees perfect reconstruction when gains are 1.0 (modulo floating point).

### Gain mapping (“kill”)
Band controls remain represented as dB values externally, but the system maps the minimum “Kill (−∞)” setting to a linear gain of `0.0` so the band can be fully removed.

Since the current control plumbing uses floats for per-pad dB values, represent “−∞” as a conventional minimum dB value (e.g., `-60 dB`) and treat that minimum as “kill” during DSP gain conversion:
- Values at or below the configured minimum dB map to linear `0.0`.
- Other values map to `10^(db/20)`.

This keeps existing state/message shapes while guaranteeing hard kill behavior.

## Architecture & State
- Filters MUST remain real-time safe and run in the mixer.
- Filter state location should follow existing voice handling:
  - Prefer per-voice EQ state to avoid state sharing artifacts when a pad can have overlapping voices.

## Validation Strategy (implementation stage)
- Rust unit tests:
  - Coefficients are finite for supported sample rates.
  - Processing remains stable on impulse/step signals.
  - Unity reconstruction: with all gains at `0.0 dB`, output is close to input.
- Python tests:
  - Updated clamping/validation for the expanded dB range.

## Decisions (confirmed)
- Knob range: DJ-style isolator range with **Kill (−∞)** at minimum and **+6 dB** boost at maximum.
- Crossover points: ~`300 Hz` (low/mid) and ~`3.5 kHz` (mid/high) to mirror common DJ mixer EQs.
