# Design: 3-band DJ isolator EQ replacement

## Scope Boundary

This planning slice only defines the replacement design. The implementation should happen as a
later small slice under this change after strict OpenSpec validation passes.

The replacement should keep the existing performer-facing EQ concept: every pad has low, mid, and
high controls, the selected-pad sidebar remains the editing surface, and middle-click reset still
returns a band to neutral. The runtime authority changes: accepted live EQ targets should become
normalized Rust DSP parameters owned by a per-pad isolator node, not standalone mixer EQ
coefficients.

## Processing Position

The isolator node should live in the existing per-pad Rust DSP chain. The intended order is:

```text
full-mix or prepared-stem source selection
-> source-frame loop wrap and voice playhead
-> playback-rate and Key Lock processing
-> optional future per-stem processing
-> per-pad DSP chain with isolator node
-> per-pad gain, voice velocity, master volume, metering, telemetry
```

During the replacement, the old hardwired `RtMixer.pad_eq` and per-voice `Eq3State` path should
not remain active in parallel with the isolator. Compatibility methods such as `set_pad_eq(...)`
may remain as Python-facing wrappers, but accepted live targets should be converted to typed DSP
parameter identities before audio processing.

## Parameter Model

The isolator should use three normalized accepted live targets:

- low: `0.0` full kill, `0.5` neutral, `1.0` limited boost,
- mid: `0.0` full kill, `0.5` neutral, `1.0` limited boost,
- high: `0.0` full kill, `0.5` neutral, `1.0` limited boost.

The UI and project state may keep the existing dB-oriented durable representation for compatibility
if that remains the smallest safe migration. In that case, the controller boundary should convert
durable values into normalized targets before publishing them to Rust. A later schema migration is
allowed only if it is OpenSpec-backed and preserves older projects.

The accepted targets should use the bounded parameter path, coalescing by parameter identity, and
Rust-owned smoothing before sample processing. Direct MIDI dispatch remains command-only; mapped
EQ knob movement should continue to derive bounded controller-owned target changes before entering
the Rust parameter path.

## Isolator Shape

The initial design target is a crossover-based 3-band DJ isolator:

- low band below `250 Hz`,
- mid band from `250 Hz` to `4 kHz`,
- high band above `4 kHz`,
- 4th-order Linkwitz-Riley-style crossover targets for stable recombination,
- neutral path transparent within floating-point tolerance,
- full-kill behavior at minimum for the selected band,
- smooth limited boost with a maximum of `+6 dB`,
- finite output for silence, impulses, sustained tones, and rapid target changes.

The implementation should choose coefficients and state layout that fit the existing Rust style
and fixed-size DSP-chain boundary. Any scratch buffers or topology preparation must be created
outside realtime rendering or stored in fixed-size Rust-owned state.

## Implementation Slices

A safe first implementation slice should be narrow:

1. Add the isolator node state and tests in Rust DSP code.
2. Route existing per-pad EQ setter targets into typed per-pad DSP parameter identities.
3. Render through the DSP-chain isolator while removing the old hardwired EQ processing from the
   live audio path.
4. Preserve existing UI controls, project restore, input-mapping action keys, and public Python
   behavior unless a focused migration is explicitly required.

Do not combine this with deck/group/master chains, new FX modules, new stem DSP, plugin hosting,
or UI redesign.

## Realtime Constraints

The CPAL audio callback must still avoid disk I/O, JSON reads/writes, Python/GIL access, UI calls,
blocking locks or waits, logging, neural inference, plugin loading/scanning, unbounded loops,
heavy allocation, and long-running work. The isolator node may only process already owned audio
buffers and fixed-size Rust state in the callback.

## Focused Review Outcome

The deterministic test-tone review keeps the implementation slice bounded and does not change the
runtime DSP code. Representative sine-tone checks at `48 kHz` show:

- all-band `+6 dB` boost is uniform at about `1.995x` RMS, matching the intended boost cap,
- mid kill around `1 kHz` strongly suppresses the band center,
- low kill around `60 Hz` still leaves substantial residual energy,
- high kill around `8 kHz` can exceed the neutral RMS instead of suppressing the high band.

Decision: the current ownership bridge and DSP-chain placement are valid, but the initial
crossover/summation topology is not ready for OpenSpec archive as the final DJ isolator behavior.
The next slice should tune the existing isolator topology or band reconstruction under this same
OpenSpec change before acceptance/archive. That follow-up must remain focused on the per-pad
isolator and must not add unrelated FX, plugin hosting, deck/group/master chains, live loop-edit
crossfades, real-time stem separation, or UI redesign.

## Focused Tuning Outcome

The tuning follow-up keeps the same per-pad Rust DSP-chain ownership, public compatibility setter,
normalized parameter model, and smoothing behavior. It replaces the previous residual
`dry - low - high` reconstruction with fixed-size Linkwitz-Riley-style splits for non-equal
isolator gains:

- low/above-low split at `250 Hz`,
- mid/high split at `4 kHz`,
- low branch alignment through the upper crossover before summation,
- exact dry-path output when all three gains are equal.

The equal-gain dry path preserves neutral transparency, all-band kill silence, and uniform
all-band `+6 dB` boost. The non-equal band split suppresses representative `60 Hz` low-kill and
`8 kHz` high-kill tones while preserving other-band audibility. No UI behavior, project restore
behavior, input-mapping semantics, plugin hosting, unrelated FX, deck/group/master chains,
real-time stem separation, live loop-edit crossfades, or broad rewrite is added by this tuning
slice.
