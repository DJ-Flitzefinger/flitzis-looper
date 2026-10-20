## Why

Changing rate receipts currently add and remove wrapped rows above the Pitch
fader. This moves its drag geometry under the pointer and can toggle scrollbars,
making a stationary pointer produce a different speed while the controls flicker.
The Key Lock start settings also introduce German labels into the English UI.

## What Changes

- Keep the Pitch fader and adjacent controls independent of asynchronous rate
  and Key Lock status text, with a bounded height based on the sidebar viewport.
- Show requested and confirmed rate details through the BPM display tooltip,
  retaining the distinction between intent and confirmed audio.
- Use English labels and explanations for both Key Lock start policies.
- Prove unchanged geometry and stationary-pointer drag behavior with actual
  ImGui frames without opening an AudioEngine or device.

## Non-goals

No rate, pitch, quantization, Key Lock policy, MIDI, scheduler or DSP changes.
No graphics backend, App, audio device or listening acceptance is claimed.
All work stays in Python UI rendering; no callback allocation, lock or GIL work
is added. Central console diagnostics are covered by the separate diagnostic change.
