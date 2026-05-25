# Design: Low-jitter input mapping

## Overview
Input mapping is split between Python-owned UX/persistence and a Rust-owned MIDI hot path.
Python remains responsible for Settings, Learn state, visual feedback, mapping JSON edits, and
keyboard focus rules. Rust owns MIDI capture, immediate monotonic timestamping, message
normalization, bounded queues, in-memory MIDI mapping lookup, and dispatch bridging outside the
audio callback.

## Hot Path
The MIDI callback receives backend messages, records a monotonic timestamp immediately, normalizes
supported messages, and tries to enqueue a compact event into a bounded channel. It does not log,
touch JSON, update UI, allocate more than required by the backend callback boundary, or call
Python.

The dispatcher thread resolves the event against the latest in-memory mapping snapshot. Direct
audio-safe commands, such as pad trigger/stop and stop-all, are forwarded through the existing
bounded control queue. Actions that require project/controller state are reported to Python as
typed event dictionaries for execution outside the hot path. Control Change events carry their
0..127 value through that dictionary so Python can implement relative controller-owned steps for
continuous controls, including endless-controller wraparound and repeated relative encoder values.
The MIDI normalizer also tracks NRPN parameter-select Control Changes and reports Data
Increment/Data Decrement messages as stable `midi:nrpn:<channel>:<parameter>` bindings with common
`65`/`63` increment/decrement values, without exposing those values to the audio callback.

The audio callback remains unchanged in responsibility: it only consumes bounded control messages
and mixes already available audio data.

## Supported Inputs
- MIDI Note On with velocity greater than zero.
- MIDI Control Change.
- NRPN increment/decrement encoded through Control Change parameter select and
  increment/decrement messages.
- Keyboard key name plus normalized modifiers.

Note On velocity zero is treated as Note Off and ignored for Learn/playback. Active Sensing,
MIDI Clock, SysEx, Program Change, Pitch Bend, Aftertouch, and MPE-style messages are ignored in
version 1.

## Mapping Boundary
Python stores mappings in `config/input/keyboard.json` and `config/input/midi.json`. On load,
save, delete, clear-all, and ON/OFF changes, Python publishes a new in-memory MIDI mapping snapshot
to Rust. Normal mapped playback never reads or writes JSON.

Mappings use stable string keys at the boundary:
- MIDI binding: `midi:<note|cc>:<channel>:<number>`
- Keyboard binding: `keyboard:<key>:<ctrl><alt><shift><super>`
- Action: stable LooperAction key, for example `pad.trigger:0` or `ui.select_bank:2`

## Real-time Safety
Rust input/control modules are allowed outside the audio callback. They may own MIDI ports,
bounded channels, mapping snapshots, and dispatcher threads. The audio callback must not own or
perform those tasks.

The Rust input layer must not become a direct callback path into the audio callback. It may only
enqueue bounded control messages through the established producer side that already separates
control work from rendering.
