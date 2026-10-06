# Bind prepared stem publication to the loaded source

## Why
Path, size and mtime do not detect equal-sized source replacement. Stem decoding
and alignment also outlive their source snapshot; a delayed set can currently
reach a same-shaped replacement. Obsolete separator workers share output paths.

## What Changes
- Version original-file identity with full SHA256 content; retain the loader's
  digest and reject preparation for a different current file.
- Capture an opaque native source/request/timing-publication ticket before work,
  check it before and after preparation and at mixer adoption, and bind every
  prepared set to the actual immutable loaded buffer used for alignment.
- Isolate worker artifacts and publish only the current job's complete output.
- Invalidate legacy stat-only stem cache entries instead of promoting them.

## Non-goals And Realtime Constraints
G3b1 does not integrate AcceptedConstantTiming, TimingAdoptionGuard, binary64
period consumers, MIDI/Key Lock accepted revisions or accepted timing persistence.
It does not change manual/TAP/legacy BPM, the shared stem trajectory, wrap policy,
separator model or audible SYNC acceptance. Immutable copy-first original/decode
proof remains C1. Hashing, decoding and alignment run outside the callback; the
callback performs only constant-time pointer/atomic checks and existing retirement.
