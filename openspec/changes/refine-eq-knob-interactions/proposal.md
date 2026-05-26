## Why

The selected-pad EQ knobs currently map the full `-60..+6 dB` range linearly to the knob
rotation. That places the neutral `0.0 dB` value visually near 4 o'clock instead of at the
expected 12 o'clock position for DJ-style EQ controls.

The EQ value entry also allows invalid typed characters to briefly enter the input buffer before
they are ignored on commit, unlike the global BPM edit field.

## What Changes

- Map each selected-pad EQ knob through a neutral-centered visual position: `0.0 dB` at 12
  o'clock, `-60.0 dB` at the minimum kill position, and `+6.0 dB` at maximum boost.
- Preserve the current accepted target curve: positive values remain linear, while negative values
  keep the existing logarithmic kill curve stretched over the shorter negative half of the knob.
- Make right-click on an EQ knob immediately set that band to `-60.0 dB`.
- Make left-drag use vertical movement as the primary adjustment and horizontal movement at half
  that sensitivity for fine adjustment.
- Filter manual EQ value entry before characters enter the field, accepting only digits, `.`, and
  `,` with comma converted to dot.

## Non-Goals

- No change to Rust isolator DSP, crossover design, smoothing, parameter rings, or realtime audio
  callback behavior.
- No change to persisted EQ value range or project file compatibility.
- No new keyboard/MIDI mapping semantics beyond the existing per-pad EQ actions.

## Realtime Constraints

This change is UI/control-path only. It MUST NOT add work to the CPAL audio callback and MUST NOT
introduce disk I/O, Python/GIL access, UI calls, blocking operations, allocations, logging, neural
inference, or plugin scanning to the realtime audio path.
