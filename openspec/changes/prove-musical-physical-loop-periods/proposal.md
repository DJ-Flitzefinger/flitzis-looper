# G3c musical versus physical loop evidence

## Why

Current accepted timing preserves a binary64 source period, while productive
SourcePlayback repeats integer physical markers. A once-rounded endpoint can
therefore become a repeated period error. SourceGrid's compatible musical-cycle
diagnostics do not establish the duration of the rendered loop.

## What Changes

- Separate exact musical periods, once-rounded physical endpoints, actual source
  progression and rendered signal features in hardware-free accepted-owner tests.
- Preserve a strict musical acceptance probe alongside passing physical-output
  characterization; a failed musical probe leaves G3 incomplete.
- Record the exact productive correction and device/listening gates still needed.

## Non-goals and realtime constraints

This evidence slice does not change physical wrapping, introduce continuous SYNC,
activate an analyzer default, create a second timing owner or certify listening.
Fixture construction, file reads, hashing and exports remain test-only and outside
the callback. Production callback code remains bounded and unchanged. Immutable
copy-first/ABA lineage, B5 audible DSP compensation and current resource costs
retain their separate gates. No full application Rust-port work is included.
