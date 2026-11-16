## Why

A numeric change during native serialization and parsing can cause the App to
reject a usable approximate tempo. The existing policies can also exhaust their
candidates without trying the conventional beat-interval calculation.

## What Changes

- Preserve the original native numbers when attaching approximate evidence.
- Try conventional QM beat tracking only after the existing selection has failed.
- Display the conventional scalar as **Approximate BPM** and keep incomplete or
  unusable results out of effective pad tempo.

## Impact

- Affected spec: audio-analysis
- Affected code: native automatic analysis, approximate tempo replay, and pad feedback
